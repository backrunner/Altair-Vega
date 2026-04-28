import { onMount, onCleanup, Show } from 'solid-js'
import { generate_short_code, normalize_short_code, hash_bytes_hex } from 'altair-vega-browser'
import {
  state,
  setNode,
  saveCode,
  setCode,
  clearPeers,
  setConnectionState,
  setRoomConnection,
  upsertPeer,
  removePeer,
  setReceivedFiles,
  upsertReceivedFile,
  addChatMessage,
  updateFileInChat,
  updateLatestFileInChat,
  clearChat,
  addToast,
  setSidebarOpen,
} from './lib/state'
import { spawnNode, normalizeBrowserEvent, persistReceivedChunk, markReceivedFileCompleted } from './lib/wasm'
import { connectRoom } from './lib/rendezvous'
import { handleRelayPayload, requestResumeInfo } from './lib/resume'
import { listReceivedFiles } from './lib/storage'
import { makeStorageKey, fullMissingRanges, generateId, stringifyError } from './lib/format'
import type { RendezvousEvent, RawBrowserEvent, BrowserEvent } from './lib/types'
import { cx } from './lib/cx'

import Header from './components/Header'
import CodeInput from './components/CodeInput'
import PeerBanner from './components/PeerBanner'
import ChatThread from './components/ChatThread'
import ComposeBar from './components/ComposeBar'
import Toast from './components/Toast'
import SidebarFiles from './components/SidebarFiles'

const appLayoutClass = [
  'mx-auto flex h-[100dvh] max-h-[100dvh] min-h-0 w-[min(1240px,100%)] flex-col overflow-hidden',
  'gap-[var(--space-2)] p-[var(--space-2)]',
  'min-[769px]:gap-[var(--space-4)] min-[769px]:p-[var(--space-3)]',
  'min-[901px]:p-[var(--space-4)]',
].join(' ')

const appMainClass = [
  'relative grid min-h-0 flex-1 grid-cols-1 gap-0 overflow-hidden',
  'min-[769px]:grid-cols-[288px_minmax(0,1fr)] min-[769px]:gap-[var(--space-3)]',
  'min-[901px]:grid-cols-[304px_minmax(0,1fr)] min-[901px]:gap-[var(--space-4)]',
].join(' ')

const sidebarBackdropClass = 'fixed inset-0 z-[100] block border-none bg-[rgba(0,0,0,0.34)] min-[769px]:hidden'

const sidebarClass = [
  'fixed bottom-[var(--space-2)] left-[var(--space-2)] top-[76px] z-[110]',
  'flex min-h-0 min-w-0 w-[min(318px,calc(100vw-var(--space-4)))]',
  'translate-x-[calc(-100%-var(--space-4))] flex-col gap-[var(--space-3)]',
  'overflow-hidden border border-[var(--color-border)]',
  'rounded-[var(--radius-module)] bg-[var(--color-surface)] p-[var(--space-2)] shadow-[var(--shadow-lg)]',
  'transition-transform duration-[var(--duration-normal)] ease-[var(--ease-out)]',
  'min-[769px]:static min-[769px]:z-auto min-[769px]:w-auto min-[769px]:translate-x-0',
  'min-[769px]:border-none min-[769px]:rounded-none min-[769px]:bg-transparent',
  'min-[769px]:p-0 min-[769px]:pr-[2px] min-[769px]:shadow-none',
].join(' ')

const sidebarOpenClass = 'translate-x-0'

const sidebarPanelsClass = 'flex min-h-0 min-w-0 flex-1 flex-col gap-[var(--space-3)] overflow-hidden'

const appChatClass = [
  'flex min-h-0 min-w-0 flex-col overflow-hidden border border-[var(--color-border)]',
  'rounded-[var(--radius-module)] bg-[var(--color-surface)] shadow-[var(--shadow-sm)]',
].join(' ')

export default function App() {
  let announceTimer = 0

  onMount(async () => {
    document.documentElement.setAttribute('data-theme', state.theme)
    document.documentElement.style.colorScheme = state.theme
    try {
      const node = await spawnNode()
      setNode(node)
      const files = await listReceivedFiles()
      if (files.length > 0) setReceivedFiles(files)
      attachEventStream(node)
    } catch (err) {
      setConnectionState('error')
      addToast('error', `Failed to start: ${err instanceof Error ? err.message : String(err)}`)
    }
  })

  onCleanup(() => {
    if (announceTimer) window.clearInterval(announceTimer)
    state.roomConnection?.close()
  })

  async function joinCode(rawCode: string) {
    if (!state.node) {
      addToast('warning', 'Browser node is still starting')
      return
    }

    let normalized: string
    try { normalized = normalize_short_code(rawCode) }
    catch { addToast('error', 'Invalid code format'); return }
    saveCode(normalized)
    clearPeers()
    clearChat()
    state.roomConnection?.close()
    if (announceTimer) { window.clearInterval(announceTimer); announceTimer = 0 }
    setConnectionState('connecting')

    let endpointTicket: string | undefined
    try {
      endpointTicket = await state.node.endpoint_ticket()
    } catch (err) {
      addToast('warning', `Endpoint ticket unavailable: ${stringifyError(err)}`)
    }

    const conn = connectRoom(normalized, state.endpointId, endpointTicket, {
      onOpen() { setConnectionState('connected'); addToast('success', `Joined room`) },
      onEvent(event: RendezvousEvent) { handleRendezvousEvent(event) },
      onClose(_code, reason) { setConnectionState('fallback'); addToast('warning', reason ?? 'Room service disconnected, using local fallback') },
      onError() { addToast('warning', 'Room service connection failed') },
      onReconnecting(_code, attempt, delayMs) {
        setConnectionState('reconnecting')
        addToast('warning', `Reconnecting to room service (${attempt}) in ${Math.round(delayMs / 1000)}s`)
      },
      onFallbackPresence(endpointId, announcedAt, fallbackEndpointTicket) {
        setConnectionState('fallback')
        if (upsertPeer(endpointId, announcedAt, undefined, undefined, fallbackEndpointTicket)) addToast('info', 'Peer discovered')
      },
      onFallbackRelayUnavailable() { addToast('warning', 'Resume not available in local fallback mode') },
    })
    setRoomConnection(conn)
  }

  function disconnectRoom(showToast = true) {
    state.roomConnection?.close()
    setRoomConnection(null)
    clearPeers()
    if (announceTimer) { window.clearInterval(announceTimer); announceTimer = 0 }
    setConnectionState('disconnected')
    if (showToast) addToast('info', 'Disconnected')
  }

  function handleRendezvousEvent(event: RendezvousEvent) {
    switch (event.type) {
      case 'snapshot':
        clearPeers()
        for (const peer of event.peers) upsertPeer(peer.endpointId, peer.connectedAt, peer.peerType, peer.label, peer.endpointTicket)
        break
      case 'peer-joined':
        if (upsertPeer(event.endpointId, event.connectedAt, event.peerType, event.label, event.endpointTicket)) addToast('info', 'Peer joined')
        break
      case 'peer-left':
        if (removePeer(event.endpointId)) addToast('info', 'Peer left')
        break
      case 'relay':
        handleRelayPayload(event.fromEndpointId, event.payload, state.roomConnection)
        break
    }
  }

  async function attachEventStream(node: any) {
    const reader = node.events().getReader()
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      await handleBrowserEvent(normalizeBrowserEvent(value as RawBrowserEvent), node)
    }
  }

  async function handleBrowserEvent(event: BrowserEvent, node: any) {
    switch (event.type) {
      case 'ready': break
      case 'receivedMessage':
        addChatMessage({ id: generateId(), direction: 'received', variant: 'text', peerEndpointId: event.endpointId, timestamp: Date.now(), text: event.body })
        break
      case 'sentMessage': break
      case 'receivedFileChunk': {
        const file = await persistReceivedChunk(event, node)
        upsertReceivedFile(file)
        updateFileInChat(file.storageKey, {
          bytesComplete: file.bytesComplete,
          completed: file.completed,
          failed: false,
          error: undefined,
        })
        if (event.chunkIndex === 0) {
          addChatMessage({ id: generateId(), direction: 'received', variant: 'file', peerEndpointId: event.endpointId, timestamp: Date.now(),
            fileTransfer: { name: event.name, sizeBytes: event.sizeBytes, bytesComplete: event.bytesComplete, hashHex: event.hashHex, mimeType: event.mimeType, completed: false, storageKey: makeStorageKey(event.endpointId, event.hashHex) },
          })
        }
        break
      }
      case 'receivedFileCompleted': {
        const file = await markReceivedFileCompleted(event)
        upsertReceivedFile(file)
        updateFileInChat(file.storageKey, {
          bytesComplete: file.sizeBytes,
          completed: true,
          failed: false,
          error: undefined,
        })
        addToast('success', `Received ${event.name}`)
        break
      }
      case 'sentFileChunk':
        updateFileInChat(makeStorageKey(state.endpointId, event.hashHex), {
          bytesComplete: event.bytesComplete,
          completed: event.bytesComplete >= event.sizeBytes,
          failed: false,
          error: undefined,
        })
        break
      case 'sentFile': addToast('success', `Sent ${event.name}`); break
      case 'error': {
        const fileError = parseBrowserFileError(event.message)
        if (fileError) {
          updateLatestFileInChat(fileError.endpointId, 'received', {
            completed: false,
            failed: true,
            error: fileError.detail,
          })
        }
        addToast('error', event.message)
        break
      }
    }
  }

  async function handleSendMessage(text: string) {
    if (!state.node || !state.selectedPeerId) return
    const peer = selectedPeer()
    try {
      if (peer?.endpointTicket) {
        await state.node.send_message_to_ticket(peer.endpointTicket, text)
      } else {
        await state.node.send_message(state.selectedPeerId, text)
      }
      addChatMessage({ id: generateId(), direction: 'sent', variant: 'text', peerEndpointId: state.selectedPeerId, timestamp: Date.now(), text })
    } catch (err) {
      addToast('error', `Send failed: ${err instanceof Error ? err.message : String(err)}`)
    }
  }

  async function handleSendFile(file: File) {
    if (!state.node || !state.selectedPeerId) return
    const peer = selectedPeer()
    let storageKey: string | null = null
    try {
      const bytes = new Uint8Array(await file.arrayBuffer())
      const hashHex = hash_bytes_hex(bytes)
      storageKey = makeStorageKey(state.endpointId, hashHex)
      const mimeType = file.type || 'application/octet-stream'
      const blobUrl = URL.createObjectURL(new Blob([bytes], { type: mimeType }))
      addChatMessage({ id: generateId(), direction: 'sent', variant: 'file', peerEndpointId: state.selectedPeerId, timestamp: Date.now(),
        fileTransfer: { name: file.name, sizeBytes: bytes.byteLength, bytesComplete: 0, hashHex, mimeType, completed: bytes.byteLength === 0, storageKey, blobUrl },
      })
      const resume = await requestResumeInfo(state.roomConnection, state.endpointId, state.selectedPeerId, {
        storageKey, hashHex, sizeBytes: bytes.byteLength, chunkSizeBytes: 256 * 1024, name: file.name, mimeType,
      })
      const missingRanges = resume?.missingRanges ?? fullMissingRanges(bytes.byteLength, 256 * 1024)
      if (peer?.endpointTicket) {
        await state.node.send_file_to_ticket_with_ranges(peer.endpointTicket, file.name, mimeType, bytes, missingRanges)
      } else {
        await state.node.send_file_with_ranges(state.selectedPeerId, file.name, mimeType, bytes, missingRanges)
      }
      updateFileInChat(storageKey, {
        bytesComplete: bytes.byteLength,
        completed: true,
        failed: false,
        error: undefined,
      })
    } catch (err) {
      const message = stringifyError(err)
      if (storageKey) {
        updateFileInChat(storageKey, {
          completed: false,
          failed: true,
          error: message,
        })
      }
      addToast('error', `File send failed: ${message}`)
    }
  }

  function selectedPeer() {
    return state.peers.find((peer) => peer.endpointId === state.selectedPeerId)
  }

  function parseBrowserFileError(message: string): { endpointId: string; detail: string } | null {
    const match = message.match(/^browser file connection error from ([^:]+):\s*(.*)$/)
    if (!match) return null
    return {
      endpointId: match[1],
      detail: match[2] || message,
    }
  }

  function handleGenerateCode() { setCode(generate_short_code()) }
  function handleConnect() { void joinCode(state.code) }
  function handleDisconnect() { disconnectRoom() }
  function handleReconnect() { void joinCode(state.code) }

  return (
    <div class={appLayoutClass}>
      <Header
        onConnect={handleConnect}
        onDisconnect={handleDisconnect}
        onReconnect={handleReconnect}
      />

      <main class={appMainClass}>
        {/* Mobile backdrop */}
        <Show when={state.sidebarOpen}>
          <button class={sidebarBackdropClass} type="button" aria-label="Close sidebar" onClick={() => setSidebarOpen(false)} />
        </Show>

        <aside id="app-sidebar" class={cx(sidebarClass, state.sidebarOpen && sidebarOpenClass)}>
          <CodeInput code={state.code} onCodeChange={setCode} onGenerate={handleGenerateCode} />
          <div class={sidebarPanelsClass}>
            <PeerBanner />
            <SidebarFiles />
          </div>
        </aside>

        <div class={appChatClass}>
          <ChatThread />
          <ComposeBar onSendMessage={handleSendMessage} onSendFile={handleSendFile} />
        </div>
      </main>

      <Toast />
    </div>
  )
}
