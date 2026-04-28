import { For, createEffect, createSignal, onCleanup } from 'solid-js'
import { AlertTriangle, Check, Info, X, XCircle } from 'lucide-solid'

import { dismissToast, state } from '../lib/state'
import { cx } from '../lib/cx'
import { IconButton } from './ui/Button'

type VisibleToast = {
  id: string
  type: 'info' | 'success' | 'warning' | 'error'
  text: string
  timestamp: number
  closing?: boolean
}

const EXIT_DELAY_MS = 180

const toastStackClass = [
  'fixed bottom-[var(--space-4)] left-1/2 z-[1300]',
  'flex w-[min(420px,calc(100vw-var(--space-6)))] -translate-x-1/2 flex-col gap-[var(--space-3)]',
  'pointer-events-none min-[561px]:left-auto min-[561px]:right-[var(--space-4)]',
  'min-[561px]:w-[min(360px,calc(100vw-var(--space-8)))] min-[561px]:translate-x-0',
].join(' ')
const toastCardClass = [
  'grid min-h-12 grid-cols-[auto_1fr_auto] items-center gap-[var(--space-3)]',
  'border border-[var(--color-border)] rounded-[var(--radius-lg)]',
  'bg-[color-mix(in_srgb,var(--color-surface-raised)_92%,transparent)]',
  'p-[var(--space-3)] shadow-[var(--shadow-lg)] backdrop-blur-[12px]',
  'pointer-events-auto animate-[toast-enter_var(--duration-slow)_var(--ease-out)_both]',
  'transform-gpu transition duration-[var(--duration-fast)] ease-[var(--ease-out)] will-change-transform',
].join(' ')
const toastClosingClass = 'translate-y-[6px] scale-[0.98] opacity-0'
const toastIconBaseClass = 'h-[18px] w-[18px] self-center'
const toastTextClass = 'min-w-0 self-center text-[var(--color-text)] text-[length:var(--text-sm)] leading-[var(--leading-normal)]'
const toastDismissClass = 'h-7 min-w-7 w-7'

function toastIconToneClass(type: VisibleToast['type']) {
  if (type === 'success') return 'text-[var(--color-success)]'
  if (type === 'warning') return 'text-[var(--color-warning)]'
  if (type === 'error') return 'text-[var(--color-danger)]'
  return 'text-[var(--color-accent)]'
}

function ToastIcon(props: { type: VisibleToast['type'] }) {
  if (props.type === 'success') return <Check size={18} aria-hidden="true" />
  if (props.type === 'warning') return <AlertTriangle size={18} aria-hidden="true" />
  if (props.type === 'error') return <XCircle size={18} aria-hidden="true" />
  return <Info size={18} aria-hidden="true" />
}

export default function Toast() {
  const [visibleToastIds, setVisibleToastIds] = createSignal<string[]>([])
  const [visibleToasts, setVisibleToasts] = createSignal<Record<string, VisibleToast>>({})
  const exitTimers = new Map<string, number>()

  createEffect(() => {
    const next = [...state.toasts].slice(-3).reverse()
    const nextIds = new Set(next.map((toast) => toast.id))

    setVisibleToasts((current) => {
      const updated = { ...current }

      for (const toast of next) {
        updated[toast.id] = current[toast.id]?.closing
          ? { ...toast, closing: false }
          : (current[toast.id] ?? { ...toast, closing: false })

        const timer = exitTimers.get(toast.id)
        if (timer) {
          window.clearTimeout(timer)
          exitTimers.delete(toast.id)
        }
      }

      for (const [id, toast] of Object.entries(current)) {
        if (nextIds.has(id)) continue
        if (!toast.closing) updated[id] = { ...toast, closing: true }
        if (exitTimers.has(id)) continue
        const timer = window.setTimeout(() => {
          setVisibleToastIds((ids) => ids.filter((toastId) => toastId !== id))
          setVisibleToasts((items) => {
            const remaining = { ...items }
            delete remaining[id]
            return remaining
          })
          exitTimers.delete(id)
        }, EXIT_DELAY_MS)
        exitTimers.set(id, timer)
      }

      return updated
    })

    setVisibleToastIds((current) => {
      const closingIds = current.filter((id) => !nextIds.has(id))
      return [...next.map((toast) => toast.id), ...closingIds]
    })
  })

  onCleanup(() => {
    for (const timer of exitTimers.values()) {
      window.clearTimeout(timer)
    }
  })

  return (
    <div class={toastStackClass} aria-live="polite" aria-atomic="false">
      <For each={visibleToastIds()}>
        {(toastId) => {
          const toast = () => visibleToasts()[toastId]

          return (
            <div class={cx(toastCardClass, toast()?.closing && toastClosingClass)} role="status">
              <div class={cx(toastIconBaseClass, toastIconToneClass(toast()?.type ?? 'info'))} aria-hidden="true">
                <ToastIcon type={toast()?.type ?? 'info'} />
              </div>
              <div class={toastTextClass}>{toast()?.text}</div>
              <IconButton class={toastDismissClass} variant="ghost" label="Dismiss notification" onClick={() => dismissToast(toastId)}>
                <X size={14} />
              </IconButton>
            </div>
          )
        }}
      </For>
    </div>
  )
}
