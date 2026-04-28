import { splitProps, type JSX } from 'solid-js'

import { cx } from '../../lib/cx'

type CardProps = JSX.HTMLAttributes<HTMLDivElement>

const cardClass = [
  'border border-[var(--color-border)] rounded-[var(--radius-module)]',
  'bg-[color-mix(in_srgb,var(--color-surface)_96%,var(--color-bg-subtle))]',
  'shadow-[var(--shadow-sm)]',
].join(' ')

export function Card(props: CardProps) {
  const [local, rest] = splitProps(props, ['class', 'children'])

  return (
    <div class={cx(cardClass, local.class)} {...rest}>
      {local.children}
    </div>
  )
}
