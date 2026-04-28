import { Show, type JSX } from 'solid-js'

type EmptyStateProps = {
  icon?: JSX.Element
  message: string
  submessage?: string
  variant?: 'default' | 'compact'
}

const emptyStateClass = 'flex flex-1 flex-col items-center justify-center gap-[var(--space-2)] px-[var(--space-6)] py-[var(--space-8)] text-center'
const emptyStateCompactClass = [
  'flex min-h-0 flex-1 flex-col items-center justify-center gap-[var(--space-2)]',
  'rounded-[var(--radius-md)] px-[var(--space-4)] py-[var(--space-4)] text-center',
].join(' ')
const emptyStateIconClass = [
  'mb-[var(--space-1)] inline-flex h-11 w-11 items-center justify-center rounded-[var(--radius-full)]',
  'border border-[color-mix(in_srgb,var(--color-primary)_16%,transparent)]',
  'bg-[var(--color-primary-subtle)] text-[var(--color-primary)] shadow-[var(--shadow-sm)]',
  '[&_svg]:h-5 [&_svg]:w-5',
].join(' ')
const emptyStateCompactIconClass = [
  'inline-flex h-10 w-10 items-center justify-center rounded-[var(--radius-full)]',
  'border border-[color-mix(in_srgb,var(--color-primary)_16%,transparent)]',
  'bg-[var(--color-primary-subtle)] text-[var(--color-primary)] shadow-[var(--shadow-sm)]',
  '[&_svg]:h-[18px] [&_svg]:w-[18px]',
].join(' ')
const emptyStateMessageClass = 'm-0 text-[var(--color-text-secondary)] text-[length:var(--text-lg)]'
const emptyStateCompactMessageClass = 'm-0 text-[var(--color-text-secondary)] text-[length:var(--text-sm)] font-620'
const emptyStateSubmessageClass = 'm-0 max-w-[36ch] text-[var(--color-text-muted)] text-[length:var(--text-sm)]'

export default function EmptyState(props: EmptyStateProps) {
  const isCompact = () => props.variant === 'compact'

  return (
    <div class={isCompact() ? emptyStateCompactClass : emptyStateClass} role="status" aria-live="polite">
      <Show when={props.icon}>
        <span class={isCompact() ? emptyStateCompactIconClass : emptyStateIconClass} aria-hidden="true">{props.icon}</span>
      </Show>
      <p class={isCompact() ? emptyStateCompactMessageClass : emptyStateMessageClass}>{props.message}</p>
      <Show when={props.submessage}>
        <p class={emptyStateSubmessageClass}>{props.submessage}</p>
      </Show>
    </div>
  )
}
