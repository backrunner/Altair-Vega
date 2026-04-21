import { DurableObject } from 'cloudflare:workers'
import {
  RENDEZVOUS_PATH,
  type RendezvousClientEvent,
  type RendezvousServerEvent,
  type RoomPeerInfo,
} from '../../rendezvous-protocol'

export interface Env {
  ROOMS: DurableObjectNamespace<Room>
}

type RoomSession = {
  endpointId: string
  connectedAt: number
  peerType?: string
  label?: string
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)
    if (url.pathname === '/' || url.pathname === '/health') {
      return new Response('ok', {
        status: 200,
        headers: { 'content-type': 'text/plain' },
      })
    }

    if (url.pathname !== RENDEZVOUS_PATH) {
      return new Response('Not found', { status: 404 })
    }

    if (request.headers.get('Upgrade')?.toLowerCase() !== 'websocket') {
      return new Response('Expected Upgrade: websocket', { status: 426 })
    }

    const code = url.searchParams.get('code')?.trim()
    const endpointId = url.searchParams.get('endpointId')?.trim()
    if (!code || !endpointId) {
      return new Response('Missing code or endpointId', { status: 400 })
    }

    const id = env.ROOMS.idFromName(code)
    return env.ROOMS.get(id).fetch(request)
  },
}

export class Room extends DurableObject {
  private readonly sessions = new Map<WebSocket, RoomSession>()

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url)
    const endpointId = url.searchParams.get('endpointId')?.trim()
    const peerType = url.searchParams.get('peerType')?.trim() || undefined
    const label = url.searchParams.get('label')?.trim() || undefined
    if (!endpointId) {
      return new Response('Missing endpointId', { status: 400 })
    }

    const pair = new WebSocketPair()
    const [client, server] = Object.values(pair)
    server.accept()

    this.evictDuplicate(endpointId)

    const session: RoomSession = {
      endpointId,
      connectedAt: Date.now(),
      peerType,
      label,
    }
    const existingPeers: RoomPeerInfo[] = [...this.sessions.values()].map((peer) => ({
      endpointId: peer.endpointId,
      connectedAt: peer.connectedAt,
      peerType: peer.peerType,
      label: peer.label,
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
    })

    server.addEventListener('message', (event) => {
      this.onMessage(server, String(event.data))
    })
    server.addEventListener('close', () => {
      this.onClose(server)
    })

    return new Response(null, {
      status: 101,
      webSocket: client,
    })
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

  private onMessage(sourceSocket: WebSocket, raw: string) {
    let message: RendezvousClientEvent | null = null
    try {
      message = JSON.parse(raw) as RendezvousClientEvent
    } catch {
      return
    }
    if (!message || message.type !== 'relay') {
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
  }
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
