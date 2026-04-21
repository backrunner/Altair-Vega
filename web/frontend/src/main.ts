import { WasmBrowserNode, generate_short_code, hash_bytes_hex, normalize_short_code } from 'altair-vega-browser'
import { RENDEZVOUS_PATH } from '../../rendezvous-protocol'

type BrowserEvent =
  | { type: 'ready'; endpointId: string }
  | { type: 'receivedMessage'; endpointId: string; body: string }
  | { type: 'sentMessage'; endpointId: string; body: string }
  | {
      type: 'receivedFileChunk'
      endpointId: string
      transferId: number
      chunkIndex: number
      name: string
      sizeBytes: number
      chunkSizeBytes: number
      chunkBytes: number
      bytesComplete: number
      hashHex: string
      mimeType: string
    }
  | {
      type: 'receivedFileCompleted'
      endpointId: string
      transferId: number
      name: string
      sizeBytes: number
      chunkSizeBytes: number
      hashHex: string
      mimeType: string
    }
  | {
      type: 'sentFile'
      endpointId: string
      transferId: number
      name: string
      sizeBytes: number
    }
  | { type: 'error'; message: string }

type RawBrowserEvent = {
  type?: string
  endpointId?: string
  endpoint_id?: string
  body?: string
  message?: string
  transferId?: number
  transfer_id?: number
  chunkIndex?: number
  chunk_index?: number
  name?: string
  sizeBytes?: number
  size_bytes?: number
  chunkSizeBytes?: number
  chunk_size_bytes?: number
  chunkBytes?: number
  chunk_bytes?: number
  bytesComplete?: number
  bytes_complete?: number
  hashHex?: string
  hash_hex?: string
  mimeType?: string
  mime_type?: string
}

type PresenceMessage = {
  type: 'presence'
  endpointId: string
  announcedAt: number
  requestReply: boolean
}

type RendezvousEvent =
  | { type: 'snapshot'; peers: Array<{ endpointId: string; connectedAt: number; peerType?: string; label?: string }> }
  | { type: 'peer-joined'; endpointId: string; connectedAt: number; peerType?: string; label?: string }
  | { type: 'peer-left'; endpointId: string }
  | { type: 'relay'; fromEndpointId: string; payload: ResumeRelayPayload }

type RoomConnection = {
  close: () => void
  sendRelay: (toEndpointId: string, payload: ResumeRelayPayload) => void
}

type ResumeQueryPayload = {
  type: 'resume-query'
  requestId: string
  sourceEndpointId: string
  storageKey: string
  hashHex: string
  sizeBytes: number
  chunkSizeBytes: number
  name: string
  mimeType: string
}

type ResumeReplyPayload = {
  type: 'resume-reply'
  requestId: string
  storageKey: string
  localBytes: number
  missingRanges: Array<{ start: number; end: number }>
}

type ResumeRelayPayload = ResumeQueryPayload | ResumeReplyPayload

type PersistedReceivedFile = ReceivedFile & {
  storedAt: number
}

type PersistedReceivedFileChunk = {
  chunkKey: string
  storageKey: string
  chunkIndex: number
  chunkBytes: number
  bytes: ArrayBuffer
}

const STORAGE_DB_NAME = 'altair-vega-web'
const STORAGE_DB_VERSION = 2
const RECEIVED_FILE_STORE = 'received-files'
const RECEIVED_FILE_CHUNK_STORE = 'received-file-chunks'
const LAST_CODE_STORAGE_KEY = 'altair-vega:last-code'
const RENDEZVOUS_URL_STORAGE_KEY = 'altair-vega:rendezvous-url'

const app = document.querySelector<HTMLDivElement>('#app')!

type Peer = {
  endpointId: string
  lastSeenAt: number
  peerType?: string
  label?: string
}

type ReceivedFile = {
  storageKey: string
  transferId: number
  endpointId: string
  name: string
  sizeBytes: number
  bytesComplete: number
  chunkSizeBytes: number
  hashHex: string
  mimeType: string
  storedAt: number
  completed: boolean
}

const state = {
  node: null as WasmBrowserNode | null,
  endpointId: '',
  code: window.localStorage.getItem(LAST_CODE_STORAGE_KEY) ?? generate_short_code(),
  roomConnection: null as RoomConnection | null,
  peers: new Map<string, Peer>(),
  selectedPeerId: '',
  composeText: '',
  selectedFile: null as File | null,
  devChunkLimit: '' as string,
  receivedFiles: [] as ReceivedFile[],
  events: [] as Array<{ title: string; detail: string }>,
  announceTimer: 0 as number | 0,
  pendingResumeQueries: new Map<
    string,
    { resolve: (value: ResumeReplyPayload) => void; reject: (reason?: unknown) => void; timeout: number }
  >(),
}

await boot()

async function boot() {
  state.node = await WasmBrowserNode.spawn()
  state.endpointId = state.node.endpoint_id()
  state.receivedFiles = await listReceivedFiles()
  if (state.receivedFiles.length > 0) {
    appendEvent('Restored received files', `${state.receivedFiles.length} file(s) loaded from browser storage`)
  }
  attachEventStream(state.node)
  joinCode(state.code)
  render()
}

function render() {
  const peers = [...state.peers.values()].sort((a, b) => b.lastSeenAt - a.lastSeenAt)
  const receivedFiles = [...state.receivedFiles].reverse()
  const currentPeer = state.selectedPeerId || 'none selected'

  app.innerHTML = `
    <div class="shell">
      <section class="card">
        <div class="row" style="justify-content: space-between;">
          <div>
            <h1>Altair Vega Web</h1>
            <p class="muted">Static-hosted browser endpoint with local short-code rendezvous for development.</p>
          </div>
          <div class="status ${state.node ? '' : 'offline'}">${state.node ? 'Browser endpoint online' : 'Offline'}</div>
        </div>
      </section>

      <section class="grid">
        <div class="card">
          <h2>Local Endpoint</h2>
          <p><code>${state.endpointId || 'spawning...'}</code></p>
          <p class="muted">During development, peers discover each other through the local room service on this dev server, keyed by the typed short code below.</p>
        </div>
        <div class="card">
          <h2>Pairing Code</h2>
          <label for="pair-code">Type the same short code in another tab on the same origin.</label>
          <div class="row">
            <input id="pair-code" value="${escapeHtml(state.code)}" />
            <button id="normalize-code" class="secondary">Normalize</button>
            <button id="regenerate-code" class="secondary">Generate</button>
            <button id="join-code">Join Code</button>
          </div>
        </div>
      </section>

      <section class="grid">
        <div class="card">
          <h2>Peers In Current Code</h2>
          <p class="muted">Discovered peers are announced through the local development room service.</p>
          <ul class="peer-list">
            ${peers.length === 0 ? '<li class="peer-item muted">No peers discovered yet.</li>' : peers
              .map((peer) => `
                <li class="peer-item">
                  <div class="row" style="justify-content: space-between; align-items: flex-start;">
                    <div>
                      <div><code>${peer.endpointId}</code></div>
                      <div class="muted">${escapeHtml(peer.label ?? peer.peerType ?? 'peer')}</div>
                      <div class="muted">Last seen ${formatRelative(peer.lastSeenAt)}</div>
                    </div>
                    <button class="secondary select-peer" data-peer-id="${peer.endpointId}">${peer.endpointId === state.selectedPeerId ? 'Selected' : 'Select'}</button>
                  </div>
                </li>
              `)
              .join('')}
          </ul>
        </div>

        <div class="card">
          <h2>Send Message</h2>
          <p class="muted">Selected peer: <code>${currentPeer}</code></p>
          <textarea id="compose-text" placeholder="Write a message to the selected peer...">${escapeHtml(state.composeText)}</textarea>
          <div class="row">
            <button id="send-message" ${state.selectedPeerId ? '' : 'disabled'}>Send Over iroh</button>
          </div>
          <hr style="border: 0; border-top: 1px solid rgba(156, 174, 255, 0.14); margin: 18px 0;" />
          <h2>Send File</h2>
          <p class="muted">Current file: <code>${state.selectedFile ? escapeHtml(state.selectedFile.name) : 'none selected'}</code></p>
          <input id="file-input" type="file" />
          <label for="chunk-limit" class="muted" style="display:block; margin-top: 12px;">Dev chunk limit per send attempt</label>
          <input id="chunk-limit" inputmode="numeric" placeholder="all chunks" value="${escapeHtml(state.devChunkLimit)}" />
          <div class="row" style="margin-top: 12px;">
            <button id="send-file" ${state.selectedPeerId && state.selectedFile ? '' : 'disabled'}>Send File</button>
          </div>
        </div>
      </section>

      <section class="card">
        <h2>Received Files</h2>
        <ul class="event-list">
          ${receivedFiles.length === 0 ? '<li class="event-item muted">No files received yet.</li>' : receivedFiles
            .map((file) => `
              <li class="event-item">
                <div><strong>${escapeHtml(file.name)}</strong></div>
                <div class="muted">From <code>${escapeHtml(file.endpointId)}</code></div>
                <div class="muted">${formatBytes(file.bytesComplete)} / ${formatBytes(file.sizeBytes)} • ${escapeHtml(file.hashHex.slice(0, 16))}... • stored ${formatRelative(file.storedAt)}</div>
                <div class="row" style="margin-top: 10px;">
                  <button class="secondary download-file" data-storage-key="${escapeHtml(file.storageKey)}" ${file.completed ? '' : 'disabled'}>Download</button>
                  <button class="secondary delete-file" data-storage-key="${escapeHtml(file.storageKey)}">Remove</button>
                </div>
              </li>
            `)
            .join('')}
        </ul>
      </section>

      <section class="card">
        <h2>Event Log</h2>
        <ul class="event-list">
          ${state.events.length === 0 ? '<li class="event-item muted">No events yet.</li>' : state.events
            .map((event) => `
              <li class="event-item">
                <strong>${escapeHtml(event.title)}</strong>
                <div class="muted">${escapeHtml(event.detail)}</div>
              </li>
            `)
            .join('')}
        </ul>
      </section>
    </div>
  `

  bindControls()
}

function bindControls() {
  const codeInput = document.querySelector<HTMLInputElement>('#pair-code')!
  const composeInput = document.querySelector<HTMLTextAreaElement>('#compose-text')!
  const fileInput = document.querySelector<HTMLInputElement>('#file-input')!
  const chunkLimitInput = document.querySelector<HTMLInputElement>('#chunk-limit')!

  codeInput.addEventListener('input', () => {
    state.code = codeInput.value
  })

  composeInput.addEventListener('input', () => {
    state.composeText = composeInput.value
  })

  fileInput.addEventListener('change', () => {
    state.selectedFile = fileInput.files?.[0] ?? null
    render()
  })

  chunkLimitInput.addEventListener('input', () => {
    state.devChunkLimit = chunkLimitInput.value
  })

  document.querySelector<HTMLButtonElement>('#normalize-code')!.onclick = async () => {
    try {
      state.code = normalize_short_code(codeInput.value)
      appendEvent('Normalized code', state.code)
      render()
    } catch (error) {
      appendEvent('Normalize failed', stringifyError(error))
      render()
    }
  }

  document.querySelector<HTMLButtonElement>('#regenerate-code')!.onclick = () => {
    state.code = generate_short_code()
    appendEvent('Generated code', state.code)
    render()
  }

  document.querySelector<HTMLButtonElement>('#join-code')!.onclick = () => {
    try {
      joinCode(codeInput.value)
      render()
    } catch (error) {
      appendEvent('Join failed', stringifyError(error))
      render()
    }
  }

  document.querySelectorAll<HTMLButtonElement>('.select-peer').forEach((button) => {
    button.onclick = () => {
      state.selectedPeerId = button.dataset.peerId || ''
      render()
    }
  })

  document.querySelector<HTMLButtonElement>('#send-message')!.onclick = async () => {
    if (!state.node || !state.selectedPeerId || !state.composeText.trim()) {
      return
    }
    try {
      await state.node.send_message(state.selectedPeerId, state.composeText.trim())
      state.composeText = ''
      render()
    } catch (error) {
      appendEvent('Send failed', stringifyError(error))
      render()
    }
  }

  document.querySelector<HTMLButtonElement>('#send-file')!.onclick = async () => {
    if (!state.node || !state.selectedPeerId || !state.selectedFile) {
      return
    }
    try {
      const bytes = new Uint8Array(await state.selectedFile.arrayBuffer())
      const hashHex = hash_bytes_hex(bytes)
      const resume = await requestResumeInfo(state.selectedPeerId, {
        storageKey: makeStorageKey(state.endpointId, hashHex),
        hashHex,
        sizeBytes: bytes.byteLength,
        chunkSizeBytes: 256 * 1024,
        name: state.selectedFile.name,
        mimeType: state.selectedFile.type || 'application/octet-stream',
      })
      const missingRanges = limitMissingRanges(
        resume?.missingRanges ?? fullMissingRanges(bytes.byteLength, 256 * 1024),
        parseChunkLimit(state.devChunkLimit),
      )
      await state.node.send_file_with_ranges(
        state.selectedPeerId,
        state.selectedFile.name,
        state.selectedFile.type || 'application/octet-stream',
        bytes,
        missingRanges,
      )
      appendEvent('Queued file send', `${state.selectedFile.name} to ${state.selectedPeerId}`)
      render()
    } catch (error) {
      appendEvent('Send file failed', stringifyError(error))
      render()
    }
  }

  document.querySelectorAll<HTMLButtonElement>('.download-file').forEach((button) => {
    button.onclick = async () => {
      const storageKey = button.dataset.storageKey ?? ''
      const file = state.receivedFiles.find((entry) => entry.storageKey === storageKey)
      if (!file) {
        return
      }
      try {
        const [manifest, chunks] = await Promise.all([
          loadReceivedFileManifest(storageKey),
          loadReceivedFileChunks(storageKey),
        ])
        if (!manifest || chunks.length === 0) {
          appendEvent('Download failed', `Stored file ${file.name} is no longer available`)
          render()
          return
        }
        const bytes = joinChunks(chunks, manifest.sizeBytes)
        const blob = new Blob([bytes.buffer], {
          type: file.mimeType || 'application/octet-stream',
        })
        const url = URL.createObjectURL(blob)
        const anchor = document.createElement('a')
        anchor.href = url
        anchor.download = file.name
        anchor.click()
        URL.revokeObjectURL(url)
        appendEvent('Downloaded received file', file.name)
      } catch (error) {
        appendEvent('Download failed', stringifyError(error))
      }
    }
  })

  document.querySelectorAll<HTMLButtonElement>('.delete-file').forEach((button) => {
    button.onclick = async () => {
      const storageKey = button.dataset.storageKey ?? ''
      const file = state.receivedFiles.find((entry) => entry.storageKey === storageKey)
      if (!file) {
        return
      }
      try {
        await deleteReceivedFile(storageKey)
        state.receivedFiles = state.receivedFiles.filter((entry) => entry.storageKey !== storageKey)
        appendEvent('Removed stored file', file.name)
        render()
      } catch (error) {
        appendEvent('Remove failed', stringifyError(error))
        render()
      }
    }
  })
}

function joinCode(rawCode: string) {
  const normalized = normalize_short_code(rawCode)
  state.code = normalized
  window.localStorage.setItem(LAST_CODE_STORAGE_KEY, normalized)
  state.peers.clear()
  state.selectedPeerId = ''
  state.roomConnection?.close()
  if (state.announceTimer) {
    window.clearInterval(state.announceTimer)
  }
  state.roomConnection = connectRoom(normalized)
  appendEvent('Joined code', normalized)
  render()
}

function connectRoom(code: string): RoomConnection {
  const wsUrl = getRendezvousUrl()
  wsUrl.searchParams.set('code', code)
  wsUrl.searchParams.set('endpointId', state.endpointId)
  wsUrl.searchParams.set('peerType', 'browser-web')
  wsUrl.searchParams.set('label', 'Browser Web')

  const socket = new WebSocket(wsUrl)
  let closedIntentionally = false

  socket.addEventListener('open', () => {
    appendEvent('Connected to room service', code)
    render()
  })

  socket.addEventListener('message', (event) => {
    const message = JSON.parse(String(event.data)) as RendezvousEvent
    handleRendezvousEvent(message)
  })

  socket.addEventListener('close', () => {
    if (closedIntentionally) {
      return
    }
    appendEvent('Room service disconnected', code)
    render()
    fallbackToBroadcastChannel(code)
  })

  socket.addEventListener('error', () => {
    appendEvent('Room service unavailable', 'Falling back to same-browser BroadcastChannel mode.')
    render()
  })

  return {
    close() {
      closedIntentionally = true
      socket.close()
    },
    sendRelay(toEndpointId, payload) {
      socket.send(
        JSON.stringify({
          type: 'relay',
          toEndpointId,
          payload,
        }),
      )
    },
  }
}

function getRendezvousUrl() {
  const queryValue = new URLSearchParams(window.location.search).get('rendezvous')?.trim()
  if (queryValue) {
    window.localStorage.setItem(RENDEZVOUS_URL_STORAGE_KEY, queryValue)
    return new URL(queryValue)
  }

  const storedValue = window.localStorage.getItem(RENDEZVOUS_URL_STORAGE_KEY)?.trim()
  if (storedValue) {
    return new URL(storedValue)
  }

  const url = new URL(RENDEZVOUS_PATH, window.location.origin)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  return url
}

function fallbackToBroadcastChannel(code: string) {
  const channel = new BroadcastChannel(`altair-vega-dev::${code}`)
  channel.onmessage = (event: MessageEvent<PresenceMessage>) => {
    if (event.data.type !== 'presence' || event.data.endpointId === state.endpointId) {
      return
    }
    upsertPeer(event.data.endpointId, event.data.announcedAt)
    if (event.data.requestReply) {
      broadcastPresence(channel, false)
    }
    render()
  }
  broadcastPresence(channel, true)
  state.announceTimer = window.setInterval(() => broadcastPresence(channel, false), 5000)
  state.roomConnection = {
    close() {
      channel.close()
    },
    sendRelay() {
      appendEvent('Resume relay unavailable', 'Fallback BroadcastChannel mode does not support resumable negotiation.')
    },
  }
}

function broadcastPresence(channel: BroadcastChannel, requestReply: boolean) {
  channel.postMessage({
    type: 'presence',
    endpointId: state.endpointId,
    announcedAt: Date.now(),
    requestReply,
  } satisfies PresenceMessage)
}

function handleRendezvousEvent(event: RendezvousEvent) {
  switch (event.type) {
    case 'snapshot':
      state.peers.clear()
      for (const peer of event.peers) {
        upsertPeer(peer.endpointId, peer.connectedAt, peer.peerType, peer.label)
      }
      break
    case 'peer-joined':
      upsertPeer(event.endpointId, event.connectedAt, event.peerType, event.label)
      break
    case 'peer-left':
      if (state.peers.delete(event.endpointId)) {
        appendEvent('Peer left', event.endpointId)
      }
      if (state.selectedPeerId === event.endpointId) {
        state.selectedPeerId = ''
      }
      break
    case 'relay':
      handleRelayPayload(event.fromEndpointId, event.payload)
      break
  }
  render()
}

function upsertPeer(endpointId: string, connectedAt: number, peerType?: string, label?: string) {
  if (endpointId === state.endpointId) {
    return
  }
  const existingPeer = state.peers.get(endpointId)
  state.peers.set(endpointId, {
    endpointId,
    lastSeenAt: connectedAt,
    peerType,
    label,
  })
  if (!existingPeer) {
    appendEvent('Discovered peer', endpointId)
  }
}

async function requestResumeInfo(
  targetEndpointId: string,
  descriptor: Omit<ResumeQueryPayload, 'type' | 'requestId' | 'sourceEndpointId'>,
): Promise<ResumeReplyPayload | null> {
  if (!state.roomConnection) {
    return null
  }

  const requestId = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const result = new Promise<ResumeReplyPayload>((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      state.pendingResumeQueries.delete(requestId)
      reject(new Error('resume query timed out'))
    }, 4000)
    state.pendingResumeQueries.set(requestId, { resolve, reject, timeout })
  })

  state.roomConnection.sendRelay(targetEndpointId, {
    type: 'resume-query',
    requestId,
    sourceEndpointId: state.endpointId,
    ...descriptor,
  })

  try {
    return await result
  } catch (error) {
    appendEvent('Resume query failed', stringifyError(error))
    return null
  }
}

async function handleRelayPayload(fromEndpointId: string, payload: ResumeRelayPayload) {
  switch (payload.type) {
    case 'resume-query': {
      const resume = await getResumeInfo(payload.storageKey, payload.sizeBytes, payload.chunkSizeBytes)
      state.roomConnection?.sendRelay(fromEndpointId, {
        type: 'resume-reply',
        requestId: payload.requestId,
        storageKey: payload.storageKey,
        localBytes: resume.localBytes,
        missingRanges: resume.missingRanges,
      })
      break
    }
    case 'resume-reply': {
      const pending = state.pendingResumeQueries.get(payload.requestId)
      if (!pending) {
        return
      }
      window.clearTimeout(pending.timeout)
      state.pendingResumeQueries.delete(payload.requestId)
      pending.resolve(payload)
      break
    }
  }
}

async function attachEventStream(node: WasmBrowserNode) {
  const reader = node.events().getReader()
  while (true) {
    const { done, value } = await reader.read()
    if (done) {
      appendEvent('Browser stream closed', 'No further browser events will be emitted.')
      render()
      return
    }

    const event = normalizeBrowserEvent(value as RawBrowserEvent)
    switch (event.type) {
      case 'ready':
        appendEvent('Browser endpoint ready', event.endpointId)
        break
      case 'receivedMessage':
        appendEvent(`Message from ${event.endpointId}`, event.body)
        break
      case 'sentMessage':
        appendEvent(`Message sent to ${event.endpointId}`, event.body)
        break
      case 'receivedFileChunk':
        upsertReceivedFile(await persistReceivedChunk(event, node))
        break
      case 'receivedFileCompleted':
        upsertReceivedFile(await markReceivedFileCompleted(event))
        appendEvent(`File received from ${event.endpointId}`, event.name)
        break
      case 'sentFile':
        appendEvent(`File sent to ${event.endpointId}`, `${event.name} (${formatBytes(event.sizeBytes)})`)
        break
      case 'error':
        appendEvent('Browser error', event.message)
        break
    }
    render()
  }
}

function normalizeBrowserEvent(event: RawBrowserEvent): BrowserEvent {
  const endpointId = event.endpointId ?? event.endpoint_id ?? ''
  switch (event.type) {
    case 'ready':
      return { type: 'ready', endpointId }
    case 'receivedMessage':
      return { type: 'receivedMessage', endpointId, body: event.body ?? '' }
    case 'sentMessage':
      return { type: 'sentMessage', endpointId, body: event.body ?? '' }
    case 'receivedFileChunk':
      return {
        type: 'receivedFileChunk',
        endpointId,
        transferId: event.transferId ?? event.transfer_id ?? 0,
        chunkIndex: event.chunkIndex ?? event.chunk_index ?? 0,
        name: event.name ?? 'received.bin',
        sizeBytes: event.sizeBytes ?? event.size_bytes ?? 0,
        chunkSizeBytes: event.chunkSizeBytes ?? event.chunk_size_bytes ?? 0,
        chunkBytes: event.chunkBytes ?? event.chunk_bytes ?? 0,
        bytesComplete: event.bytesComplete ?? event.bytes_complete ?? 0,
        hashHex: event.hashHex ?? event.hash_hex ?? '',
        mimeType: event.mimeType ?? event.mime_type ?? 'application/octet-stream',
      }
    case 'receivedFileCompleted':
      return {
        type: 'receivedFileCompleted',
        endpointId,
        transferId: event.transferId ?? event.transfer_id ?? 0,
        name: event.name ?? 'received.bin',
        sizeBytes: event.sizeBytes ?? event.size_bytes ?? 0,
        chunkSizeBytes: event.chunkSizeBytes ?? event.chunk_size_bytes ?? 0,
        hashHex: event.hashHex ?? event.hash_hex ?? '',
        mimeType: event.mimeType ?? event.mime_type ?? 'application/octet-stream',
      }
    case 'sentFile':
      return {
        type: 'sentFile',
        endpointId,
        transferId: event.transferId ?? event.transfer_id ?? 0,
        name: event.name ?? 'sent.bin',
        sizeBytes: event.sizeBytes ?? event.size_bytes ?? 0,
      }
    case 'error':
      return { type: 'error', message: event.message ?? 'Unknown browser event error' }
    default:
      return { type: 'error', message: `Unknown browser event type: ${String(event.type)}` }
  }
}

function appendEvent(title: string, detail: string) {
  state.events.unshift({ title, detail })
  state.events = state.events.slice(0, 24)
}

function upsertReceivedFile(file: ReceivedFile) {
  state.receivedFiles = state.receivedFiles.filter((item) => item.storageKey !== file.storageKey)
  state.receivedFiles.push(file)
}

function stringifyError(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function formatRelative(timestamp: number) {
  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000))
  return `${seconds}s ago`
}

function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`
}

async function persistReceivedChunk(
  event: Extract<BrowserEvent, { type: 'receivedFileChunk' }>,
  node: WasmBrowserNode,
): Promise<ReceivedFile> {
  const bytes = node.take_received_chunk(BigInt(event.transferId), BigInt(event.chunkIndex))
  const stableBytes = new Uint8Array(bytes.byteLength)
  stableBytes.set(bytes)

  const storageKey = makeStorageKey(event.endpointId, event.hashHex)
  const existing = await loadReceivedFileManifest(storageKey)
  const existingChunk = await loadReceivedChunk(storageKey, event.chunkIndex)
  const bytesComplete = (existing?.bytesComplete ?? 0) + (existingChunk ? 0 : event.chunkBytes)
  const record: PersistedReceivedFile = {
    storageKey,
    transferId: event.transferId,
    endpointId: event.endpointId,
    name: event.name,
    sizeBytes: event.sizeBytes,
    bytesComplete,
    chunkSizeBytes: event.chunkSizeBytes,
    hashHex: event.hashHex,
    mimeType: event.mimeType,
    storedAt: existing?.storedAt ?? Date.now(),
    completed: bytesComplete >= event.sizeBytes,
  }
  const chunk: PersistedReceivedFileChunk = {
    chunkKey: makeChunkKey(storageKey, event.chunkIndex),
    storageKey,
    chunkIndex: event.chunkIndex,
    chunkBytes: event.chunkBytes,
    bytes: stableBytes.buffer.slice(0),
  }
  await storeReceivedChunk(record, chunk)
  return record
}

async function markReceivedFileCompleted(
  event: Extract<BrowserEvent, { type: 'receivedFileCompleted' }>,
): Promise<ReceivedFile> {
  const storageKey = makeStorageKey(event.endpointId, event.hashHex)
  const existing = await loadReceivedFileManifest(storageKey)
  const record: PersistedReceivedFile = {
    storageKey,
    transferId: event.transferId,
    endpointId: event.endpointId,
    name: event.name,
    sizeBytes: event.sizeBytes,
    bytesComplete: event.sizeBytes,
    chunkSizeBytes: event.chunkSizeBytes,
    hashHex: event.hashHex,
    mimeType: event.mimeType,
    storedAt: existing?.storedAt ?? Date.now(),
    completed: true,
  }
  await storeReceivedManifest(record)
  return record
}

async function listReceivedFiles(): Promise<ReceivedFile[]> {
  const db = await openStorage()
  const tx = db.transaction(RECEIVED_FILE_STORE, 'readonly')
  const store = tx.objectStore(RECEIVED_FILE_STORE)
  const rows = await requestToPromise<PersistedReceivedFile[]>(store.getAll())
  await transactionDone(tx)
  return rows.sort((a, b) => a.storedAt - b.storedAt)
}

async function loadReceivedFileManifest(storageKey: string): Promise<PersistedReceivedFile | null> {
  const db = await openStorage()
  const tx = db.transaction(RECEIVED_FILE_STORE, 'readonly')
  const store = tx.objectStore(RECEIVED_FILE_STORE)
  const row = await requestToPromise<PersistedReceivedFile | undefined>(store.get(storageKey))
  await transactionDone(tx)
  return row ?? null
}

async function storeReceivedManifest(record: PersistedReceivedFile): Promise<void> {
  const db = await openStorage()
  const tx = db.transaction(RECEIVED_FILE_STORE, 'readwrite')
  tx.objectStore(RECEIVED_FILE_STORE).put(record)
  await transactionDone(tx)
}

async function storeReceivedChunk(
  record: PersistedReceivedFile,
  chunk: PersistedReceivedFileChunk,
): Promise<void> {
  const db = await openStorage()
  const tx = db.transaction([RECEIVED_FILE_STORE, RECEIVED_FILE_CHUNK_STORE], 'readwrite')
  tx.objectStore(RECEIVED_FILE_STORE).put(record)
  tx.objectStore(RECEIVED_FILE_CHUNK_STORE).put(chunk)
  await transactionDone(tx)
}

async function loadReceivedFileChunks(storageKey: string): Promise<PersistedReceivedFileChunk[]> {
  const db = await openStorage()
  const tx = db.transaction(RECEIVED_FILE_CHUNK_STORE, 'readonly')
  const store = tx.objectStore(RECEIVED_FILE_CHUNK_STORE)
  const index = store.index('storageKey')
  const rows = await requestToPromise<PersistedReceivedFileChunk[]>(index.getAll(storageKey))
  await transactionDone(tx)
  return rows.sort((a, b) => a.chunkIndex - b.chunkIndex)
}

async function getResumeInfo(
  storageKey: string,
  sizeBytes: number,
  chunkSizeBytes: number,
): Promise<{ localBytes: number; missingRanges: Array<{ start: number; end: number }> }> {
  const manifest = await loadReceivedFileManifest(storageKey)
  if (!manifest) {
    return {
      localBytes: 0,
      missingRanges: chunkCount(sizeBytes, chunkSizeBytes) === 0
        ? []
        : [{ start: 0, end: chunkCount(sizeBytes, chunkSizeBytes) }],
    }
  }

  const chunks = await loadReceivedFileChunks(storageKey)
  const present = new Set(chunks.map((chunk) => chunk.chunkIndex))
  const totalChunks = chunkCount(sizeBytes, chunkSizeBytes)
  const missingRanges: Array<{ start: number; end: number }> = []
  let rangeStart: number | null = null
  for (let index = 0; index < totalChunks; index += 1) {
    if (!present.has(index)) {
      rangeStart ??= index
      continue
    }
    if (rangeStart != null) {
      missingRanges.push({ start: rangeStart, end: index })
      rangeStart = null
    }
  }
  if (rangeStart != null) {
    missingRanges.push({ start: rangeStart, end: totalChunks })
  }
  return {
    localBytes: manifest.bytesComplete,
    missingRanges,
  }
}

async function loadReceivedChunk(
  storageKey: string,
  chunkIndex: number,
): Promise<PersistedReceivedFileChunk | null> {
  const db = await openStorage()
  const tx = db.transaction(RECEIVED_FILE_CHUNK_STORE, 'readonly')
  const store = tx.objectStore(RECEIVED_FILE_CHUNK_STORE)
  const row = await requestToPromise<PersistedReceivedFileChunk | undefined>(
    store.get(makeChunkKey(storageKey, chunkIndex)),
  )
  await transactionDone(tx)
  return row ?? null
}

async function deleteReceivedFile(storageKey: string): Promise<void> {
  const db = await openStorage()
  const tx = db.transaction([RECEIVED_FILE_STORE, RECEIVED_FILE_CHUNK_STORE], 'readwrite')
  tx.objectStore(RECEIVED_FILE_STORE).delete(storageKey)
  const chunkStore = tx.objectStore(RECEIVED_FILE_CHUNK_STORE)
  const index = chunkStore.index('storageKey')
  const rows = await requestToPromise<PersistedReceivedFileChunk[]>(index.getAll(storageKey))
  for (const row of rows) {
    chunkStore.delete(row.chunkKey)
  }
  await transactionDone(tx)
}

function openStorage(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(STORAGE_DB_NAME, STORAGE_DB_VERSION)
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(RECEIVED_FILE_STORE)) {
        db.createObjectStore(RECEIVED_FILE_STORE, { keyPath: 'storageKey' })
      }
      if (!db.objectStoreNames.contains(RECEIVED_FILE_CHUNK_STORE)) {
        const chunkStore = db.createObjectStore(RECEIVED_FILE_CHUNK_STORE, {
          keyPath: 'chunkKey',
        })
        chunkStore.createIndex('storageKey', 'storageKey', { unique: false })
      }
    }
    request.onerror = () => reject(request.error ?? new Error('failed to open IndexedDB'))
    request.onsuccess = () => resolve(request.result)
  })
}

function requestToPromise<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onerror = () => reject(request.error ?? new Error('IndexedDB request failed'))
    request.onsuccess = () => resolve(request.result)
  })
}

function transactionDone(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error ?? new Error('IndexedDB transaction failed'))
    tx.onabort = () => reject(tx.error ?? new Error('IndexedDB transaction aborted'))
  })
}

function makeStorageKey(endpointId: string, hashHex: string) {
  return `${endpointId}:${hashHex}`
}

function makeChunkKey(storageKey: string, chunkIndex: number) {
  return `${storageKey}:${chunkIndex}`
}

function chunkCount(sizeBytes: number, chunkSizeBytes: number) {
  if (sizeBytes === 0) {
    return 0
  }
  return Math.ceil(sizeBytes / chunkSizeBytes)
}

function fullMissingRanges(sizeBytes: number, chunkSizeBytes: number) {
  const totalChunks = chunkCount(sizeBytes, chunkSizeBytes)
  return totalChunks === 0 ? [] : [{ start: 0, end: totalChunks }]
}

function parseChunkLimit(value: string): number | null {
  const parsed = Number(value.trim())
  return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : null
}

function limitMissingRanges(
  ranges: Array<{ start: number; end: number }>,
  limit: number | null,
) {
  if (!limit) {
    return ranges
  }

  const result: Array<{ start: number; end: number }> = []
  let remaining = limit
  for (const range of ranges) {
    if (remaining <= 0) {
      break
    }
    const span = range.end - range.start
    if (span <= remaining) {
      result.push(range)
      remaining -= span
      continue
    }
    result.push({ start: range.start, end: range.start + remaining })
    break
  }
  return result
}

function joinChunks(chunks: PersistedReceivedFileChunk[], totalSize: number) {
  const result = new Uint8Array(totalSize)
  let offset = 0
  for (const chunk of chunks) {
    const bytes = new Uint8Array(chunk.bytes)
    result.set(bytes, offset)
    offset += bytes.byteLength
  }
  return result
}

function escapeHtml(value: string | number | null | undefined) {
  const text = value == null ? '' : String(value)
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
}
