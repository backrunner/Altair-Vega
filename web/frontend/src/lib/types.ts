/* Shared TypeScript types for Altair Vega web frontend */

export type BrowserEvent =
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
  | {
      type: 'sentFileChunk'
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
  | { type: 'error'; message: string }

export type RawBrowserEvent = {
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

export type PresenceMessage = {
  type: 'presence'
  endpointId: string
  announcedAt: number
  requestReply: boolean
  endpointTicket?: string
}

export type RendezvousEvent =
  | { type: 'snapshot'; peers: Array<{ endpointId: string; connectedAt: number; peerType?: string; label?: string; endpointTicket?: string }> }
  | { type: 'peer-joined'; endpointId: string; connectedAt: number; peerType?: string; label?: string; endpointTicket?: string }
  | { type: 'peer-left'; endpointId: string }
  | { type: 'relay'; fromEndpointId: string; payload: ResumeRelayPayload }

export type RoomConnection = {
  close: () => void
  sendRelay: (toEndpointId: string, payload: ResumeRelayPayload) => void
}

export type ResumeQueryPayload = {
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

export type ResumeReplyPayload = {
  type: 'resume-reply'
  requestId: string
  storageKey: string
  localBytes: number
  missingRanges: Array<{ start: number; end: number }>
}

export type ResumeRelayPayload = ResumeQueryPayload | ResumeReplyPayload

export type Peer = {
  endpointId: string
  lastSeenAt: number
  peerType?: string
  label?: string
  endpointTicket?: string
}

export type ReceivedFile = {
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

export type PersistedReceivedFile = ReceivedFile & {
  storedAt: number
}

export type PersistedReceivedFileChunk = {
  chunkKey: string
  storageKey: string
  chunkIndex: number
  chunkBytes: number
  bytes: ArrayBuffer
}

export type ChatMessage = {
  id: string
  direction: 'sent' | 'received'
  variant: 'text' | 'file'
  peerEndpointId: string
  timestamp: number
  text?: string
  fileTransfer?: {
    name: string
    sizeBytes: number
    bytesComplete: number
    hashHex: string
    mimeType: string
    completed: boolean
    failed?: boolean
    error?: string
    storageKey: string
    blobUrl?: string
  }
}

export type ConnectionState =
  | 'starting'
  | 'ready'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'fallback'
  | 'disconnected'
  | 'error'

export type ToastMessage = {
  id: string
  type: 'info' | 'success' | 'warning' | 'error'
  text: string
  timestamp: number
}
