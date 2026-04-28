import {
  Content as ContextMenuPrimitiveContent,
  Item as ContextMenuPrimitiveItem,
  Portal as ContextMenuPrimitivePortal,
  Root as ContextMenuPrimitiveRoot,
  Trigger as ContextMenuPrimitiveTrigger,
} from '@kobalte/core/context-menu'
import { splitProps } from 'solid-js'

import { cx } from '../../lib/cx'

const contextMenuContentClass = [
  'z-[1300] min-w-[156px] overflow-hidden outline-none focus:outline-none focus-visible:outline-none',
  'border border-[color-mix(in_srgb,var(--color-border)_72%,transparent)] rounded-[var(--radius-lg)]',
  'bg-[color-mix(in_srgb,var(--color-surface-raised)_94%,var(--color-bg-subtle))]',
  'p-0 text-[var(--color-text)]',
  'shadow-[0_18px_46px_rgba(0,0,0,0.26)] backdrop-blur-[12px]',
  'animate-[fade-in_var(--duration-fast)_var(--ease-out)]',
].join(' ')

const contextMenuItemClass = [
  'flex min-h-8 cursor-default select-none items-center gap-[var(--space-2)]',
  'border-0 px-[10px] py-[8px]',
  'text-[length:0.76rem] font-550 leading-[var(--leading-tight)] outline-none',
  'focus:outline-none focus-visible:outline-none',
  'transition-[background-color] duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'data-[highlighted]:bg-[var(--color-bg-muted)]',
  'data-[disabled]:pointer-events-none data-[disabled]:opacity-45',
  '[&_svg]:h-[14px] [&_svg]:w-[14px] [&_svg]:shrink-0',
].join(' ')

const contextMenuItemDestructiveClass = [
  'text-[var(--color-danger)]',
  'data-[highlighted]:bg-[var(--color-danger-subtle)]',
].join(' ')

export function ContextMenuRoot(props: Parameters<typeof ContextMenuPrimitiveRoot>[0]) {
  return <ContextMenuPrimitiveRoot {...props} />
}

export function ContextMenuTrigger(props: Parameters<typeof ContextMenuPrimitiveTrigger>[0]) {
  const [local, rest] = splitProps(props, ['class', 'children'])

  return (
    <ContextMenuPrimitiveTrigger class={local.class} {...rest}>
      {local.children}
    </ContextMenuPrimitiveTrigger>
  )
}

export function ContextMenuContent(props: Parameters<typeof ContextMenuPrimitiveContent>[0]) {
  const [local, rest] = splitProps(props, ['class', 'children'])

  return (
    <ContextMenuPrimitivePortal>
      <ContextMenuPrimitiveContent class={cx(contextMenuContentClass, local.class)} {...rest}>
        {local.children}
      </ContextMenuPrimitiveContent>
    </ContextMenuPrimitivePortal>
  )
}

type ContextMenuItemProps = Parameters<typeof ContextMenuPrimitiveItem>[0] & {
  destructive?: boolean
}

export function ContextMenuItem(props: ContextMenuItemProps) {
  const [local, rest] = splitProps(props, ['class', 'children', 'destructive', 'disabled', 'onSelect'])

  return (
    <ContextMenuPrimitiveItem
      class={cx(contextMenuItemClass, local.destructive && contextMenuItemDestructiveClass, local.class)}
      disabled={local.disabled}
      onSelect={local.onSelect}
      {...rest}
    >
      {local.children}
    </ContextMenuPrimitiveItem>
  )
}
