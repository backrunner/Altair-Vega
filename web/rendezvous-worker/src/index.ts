import { DurableObject } from 'cloudflare:workers'
import {
  RENDEZVOUS_CLOSE_INVALID_PAYLOAD,
  RENDEZVOUS_CLOSE_MESSAGE_TOO_LARGE,
  RENDEZVOUS_CLOSE_ROOM_EXPIRED,
  RENDEZVOUS_PATH,
  type RendezvousClientEvent,
  type RendezvousServerEvent,
  type RoomPeerInfo,
} from '../../rendezvous-protocol'

export interface Env {
  ROOMS: DurableObjectNamespace<Room>
  ALLOWED_ORIGINS?: string
  ROOM_TTL_SECONDS?: string
  MAX_ROOM_PEERS?: string
  MAX_MESSAGE_BYTES?: string
}

type RoomSession = {
  endpointId: string
  connectedAt: number
  peerType?: string
  label?: string
  endpointTicket?: string
  limits?: RoomLimits
}

type RoomLimits = {
  maxRoomAgeMs: number
  maxPeers: number
  maxMessageBytes: number
}

const DEFAULT_ROOM_TTL_SECONDS = 30 * 60
const DEFAULT_MAX_ROOM_PEERS = 8
const DEFAULT_MAX_MESSAGE_BYTES = 256 * 1024
const MAX_CODE_BYTES = 128
const MAX_ENDPOINT_ID_BYTES = 128
const MAX_ENDPOINT_TICKET_BYTES = 4096
const MAX_PEER_TYPE_BYTES = 64
const MAX_LABEL_BYTES = 96

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)
    if (url.pathname === '/' || url.pathname === '/health') {
      return new Response('ok', {
        status: 200,
        headers: { 'content-type': 'text/plain' },
      })
    }

    if (!isRendezvousPath(url.pathname)) {
      return new Response('Not found', { status: 404 })
    }

    if (!isTrustedOrigin(request, env.ALLOWED_ORIGINS)) {
      return new Response('Origin not allowed', { status: 403 })
    }

    if (request.headers.get('Upgrade')?.toLowerCase() !== 'websocket') {
      return new Response('Expected Upgrade: websocket', { status: 426 })
    }

    const code = url.searchParams.get('code')?.trim()
    const endpointId = url.searchParams.get('endpointId')?.trim()
    if (!code || !endpointId) {
      return new Response('Missing code or endpointId', { status: 400 })
    }
    if (!isSafeToken(code, MAX_CODE_BYTES) || !isSafeToken(endpointId, MAX_ENDPOINT_ID_BYTES)) {
      return new Response('Invalid code or endpointId', { status: 400 })
    }

    const id = env.ROOMS.idFromName(code)
    const nextUrl = new URL(request.url)
    nextUrl.searchParams.set('roomTtlSeconds', env.ROOM_TTL_SECONDS ?? '')
    nextUrl.searchParams.set('maxRoomPeers', env.MAX_ROOM_PEERS ?? '')
    nextUrl.searchParams.set('maxMessageBytes', env.MAX_MESSAGE_BYTES ?? '')
    return env.ROOMS.get(id).fetch(new Request(nextUrl, request))
  },
}

function isRendezvousPath(pathname: string) {
  return pathname === RENDEZVOUS_PATH || pathname.endsWith(`/${RENDEZVOUS_PATH.replace(/^\//, '')}`)
}

export class Room extends DurableObject {
  private readonly sessions = new Map<WebSocket, RoomSession>()
  private createdAt = Date.now()

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env)
    for (const socket of this.ctx.getWebSockets()) {
      const session = socket.deserializeAttachment()
      if (isRoomSession(session)) {
        this.sessions.set(socket, session)
      }
    }
    const connectedAtValues = [...this.sessions.values()].map((session) => session.connectedAt)
    if (connectedAtValues.length > 0) {
      this.createdAt = Math.min(...connectedAtValues)
    }
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url)
    const endpointId = url.searchParams.get('endpointId')?.trim()
    const peerType = boundedOptionalParam(url, 'peerType', MAX_PEER_TYPE_BYTES)
    const label = boundedOptionalParam(url, 'label', MAX_LABEL_BYTES)
    const endpointTicket = boundedOptionalParam(url, 'endpointTicket', MAX_ENDPOINT_TICKET_BYTES)
    const limits = roomLimitsFromUrl(url)
    if (!endpointId) {
      return new Response('Missing endpointId', { status: 400 })
    }
    if (!isSafeToken(endpointId, MAX_ENDPOINT_ID_BYTES)) {
      return new Response('Invalid endpointId', { status: 400 })
    }
    if (peerType === null || label === null || endpointTicket === null) {
      return new Response('Invalid peer metadata', { status: 400 })
    }

    this.cleanup(limits, Date.now())
    if (this.isExpired(limits, Date.now())) {
      this.closeAll(RENDEZVOUS_CLOSE_ROOM_EXPIRED, 'room expired')
      return new Response('Room expired', { status: 410 })
    }

    this.evictDuplicate(endpointId)
    if (this.sessions.size >= limits.maxPeers) {
      return new Response('Room capacity reached', { status: 409 })
    }

    const pair = new WebSocketPair()
    const [client, server] = Object.values(pair)

    const session: RoomSession = {
      endpointId,
      connectedAt: Date.now(),
      peerType,
      label,
      endpointTicket,
      limits,
    }
    this.ctx.acceptWebSocket(server)
    server.serializeAttachment(session)
    const existingPeers: RoomPeerInfo[] = [...this.sessions.values()].map((peer) => ({
      endpointId: peer.endpointId,
      connectedAt: peer.connectedAt,
      peerType: peer.peerType,
      label: peer.label,
      endpointTicket: peer.endpointTicket,
    }))

    this.sessions.set(server, session)
    send(server, {
      type: 'snapshot',
      peers: existingPeers,
    })
    broadcast(this.sessions, endpointId, {
      type: 'peer-joined',
      endpointId,
      connectedAt: session.connectedAt,
      peerType,
      label,
      endpointTicket,
    })

    return new Response(null, {
      status: 101,
      webSocket: client,
    })
  }

  webSocketMessage(socket: WebSocket, data: string | ArrayBuffer) {
    this.onMessage(socket, data, this.sessions.get(socket)?.limits ?? DEFAULT_ROOM_LIMITS)
  }

  webSocketClose(socket: WebSocket) {
    this.onClose(socket)
  }

  webSocketError(socket: WebSocket) {
    this.onClose(socket)
  }

  private evictDuplicate(endpointId: string) {
    for (const [socket, session] of this.sessions.entries()) {
      if (session.endpointId !== endpointId) {
        continue
      }
      socket.close(1000, 'replaced by newer peer session')
      this.sessions.delete(socket)
    }
  }

  private onMessage(sourceSocket: WebSocket, data: string | ArrayBuffer, limits: RoomLimits) {
    this.cleanup(limits, Date.now())
    if (this.isExpired(limits, Date.now())) {
      this.closeAll(RENDEZVOUS_CLOSE_ROOM_EXPIRED, 'room expired')
      return
    }

    if (typeof data !== 'string') {
      sourceSocket.close(RENDEZVOUS_CLOSE_INVALID_PAYLOAD, 'expected text message')
      this.onClose(sourceSocket)
      return
    }
    if (utf8Bytes(data) > limits.maxMessageBytes) {
      sourceSocket.close(RENDEZVOUS_CLOSE_MESSAGE_TOO_LARGE, 'message too large')
      this.onClose(sourceSocket)
      return
    }

    let message: RendezvousClientEvent | null = null
    try {
      message = JSON.parse(data) as RendezvousClientEvent
    } catch {
      sourceSocket.close(RENDEZVOUS_CLOSE_INVALID_PAYLOAD, 'invalid JSON')
      this.onClose(sourceSocket)
      return
    }
    if (!isValidRelayMessage(message, limits.maxMessageBytes)) {
      sourceSocket.close(RENDEZVOUS_CLOSE_INVALID_PAYLOAD, 'invalid relay message')
      this.onClose(sourceSocket)
      return
    }

    const source = this.sessions.get(sourceSocket)
    if (!source) {
      return
    }

    for (const [socket, session] of this.sessions.entries()) {
      if (session.endpointId !== message.toEndpointId) {
        continue
      }
      send(socket, {
        type: 'relay',
        fromEndpointId: source.endpointId,
        payload: message.payload,
      })
      return
    }
  }

  private onClose(socket: WebSocket) {
    const session = this.sessions.get(socket)
    if (!session) {
      return
    }
    this.sessions.delete(socket)
    broadcast(this.sessions, session.endpointId, {
      type: 'peer-left',
      endpointId: session.endpointId,
    })
    if (this.sessions.size === 0) {
      this.createdAt = Date.now()
    }
  }

  private cleanup(limits: RoomLimits, now: number) {
    for (const socket of this.sessions.keys()) {
      if (socket.readyState === socket.CLOSED || socket.readyState === socket.CLOSING) {
        this.onClose(socket)
      }
    }

    if (this.sessions.size === 0) {
      this.createdAt = now
      return
    }

    if (this.isExpired(limits, now)) {
      this.closeAll(RENDEZVOUS_CLOSE_ROOM_EXPIRED, 'room expired')
    }
  }

  private isExpired(limits: RoomLimits, now: number) {
    return now - this.createdAt > limits.maxRoomAgeMs
  }

  private closeAll(code: number, reason: string) {
    for (const socket of this.sessions.keys()) {
      socket.close(code, reason)
    }
    this.sessions.clear()
    this.createdAt = Date.now()
  }
}

const DEFAULT_ROOM_LIMITS: RoomLimits = {
  maxRoomAgeMs: DEFAULT_ROOM_TTL_SECONDS * 1000,
  maxPeers: DEFAULT_MAX_ROOM_PEERS,
  maxMessageBytes: DEFAULT_MAX_MESSAGE_BYTES,
}

function isRoomSession(value: unknown): value is RoomSession {
  if (!value || typeof value !== 'object') {
    return false
  }
  const session = value as Partial<RoomSession>
  return (
    typeof session.endpointId === 'string' &&
    typeof session.connectedAt === 'number' &&
    (session.peerType === undefined || typeof session.peerType === 'string') &&
    (session.label === undefined || typeof session.label === 'string') &&
    (session.endpointTicket === undefined || typeof session.endpointTicket === 'string') &&
    (session.limits === undefined || isRoomLimits(session.limits))
  )
}

function isRoomLimits(value: unknown): value is RoomLimits {
  if (!value || typeof value !== 'object') {
    return false
  }
  const limits = value as Partial<RoomLimits>
  return (
    typeof limits.maxRoomAgeMs === 'number' &&
    typeof limits.maxPeers === 'number' &&
    typeof limits.maxMessageBytes === 'number'
  )
}

function isTrustedOrigin(request: Request, allowedOriginsValue: string | undefined) {
  const allowedOrigins = parseAllowedOrigins(allowedOriginsValue)
  if (allowedOrigins.length === 0) {
    return true
  }

  const origin = request.headers.get('Origin')?.trim()
  if (!origin) {
    return false
  }
  return allowedOrigins.includes(origin)
}

function parseAllowedOrigins(value: string | undefined) {
  return (value ?? '')
    .split(',')
    .map((origin) => origin.trim())
    .filter(Boolean)
}

function roomLimitsFromUrl(url: URL): RoomLimits {
  return {
    maxRoomAgeMs: positiveIntParam(url, 'roomTtlSeconds', DEFAULT_ROOM_TTL_SECONDS) * 1000,
    maxPeers: positiveIntParam(url, 'maxRoomPeers', DEFAULT_MAX_ROOM_PEERS),
    maxMessageBytes: positiveIntParam(url, 'maxMessageBytes', DEFAULT_MAX_MESSAGE_BYTES),
  }
}

function positiveIntParam(url: URL, name: string, fallback: number) {
  const value = Number.parseInt(url.searchParams.get(name) ?? '', 10)
  return Number.isFinite(value) && value > 0 ? value : fallback
}

function boundedOptionalParam(url: URL, name: string, maxBytes: number): string | undefined | null {
  const value = url.searchParams.get(name)?.trim()
  if (!value) {
    return undefined
  }
  return isSafeToken(value, maxBytes) ? value : null
}

function isSafeToken(value: string, maxBytes: number) {
  return utf8Bytes(value) <= maxBytes && !/[\u0000-\u001f\u007f]/u.test(value)
}

function isValidRelayMessage(
  message: RendezvousClientEvent | null,
  maxMessageBytes: number,
): message is RendezvousClientEvent {
  if (!message || typeof message !== 'object') {
    return false
  }
  if (message.type !== 'relay' || !isSafeToken(message.toEndpointId, MAX_ENDPOINT_ID_BYTES)) {
    return false
  }
  if (!message.payload || typeof message.payload !== 'object') {
    return false
  }
  return utf8Bytes(JSON.stringify(message.payload)) <= maxMessageBytes
}

function utf8Bytes(value: string) {
  return new TextEncoder().encode(value).byteLength
}

function broadcast(
  sessions: Map<WebSocket, RoomSession>,
  sourceEndpointId: string,
  event: RendezvousServerEvent,
) {
  for (const [socket, session] of sessions.entries()) {
    if (session.endpointId === sourceEndpointId) {
      continue
    }
    send(socket, event)
  }
}

function send(socket: WebSocket, event: RendezvousServerEvent) {
  if (socket.readyState !== socket.OPEN) {
    return
  }
  socket.send(JSON.stringify(event))
}
