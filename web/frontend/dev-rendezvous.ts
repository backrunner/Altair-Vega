import type { ViteDevServer } from 'vite'
import { WebSocketServer, type WebSocket } from 'ws'
import {
  RENDEZVOUS_PATH,
  type RendezvousClientEvent,
  type RendezvousServerEvent,
  type RoomPeerInfo,
} from '../rendezvous-protocol'

type RoomPeer = {
  endpointId: string
  connectedAt: number
  peerType?: string
  label?: string
  endpointTicket?: string
  socket: WebSocket
}

export function createDevRendezvousPlugin() {
  const rooms = new Map<string, Map<string, RoomPeer>>()

  return {
    name: 'altair-vega-dev-rendezvous',
    apply: 'serve' as const,
    configureServer(server: ViteDevServer) {
      const wss = new WebSocketServer({ noServer: true })

      server.httpServer?.on('upgrade', (request, socket, head) => {
        const url = request.url ? new URL(request.url, 'http://127.0.0.1') : null
        if (!url || url.pathname !== RENDEZVOUS_PATH) {
          return
        }

        const code = url.searchParams.get('code')?.trim()
        const endpointId = url.searchParams.get('endpointId')?.trim()
        const peerType = url.searchParams.get('peerType')?.trim() || undefined
        const label = url.searchParams.get('label')?.trim() || undefined
        const endpointTicket = url.searchParams.get('endpointTicket')?.trim() || undefined
        if (!code || !endpointId) {
          socket.destroy()
          return
        }

        wss.handleUpgrade(request, socket, head, (ws) => {
          attachPeer(rooms, ws, code, endpointId, peerType, label, endpointTicket)
        })
      })
    },
  }
}

function attachPeer(
  rooms: Map<string, Map<string, RoomPeer>>,
  socket: WebSocket,
  code: string,
  endpointId: string,
  peerType?: string,
  label?: string,
  endpointTicket?: string,
) {
  const room = rooms.get(code) ?? new Map<string, RoomPeer>()
  rooms.set(code, room)

  const existingPeers: RoomPeerInfo[] = [...room.values()].map((peer) => ({
    endpointId: peer.endpointId,
    connectedAt: peer.connectedAt,
    peerType: peer.peerType,
    label: peer.label,
    endpointTicket: peer.endpointTicket,
  }))

  const duplicate = room.get(endpointId)
  if (duplicate) {
    duplicate.socket.close(1000, 'replaced by newer peer session')
    room.delete(endpointId)
  }

  const peer: RoomPeer = {
    endpointId,
    connectedAt: Date.now(),
    peerType,
    label,
    endpointTicket,
    socket,
  }
  room.set(endpointId, peer)

  send(socket, {
    type: 'snapshot',
    peers: existingPeers,
  })
  broadcast(room, endpointId, {
    type: 'peer-joined',
    endpointId,
    connectedAt: peer.connectedAt,
    peerType,
    label,
    endpointTicket,
  })

  socket.on('close', () => {
    const currentRoom = rooms.get(code)
    if (!currentRoom) {
      return
    }
    currentRoom.delete(endpointId)
    broadcast(currentRoom, endpointId, {
      type: 'peer-left',
      endpointId,
    })
    if (currentRoom.size === 0) {
      rooms.delete(code)
    }
  })

  socket.on('message', (raw) => {
    let message: RendezvousClientEvent | null = null
    try {
      message = JSON.parse(String(raw)) as RendezvousClientEvent
    } catch {
      return
    }
    if (!message || message.type !== 'relay') {
      return
    }
    const target = room.get(message.toEndpointId)
    if (!target) {
      return
    }
    send(target.socket, {
      type: 'relay',
      fromEndpointId: endpointId,
      payload: message.payload,
    })
  })
}

function broadcast(
  room: Map<string, RoomPeer>,
  sourceEndpointId: string,
  event: RendezvousServerEvent,
) {
  for (const peer of room.values()) {
    if (peer.endpointId === sourceEndpointId) {
      continue
    }
    send(peer.socket, event)
  }
}

function send(socket: WebSocket, event: RendezvousServerEvent) {
  if (socket.readyState !== socket.OPEN) {
    return
  }
  socket.send(JSON.stringify(event))
}
