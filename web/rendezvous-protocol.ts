export const RENDEZVOUS_PATH = '/__altair_vega_rendezvous'

export const RENDEZVOUS_CLOSE_INVALID_PAYLOAD = 1003
export const RENDEZVOUS_CLOSE_MESSAGE_TOO_LARGE = 1009
export const RENDEZVOUS_CLOSE_ROOM_EXPIRED = 4000

export function describeRendezvousClose(code: number, reason?: string): string | null {
  const detail = reason?.trim()
  switch (code) {
    case RENDEZVOUS_CLOSE_INVALID_PAYLOAD:
      return detail || 'The room service rejected an invalid rendezvous message.'
    case RENDEZVOUS_CLOSE_MESSAGE_TOO_LARGE:
      return detail || 'The room service rejected a rendezvous message that was too large.'
    case RENDEZVOUS_CLOSE_ROOM_EXPIRED:
      return detail || 'The rendezvous room expired. Start a new room with a fresh code.'
    default:
      return null
  }
}

export type RoomPeerInfo = {
  endpointId: string
  connectedAt: number
  peerType?: string
  label?: string
  endpointTicket?: string
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

export type RendezvousServerEvent =
  | { type: 'snapshot'; peers: RoomPeerInfo[] }
  | { type: 'peer-joined'; endpointId: string; connectedAt: number; peerType?: string; label?: string; endpointTicket?: string }
  | { type: 'peer-left'; endpointId: string }
  | { type: 'relay'; fromEndpointId: string; payload: ResumeRelayPayload }

export type RendezvousClientEvent =
  | { type: 'relay'; toEndpointId: string; payload: ResumeRelayPayload }
