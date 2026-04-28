import type { ChatMessage } from '../lib/types'
import { formatTime } from '../lib/format'
import { peerName } from '../lib/identity'
import { cx } from '../lib/cx'

import FileCard from './FileCard'

type ChatBubbleProps = {
  message: ChatMessage
}

const bubbleRowClass = 'flex w-full animate-[chat-bubble-enter_var(--duration-normal)_var(--ease-out)]'
const bubbleRowSentClass = 'justify-end'
const bubbleRowReceivedClass = 'justify-start'
const bubbleClass = [
  'max-w-[88%] border rounded-[var(--radius-xl)] px-[var(--space-4)] py-[var(--space-3)]',
  'text-[var(--color-text)] min-[561px]:max-w-[min(75%,620px)]',
].join(' ')
const bubbleSentClass = [
  'rounded-br-[var(--radius-sm)] border-[var(--color-message-sent-border)]',
  'bg-[var(--color-message-sent-bg)] shadow-[0_8px_24px_var(--color-message-sent-shadow)]',
].join(' ')
const bubbleReceivedClass = [
  'rounded-bl-[var(--radius-sm)] border-[var(--color-message-received-border)]',
  'bg-[var(--color-message-received-bg)] shadow-[var(--shadow-sm)]',
].join(' ')
const bubbleMetaClass = [
  'mb-[var(--space-2)] flex items-center justify-between gap-[var(--space-3)]',
  'text-[var(--color-text-secondary)] text-[length:var(--text-xs)]',
].join(' ')
const bubblePeerClass = 'font-650'
const bubbleTimeClass = 'text-[var(--color-text-muted)]'
const bubbleTextClass = 'm-0 whitespace-pre-wrap break-anywhere leading-[var(--leading-normal)]'
const bubbleFileClass = 'min-w-0'

export default function ChatBubble(props: ChatBubbleProps) {
  const isSent = () => props.message.direction === 'sent'

  return (
    <article class={cx(bubbleRowClass, isSent() ? bubbleRowSentClass : bubbleRowReceivedClass)}>
      <div
        class={cx(bubbleClass, isSent() ? bubbleSentClass : bubbleReceivedClass)}
      >
        <header class={bubbleMetaClass}>
          <span class={bubblePeerClass}>{peerName(props.message.peerEndpointId)}</span>
          <span class={bubbleTimeClass}>{formatTime(props.message.timestamp)}</span>
        </header>

        {props.message.variant === 'text' ? (
          <p class={bubbleTextClass}>{props.message.text ?? ''}</p>
        ) : (
          <div class={bubbleFileClass}>
            <FileCard file={props.message.fileTransfer!} direction={props.message.direction} />
          </div>
        )}
      </div>
    </article>
  )
}
