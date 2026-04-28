import { splitProps, type JSX } from 'solid-js'

import { cx } from '../../lib/cx'

type InputProps = JSX.InputHTMLAttributes<HTMLInputElement>

const inputClass = [
  'w-full min-h-[36px] border border-[var(--color-border)] rounded-[var(--radius-md)]',
  'bg-[var(--color-surface)] px-[var(--space-3)] py-[var(--space-2)]',
  'text-[var(--color-text)] text-[length:var(--text-base)] leading-[var(--leading-tight)]',
  'transition duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'placeholder:text-[var(--color-text-muted)]',
  'focus:outline-none focus:border-[var(--color-accent)] focus:shadow-[0_0_0_3px_var(--color-accent-subtle)]',
].join(' ')

export function Input(props: InputProps) {
  const [local, rest] = splitProps(props, ['class'])

  return <input class={cx(inputClass, local.class)} {...rest} />
}
