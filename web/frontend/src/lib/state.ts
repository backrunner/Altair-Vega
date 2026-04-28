import { createStore, produce } from 'solid-js/store'
import type {
  Peer,
  ReceivedFile,
  RoomConnection,
  ChatMessage,
  ConnectionState,
  ToastMessage,
} from './types'
import { WasmBrowserNode, generate_short_code } from 'altair-vega-browser'

const LAST_CODE_STORAGE_KEY = 'altair-vega:last-code'

export type AppState = {
  node: WasmBrowserNode | null
  endpointId: string
  code: string
  connectionState: ConnectionState
  roomConnection: RoomConnection | null
  peers: Peer[]
  selectedPeerId: string
  receivedFiles: ReceivedFile[]
  chatMessages: Record<string, ChatMessage[]>
  toasts: ToastMessage[]
  theme: 'light' | 'dark'
  settingsOpen: boolean
  sidebarOpen: boolean
}

function getInitialTheme(): 'light' | 'dark' {
  const stored = window.localStorage.getItem('altair-vega:theme')
  if (stored === 'light' || stored === 'dark') return stored
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

const [state, setState] = createStore<AppState>({
  node: null,
  endpointId: '',
  code: window.localStorage.getItem(LAST_CODE_STORAGE_KEY) ?? generate_short_code(),
  connectionState: 'starting',
  roomConnection: null,
  peers: [],
  selectedPeerId: '',
  receivedFiles: [],
  chatMessages: {},
  toasts: [],
  theme: getInitialTheme(),
  settingsOpen: false,
  sidebarOpen: false,
})

export { state }

export function setNode(node: WasmBrowserNode) {
  setState('node', node)
  setState('endpointId', node.endpoint_id())
  setState('connectionState', 'ready')
}

export function setCode(code: string) {
  setState('code', code)
}

export function saveCode(code: string) {
  setState('code', code)
  window.localStorage.setItem(LAST_CODE_STORAGE_KEY, code)
}

export function clearSavedCode() {
  window.localStorage.removeItem(LAST_CODE_STORAGE_KEY)
}

export function setConnectionState(s: ConnectionState) {
  setState('connectionState', s)
}

export function setRoomConnection(conn: RoomConnection | null) {
  setState('roomConnection', conn)
}

export function clearPeers() {
  setState('peers', [])
  setState('selectedPeerId', '')
}

export function upsertPeer(
  endpointId: string,
  connectedAt: number,
  peerType?: string,
  label?: string,
  endpointTicket?: string,
): boolean {
  const selfId = state.endpointId
  if (endpointId === selfId) return false

  const existing = state.peers.find((p) => p.endpointId === endpointId)
  if (existing) {
    setState(
      produce((s) => {
        const peer = s.peers.find((p) => p.endpointId === endpointId)
        if (peer) {
          peer.lastSeenAt = connectedAt
          if (peerType !== undefined) peer.peerType = peerType
          if (label !== undefined) peer.label = label
          if (endpointTicket !== undefined) peer.endpointTicket = endpointTicket
        }
      }),
    )
    return false
  }

  setState(
    produce((s) => {
      s.peers.push({ endpointId, lastSeenAt: connectedAt, peerType, label, endpointTicket })
    }),
  )

  // Auto-select if only peer
  if (state.peers.length === 1) {
    setState('selectedPeerId', endpointId)
  }

  return true
}

export function removePeer(endpointId: string): boolean {
  const existed = state.peers.some((p) => p.endpointId === endpointId)
  if (!existed) return false

  setState(
    produce((s) => {
      s.peers = s.peers.filter((p) => p.endpointId !== endpointId)
      if (s.selectedPeerId === endpointId) {
        s.selectedPeerId = s.peers.length === 1 ? s.peers[0].endpointId : ''
      }
    }),
  )
  return true
}

export function selectPeer(endpointId: string) {
  setState('selectedPeerId', endpointId)
}

export function setReceivedFiles(files: ReceivedFile[]) {
  setState('receivedFiles', files)
}

export function upsertReceivedFile(file: ReceivedFile) {
  setState(
    produce((s) => {
      s.receivedFiles = s.receivedFiles.filter((f) => f.storageKey !== file.storageKey)
      s.receivedFiles.push(file)
    }),
  )
}

export function removeReceivedFile(storageKey: string) {
  setState(
    produce((s) => {
      s.receivedFiles = s.receivedFiles.filter((f) => f.storageKey !== storageKey)
    }),
  )
}

export function addChatMessage(msg: ChatMessage) {
  const peerId = msg.peerEndpointId
  setState(
    produce((s) => {
      if (!s.chatMessages[peerId]) {
        s.chatMessages[peerId] = []
      }
      s.chatMessages[peerId].push(msg)
    }),
  )
}

export function updateFileInChat(storageKey: string, update: Partial<NonNullable<ChatMessage['fileTransfer']>>) {
  setState(
    produce((s) => {
      for (const peerId of Object.keys(s.chatMessages)) {
        const msgs = s.chatMessages[peerId]
        const msg = [...msgs].reverse().find(
          (m) => m.variant === 'file' && m.fileTransfer?.storageKey === storageKey,
        )
        if (msg?.fileTransfer) {
          Object.assign(msg.fileTransfer, update)
          return
        }
      }
    }),
  )
}

export function updateLatestFileInChat(
  peerEndpointId: string,
  direction: 'sent' | 'received',
  update: Partial<NonNullable<ChatMessage['fileTransfer']>>,
) {
  setState(
    produce((s) => {
      const msgs = s.chatMessages[peerEndpointId]
      if (!msgs) return
      const msg = [...msgs].reverse().find(
        (m) => m.variant === 'file'
          && m.direction === direction
          && m.fileTransfer
          && !m.fileTransfer.completed
          && !m.fileTransfer.failed,
      )
      if (msg?.fileTransfer) {
        Object.assign(msg.fileTransfer, update)
      }
    }),
  )
}

export function clearChat() {
  setState('chatMessages', {})
}

export function addToast(type: ToastMessage['type'], text: string) {
  const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  setState(
    produce((s) => {
      s.toasts.push({ id, type, text, timestamp: Date.now() })
    }),
  )
  setTimeout(() => dismissToast(id), 4000)
}

export function dismissToast(id: string) {
  setState(
    produce((s) => {
      s.toasts = s.toasts.filter((t) => t.id !== id)
    }),
  )
}

export function toggleTheme() {
  const next = state.theme === 'light' ? 'dark' : 'light'
  setState('theme', next)
  window.localStorage.setItem('altair-vega:theme', next)
  document.documentElement.setAttribute('data-theme', next)
  document.documentElement.style.colorScheme = next
}

export function setSettingsOpen(open: boolean) {
  setState('settingsOpen', open)
}

export function setSidebarOpen(open: boolean) {
  setState('sidebarOpen', open)
}

export function toggleSidebar() {
  setState('sidebarOpen', !state.sidebarOpen)
}
