import { splitProps, type JSX } from 'solid-js'

import { cx } from '../../lib/cx'

type BadgeVariant = 'default' | 'secondary' | 'success' | 'warning' | 'destructive'

type BadgeProps = JSX.HTMLAttributes<HTMLSpanElement> & {
  variant?: BadgeVariant
}

const badgeBase = [
  'inline-flex select-none min-w-0 items-center justify-center gap-[var(--space-1)]',
  'rounded-[var(--radius-full)] px-[var(--space-2)] py-[2px]',
  'text-[length:var(--text-xs)] font-550 leading-[var(--leading-tight)] whitespace-nowrap',
].join(' ')

const badgeVariants: Record<BadgeVariant, string> = {
  default: 'bg-[var(--color-primary-subtle)] text-[var(--color-primary)]',
  secondary: 'border border-[color-mix(in_srgb,var(--color-secondary)_18%,transparent)] bg-[var(--color-secondary-subtle)] text-[var(--color-secondary)]',
  success: 'bg-[var(--color-success-subtle)] text-[var(--color-success)]',
  warning: 'bg-[var(--color-warning-subtle)] text-[var(--color-warning)]',
  destructive: 'bg-[var(--color-danger-subtle)] text-[var(--color-danger)]',
}

export function Badge(props: BadgeProps) {
  const [local, rest] = splitProps(props, ['class', 'variant', 'children'])

  return (
    <span class={cx(badgeBase, badgeVariants[local.variant ?? 'default'], local.class)} {...rest}>
      {local.children}
    </span>
  )
}
