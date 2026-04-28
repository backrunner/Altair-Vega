import { Dialog as DialogPrimitive } from '@kobalte/core/dialog'
import { X } from 'lucide-solid'
import { splitProps, type JSX, type ParentProps } from 'solid-js'

import { cx } from '../../lib/cx'

const dialogOverlayClass = [
  'fixed inset-0 z-[1000] bg-[rgba(0,0,0,0.45)]',
  'backdrop-blur-[5px] animate-[fade-in_var(--duration-normal)_var(--ease-out)]',
  'transition-opacity duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'data-[closed]:pointer-events-none data-[closed]:opacity-0',
].join(' ')

const dialogPositionerClass = [
  'fixed inset-0 z-[1001] grid items-end p-[var(--space-2)] pointer-events-none',
  'min-[561px]:place-items-center min-[561px]:p-[var(--space-4)]',
].join(' ')

const dialogContentClass = [
  'relative flex max-h-[calc(100vh-16px)] w-full flex-col overflow-hidden',
  'border border-[var(--color-border)] rounded-[var(--radius-module)] bg-[var(--color-surface)]',
  'shadow-[var(--shadow-lg)] pointer-events-auto',
  'animate-[dialog-in_var(--duration-normal)_var(--ease-out)]',
  'transition duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'data-[closed]:pointer-events-none data-[closed]:opacity-0 data-[closed]:translate-y-[8px] data-[closed]:scale-[0.98]',
  'min-[561px]:w-[min(680px,100%)] min-[561px]:max-h-[min(760px,calc(100vh-32px))]',
].join(' ')

const dialogHeaderClass = [
  'flex flex-col gap-[var(--space-1)]',
  'border-b border-[var(--color-border-subtle)]',
  'px-[var(--space-4)] py-[var(--space-5)] pb-[var(--space-3)]',
  'min-[561px]:px-[var(--space-6)]',
].join(' ')

const dialogTitleClass = 'text-[var(--color-text)] text-[length:var(--text-xl)] font-680 tracking-normal'

const dialogDescriptionClass = [
  'max-w-[58ch] text-[var(--color-text-muted)]',
  'text-[length:var(--text-sm)] leading-[var(--leading-normal)]',
].join(' ')

const dialogCloseClass = [
  'absolute right-[var(--space-4)] top-[var(--space-4)]',
  'inline-flex h-8 w-8 items-center justify-center',
  'border-none rounded-[var(--radius-md)] bg-transparent text-[var(--color-text-muted)]',
  'hover:bg-[var(--color-bg-muted)] hover:text-[var(--color-text)]',
].join(' ')

const dialogFooterClass = [
  'flex justify-end gap-[var(--space-2)]',
  'border-t border-[var(--color-border-subtle)]',
  'px-[var(--space-4)] py-[var(--space-4)] min-[561px]:px-[var(--space-6)]',
].join(' ')

type DialogRootProps = ParentProps<{
  open: boolean
  onOpenChange: (open: boolean) => void
}>

export function DialogRoot(props: DialogRootProps) {
  return (
    <DialogPrimitive open={props.open} onOpenChange={props.onOpenChange}>
      {props.children}
    </DialogPrimitive>
  )
}

type DialogContentProps = JSX.HTMLAttributes<HTMLDivElement> & ParentProps<{
  onOpenAutoFocus?: (event: Event) => void
}>

export function DialogContent(props: DialogContentProps) {
  const [local, rest] = splitProps(props, ['class', 'children'])

  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay class={dialogOverlayClass} />
      <div class={dialogPositionerClass}>
        <DialogPrimitive.Content class={cx(dialogContentClass, local.class)} {...rest}>
          {local.children}
          <DialogPrimitive.CloseButton
            class={dialogCloseClass}
            aria-label="Close dialog"
          >
            <X size={16} />
          </DialogPrimitive.CloseButton>
        </DialogPrimitive.Content>
      </div>
    </DialogPrimitive.Portal>
  )
}

export function DialogHeader(props: JSX.HTMLAttributes<HTMLDivElement>) {
  const [local, rest] = splitProps(props, ['class', 'children'])

  return (
    <div class={cx(dialogHeaderClass, local.class)} {...rest}>
      {local.children}
    </div>
  )
}

export function DialogTitle(props: ParentProps) {
  return <DialogPrimitive.Title class={dialogTitleClass}>{props.children}</DialogPrimitive.Title>
}

export function DialogDescription(props: ParentProps) {
  return (
    <DialogPrimitive.Description class={dialogDescriptionClass}>
      {props.children}
    </DialogPrimitive.Description>
  )
}

export function DialogFooter(props: JSX.HTMLAttributes<HTMLDivElement>) {
  const [local, rest] = splitProps(props, ['class', 'children'])

  return (
    <div class={cx(dialogFooterClass, local.class)} {...rest}>
      {local.children}
    </div>
  )
}

export { DialogPrimitive }
