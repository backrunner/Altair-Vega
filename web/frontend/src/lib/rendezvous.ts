import { describeRendezvousClose, RENDEZVOUS_PATH } from '../../../rendezvous-protocol'
import type { ResumeRelayPayload, RendezvousEvent, RoomConnection, PresenceMessage } from './types'

const DEFAULT_RENDEZVOUS_URL = import.meta.env.VITE_DEFAULT_RENDEZVOUS_URL?.trim() ?? ''
const DEFAULT_RENDEZVOUS_PATH = import.meta.env.VITE_RENDEZVOUS_PATH?.trim() || RENDEZVOUS_PATH
const MAX_RECONNECT_ATTEMPTS = 5
const RECONNECT_BASE_DELAY_MS = 500

export function getRendezvousUrl(): URL {
  const RENDEZVOUS_URL_STORAGE_KEY = 'altair-vega:rendezvous-url'

  const queryValue = new URLSearchParams(window.location.search).get('rendezvous')?.trim()
  if (queryValue) {
    window.localStorage.setItem(RENDEZVOUS_URL_STORAGE_KEY, queryValue)
    return new URL(queryValue)
  }

  const storedValue = window.localStorage.getItem(RENDEZVOUS_URL_STORAGE_KEY)?.trim()
  if (storedValue) {
    return new URL(storedValue)
  }

  if (DEFAULT_RENDEZVOUS_URL) {
    return new URL(DEFAULT_RENDEZVOUS_URL)
  }

  const path = DEFAULT_RENDEZVOUS_PATH.startsWith('/')
    ? DEFAULT_RENDEZVOUS_PATH
    : DEFAULT_RENDEZVOUS_PATH.replace(/^\.\//, '')
  const base = DEFAULT_RENDEZVOUS_PATH.startsWith('/') ? window.location.origin : window.location.href
  const url = new URL(path, base)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  return url
}

export type RoomCallbacks = {
  onOpen: (code: string) => void
  onEvent: (event: RendezvousEvent) => void
  onClose: (code: string, reason?: string) => void
  onError: (code: string) => void
  onReconnecting: (code: string, attempt: number, delayMs: number) => void
  onFallbackPresence: (endpointId: string, announcedAt: number, endpointTicket?: string) => void
  onFallbackRelayUnavailable: () => void
}

export function connectRoom(
  code: string,
  endpointId: string,
  endpointTicket: string | undefined,
  callbacks: RoomCallbacks,
): RoomConnection {
  let closedIntentionally = false
  let reconnectAttempts = 0
  let reconnectTimer = 0
  let socket: WebSocket | null = null
  let fallbackConnection: RoomConnection | null = null

  const openSocket = () => {
    const wsUrl = getRendezvousUrl()
    wsUrl.searchParams.set('code', code)
    wsUrl.searchParams.set('endpointId', endpointId)
    wsUrl.searchParams.set('peerType', 'browser-web')
    wsUrl.searchParams.set('label', 'Browser Web')
    if (endpointTicket) {
      wsUrl.searchParams.set('endpointTicket', endpointTicket)
    }

    const nextSocket = new WebSocket(wsUrl)
    socket = nextSocket
    let disconnected = false

    const handleDisconnect = (event?: CloseEvent) => {
      if (closedIntentionally || disconnected) return
      disconnected = true
      if (socket === nextSocket) socket = null

      const closeReason = event ? describeRendezvousClose(event.code, event.reason) : null
      if (closeReason) {
        callbacks.onClose(code, closeReason)
        fallbackConnection = fallbackToBroadcastChannel(code, endpointId, endpointTicket, callbacks)
        return
      }

      if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
        reconnectAttempts += 1
        const delayMs = RECONNECT_BASE_DELAY_MS * 2 ** (reconnectAttempts - 1)
        callbacks.onReconnecting(code, reconnectAttempts, delayMs)
        reconnectTimer = window.setTimeout(openSocket, delayMs)
        return
      }

      callbacks.onClose(code)
      fallbackConnection = fallbackToBroadcastChannel(code, endpointId, endpointTicket, callbacks)
    }

    nextSocket.addEventListener('open', () => {
      reconnectAttempts = 0
      callbacks.onOpen(code)
    })

    nextSocket.addEventListener('message', (event) => {
      const message = JSON.parse(String(event.data)) as RendezvousEvent
      callbacks.onEvent(message)
    })

    nextSocket.addEventListener('close', handleDisconnect)

    nextSocket.addEventListener('error', () => {
      callbacks.onError(code)
      handleDisconnect()
      nextSocket.close()
    })
  }

  openSocket()

  return {
    close() {
      closedIntentionally = true
      if (reconnectTimer) window.clearTimeout(reconnectTimer)
      fallbackConnection?.close()
      socket?.close()
    },
    sendRelay(toEndpointId, payload) {
      if (fallbackConnection) {
        fallbackConnection.sendRelay(toEndpointId, payload)
        return
      }
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        callbacks.onFallbackRelayUnavailable()
        return
      }
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

function fallbackToBroadcastChannel(
  code: string,
  endpointId: string,
  endpointTicket: string | undefined,
  callbacks: RoomCallbacks,
): RoomConnection {
  const channel = new BroadcastChannel(`altair-vega-dev::${code}`)

  channel.onmessage = (event: MessageEvent<PresenceMessage>) => {
    if (event.data.type !== 'presence' || event.data.endpointId === endpointId) return
    callbacks.onFallbackPresence(event.data.endpointId, event.data.announcedAt, event.data.endpointTicket)
  }

  broadcastPresence(channel, endpointId, endpointTicket, true)
  const timer = window.setInterval(() => broadcastPresence(channel, endpointId, endpointTicket, false), 5000)

  return {
    close() {
      window.clearInterval(timer)
      channel.close()
    },
    sendRelay() {
      callbacks.onFallbackRelayUnavailable()
    },
  }
}

function broadcastPresence(
  channel: BroadcastChannel,
  endpointId: string,
  endpointTicket: string | undefined,
  requestReply: boolean,
) {
  channel.postMessage({
    type: 'presence',
    endpointId,
    announcedAt: Date.now(),
    requestReply,
    endpointTicket,
  } satisfies PresenceMessage)
}
