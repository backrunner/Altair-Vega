import { For, Show, createEffect, onCleanup, onMount } from 'solid-js'
import { UsersRound } from 'lucide-solid'

import { state } from '../lib/state'

import ChatBubble from './ChatBubble'
import EmptyState from './EmptyState'

const chatThreadClass = 'flex min-h-0 flex-1'
const chatThreadScrollClass = 'min-h-0 flex-1 overflow-y-auto p-[var(--space-3)] min-[561px]:p-[var(--space-4)]'
const chatThreadListClass = 'flex min-h-full flex-col gap-[var(--space-2)] min-[561px]:gap-[var(--space-3)]'

export default function ChatThread() {
  let containerRef: HTMLDivElement | undefined
  let listRef: HTMLDivElement | undefined
  let listResizeObserver: ResizeObserver | undefined
  let scrollFrame: number | undefined
  let resizeFrame: number | undefined
  let stickToBottom = true
  let forceStickUntil = 0

  const messages = () => state.chatMessages[state.selectedPeerId] ?? []
  const hasPeers = () => state.peers.length > 0
  const emptyState = () => (
    <Show
      when={hasPeers()}
      fallback={<EmptyState variant="compact" icon={<UsersRound />} message="No peers" />}
    >
      <EmptyState message="Send a message or drop a file to get started" />
    </Show>
  )

  const isNearBottom = () => {
    if (!containerRef) return true
    return containerRef.scrollHeight - containerRef.scrollTop - containerRef.clientHeight < 48
  }

  const shouldStickToBottom = () => stickToBottom || performance.now() < forceStickUntil

  const scrollToBottom = (behavior: ScrollBehavior = 'auto') => {
    stickToBottom = true
    containerRef?.scrollTo({ top: containerRef.scrollHeight, behavior })
  }

  const handleScroll = () => {
    if (performance.now() < forceStickUntil) return
    stickToBottom = isNearBottom()
  }

  const handleUserScrollIntent = () => {
    forceStickUntil = 0
  }

  onMount(() => {
    scrollToBottom()
  })

  createEffect(() => {
    const messageCount = messages().length
    if (messageCount === 0) return

    forceStickUntil = performance.now() + 900
    stickToBottom = true
    if (scrollFrame) cancelAnimationFrame(scrollFrame)
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = undefined
      scrollToBottom('smooth')
    })
  })

  createEffect(() => {
    const messageCount = messages().length
    listResizeObserver?.disconnect()
    if (messageCount === 0 || !listRef || !containerRef) return

    listResizeObserver = new ResizeObserver(() => {
      if (!shouldStickToBottom()) return
      if (resizeFrame) cancelAnimationFrame(resizeFrame)
      resizeFrame = requestAnimationFrame(() => {
        resizeFrame = undefined
        scrollToBottom()
      })
    })
    listResizeObserver.observe(listRef)
  })

  onCleanup(() => {
    listResizeObserver?.disconnect()
    if (scrollFrame) cancelAnimationFrame(scrollFrame)
    if (resizeFrame) cancelAnimationFrame(resizeFrame)
  })

  return (
    <section class={chatThreadClass}>
      <Show
        when={messages().length > 0}
        fallback={emptyState()}
      >
        <div
          class={chatThreadScrollClass}
          ref={containerRef}
          onScroll={handleScroll}
          onTouchStart={handleUserScrollIntent}
          onWheel={handleUserScrollIntent}
        >
          <div class={chatThreadListClass} ref={listRef}>
            <For each={messages()}>{(message) => <ChatBubble message={message} />}</For>
          </div>
        </div>
      </Show>
    </section>
  )
}
