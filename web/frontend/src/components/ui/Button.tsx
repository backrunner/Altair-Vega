import { splitProps, type JSX } from 'solid-js'

import { cx } from '../../lib/cx'

type ButtonVariant = 'default' | 'secondary' | 'outline' | 'ghost' | 'destructive'
type ButtonSize = 'default' | 'sm' | 'icon'

type ButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant
  size?: ButtonSize
}

const buttonBase = [
  'inline-flex select-none items-center justify-center gap-[var(--space-2)]',
  'border border-transparent',
  'font-550 text-[length:var(--text-sm)] leading-[var(--leading-tight)] whitespace-nowrap',
  'transition duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'disabled:cursor-not-allowed disabled:opacity-40',
  '[&_svg]:h-4 [&_svg]:w-4 [&_svg]:shrink-0',
].join(' ')

const buttonVariants: Record<ButtonVariant, string> = {
  default: [
    'bg-[var(--color-primary)] text-[var(--color-primary-text)]',
    'not-disabled:hover:bg-[var(--color-primary-hover)]',
  ].join(' '),
  secondary: [
    'border-[color-mix(in_srgb,var(--color-secondary)_18%,transparent)]',
    'bg-[var(--color-secondary-subtle)] text-[var(--color-secondary)]',
    'not-disabled:hover:bg-[color-mix(in_srgb,var(--color-secondary-subtle)_64%,var(--color-bg-muted))] not-disabled:hover:text-[var(--color-secondary-hover)]',
  ].join(' '),
  outline: [
    'border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text)]',
    'not-disabled:hover:bg-[var(--color-bg-subtle)]',
  ].join(' '),
  ghost: [
    'bg-transparent text-[var(--color-text-secondary)]',
    'not-disabled:hover:bg-[var(--color-bg-muted)] not-disabled:hover:text-[var(--color-text)]',
  ].join(' '),
  destructive: [
    'bg-[var(--color-danger)] text-white',
    'not-disabled:hover:bg-[color-mix(in_srgb,var(--color-danger)_86%,#000)]',
  ].join(' '),
}

const buttonSizes: Record<ButtonSize, string> = {
  default: 'min-h-[36px] rounded-[var(--radius-md)] px-[var(--space-4)] py-[var(--space-2)]',
  sm: 'min-h-7 rounded-[var(--radius-md)] px-[var(--space-2)] py-[2px] text-[length:var(--text-xs)]',
  icon: 'h-[36px] min-w-[36px] w-[36px] rounded-[var(--radius-md)] p-0',
}

export function Button(props: ButtonProps) {
  const [local, rest] = splitProps(props, ['class', 'variant', 'size', 'children'])

  return (
    <button
      class={cx(
        buttonBase,
        buttonVariants[local.variant ?? 'default'],
        buttonSizes[local.size ?? 'default'],
        local.class,
      )}
      {...rest}
    >
      {local.children}
    </button>
  )
}

type IconButtonProps = Omit<ButtonProps, 'size'> & {
  label: string
}

export function IconButton(props: IconButtonProps) {
  const [local, rest] = splitProps(props, ['label', 'title', 'children'])

  return (
    <Button
      size="icon"
      aria-label={local.label}
      title={local.title ?? local.label}
      {...rest}
    >
      {local.children}
    </Button>
  )
}
