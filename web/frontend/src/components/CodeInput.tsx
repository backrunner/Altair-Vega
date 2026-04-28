import { For, Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js'
import { normalize_short_code } from 'altair-vega-browser'
import { Check, Copy, RefreshCcw } from 'lucide-solid'

import { addToast } from '../lib/state'
import { cx } from '../lib/cx'
import { Button, IconButton } from './ui/Button'
import { Card } from './ui/Card'

type CodeInputProps = {
  code: string
  onCodeChange: (code: string) => void
  onGenerate: () => void
}

type SegmentIndex = 0 | 1 | 2 | 3

const EMPTY_SEGMENTS = ['', '', '', '']
const SEGMENT_PLACEHOLDERS = ['0000', 'word', 'word', 'word']
const SEGMENT_INDEXES: SegmentIndex[] = [0, 1, 2, 3]

const codeInputClass = 'flex shrink-0 select-none flex-col gap-[var(--space-2)] px-[var(--space-3)] pb-[var(--space-3)] pt-[var(--space-2)]'
const codeInputHeaderClass = 'flex min-h-7 items-center justify-between gap-[var(--space-2)]'
const codeInputLabelClass = 'text-[var(--color-text-secondary)] text-[length:var(--text-sm)] font-600 leading-[var(--leading-tight)]'
const codeInputNewClass = [
  '!min-h-[24px] rounded-[var(--radius-full)] px-[7px] py-[2px]',
  'text-[length:0.72rem] leading-[var(--leading-tight)] [&_svg]:!h-[12px] [&_svg]:!w-[12px]',
].join(' ')
const codeInputRowClass = [
  'grid grid-cols-[minmax(0,1fr)_32px] items-center gap-[var(--space-1)]',
  'border border-[var(--color-border)] rounded-[var(--radius-lg)]',
  'bg-[color-mix(in_srgb,var(--color-bg-muted)_72%,var(--color-bg))]',
  'p-[var(--space-1)] shadow-[inset_0_1px_0_color-mix(in_srgb,var(--color-surface)_58%,transparent)]',
].join(' ')
const codeInputGroupClass = [
  'min-w-0',
].join(' ')
const codeInputDisplayClass = [
  'min-h-[32px] min-w-0 cursor-text select-text overflow-x-auto',
  'whitespace-nowrap text-[length:0.72rem] leading-[32px] [font-family:var(--font-mono)]',
  '[scrollbar-width:none] [&::-webkit-scrollbar]:hidden',
].join(' ')
const codeInputSegmentClass = [
  'inline-flex h-[32px] min-w-0 select-text items-center justify-center align-middle',
  'border border-[var(--color-border-subtle)] rounded-[calc(var(--radius-lg)-4px)]',
  'bg-[color-mix(in_srgb,var(--color-surface-raised)_86%,var(--color-surface))]',
  'px-[5px] font-650 text-[var(--color-text)] shadow-[var(--shadow-sm)]',
  'transition duration-[var(--duration-fast)] ease-[var(--ease-out)]',
].join(' ')
const codeInputPlaceholderSegmentClass = 'text-[var(--color-text-muted)] opacity-72'
const codeInputSeparatorClass = [
  'inline-flex shrink-0 select-text items-center justify-center px-[2px] align-middle text-[var(--color-text-muted)] [font-family:var(--font-mono)]',
  'text-[length:0.7rem] font-700',
].join(' ')
const codeInputCopyClass = [
  'aspect-square !h-[32px] !min-h-[32px] !min-w-[32px] !w-[32px] rounded-[calc(var(--radius-lg)-4px)]',
  '!border-[var(--color-border-subtle)] !bg-[var(--color-surface-raised)]',
  'not-disabled:hover:!bg-[var(--color-surface)]',
  '[&_svg]:!h-[13px] [&_svg]:!w-[13px]',
].join(' ')
const codeInputCopyWrapClass = 'relative min-w-[32px]'
const codeInputCopySuccessClass = [
  '!border-[color-mix(in_srgb,var(--color-success)_26%,transparent)]',
  '!bg-[var(--color-success-subtle)] !text-[var(--color-success)]',
].join(' ')
const codeInputCopiedTipClass = [
  'pointer-events-none absolute bottom-[calc(100%+6px)] right-0 z-[2]',
  'rounded-[var(--radius-sm)] bg-[var(--color-success)] px-[var(--space-2)] py-[3px]',
  'text-white text-[length:0.68rem] font-650 leading-[var(--leading-tight)] shadow-[var(--shadow-sm)]',
  'animate-[fade-in_var(--duration-fast)_var(--ease-out)]',
].join(' ')

function sanitizeSlot(value: string) {
  return value.replace(/\D/g, '').slice(0, 4)
}

function sanitizeWord(value: string) {
  return value.replace(/[^a-z]/gi, '').toLowerCase().slice(0, 5)
}

function parseSegments(code: string) {
  if (!code.trim()) return [...EMPTY_SEGMENTS]

  try {
    return normalize_short_code(code).split('-').slice(0, 4)
  } catch {
    const parts = code.split('-', 4)
    return [
      sanitizeSlot(parts[0] ?? ''),
      sanitizeWord(parts[1] ?? ''),
      sanitizeWord(parts[2] ?? ''),
      sanitizeWord(parts[3] ?? ''),
    ]
  }
}

function joinPartial(segments: string[]) {
  let lastFilledIndex = -1
  for (let index = segments.length - 1; index >= 0; index -= 1) {
    if (segments[index].length > 0) {
      lastFilledIndex = index
      break
    }
  }
  if (lastFilledIndex < 0) return ''
  return segments.slice(0, lastFilledIndex + 1).join('-')
}

function joinFull(segments: string[]) {
  return segments.join('-')
}

function getNormalizedCode(segments: string[]) {
  if (!segments[0] || !segments[1] || !segments[2] || !segments[3]) return null

  try {
    return normalize_short_code(joinFull(segments))
  } catch {
    return null
  }
}

function copyWithCommand(text: string) {
  let copied = false
  const handleCopyEvent = (event: ClipboardEvent) => {
    event.clipboardData?.setData('text/plain', text)
    event.preventDefault()
    copied = true
  }

  document.addEventListener('copy', handleCopyEvent)
  try {
    return document.execCommand('copy') && copied
  } finally {
    document.removeEventListener('copy', handleCopyEvent)
  }
}

function copyWithTextarea(text: string) {
  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.style.position = 'fixed'
  textarea.style.left = '0'
  textarea.style.top = '0'
  textarea.style.width = '1px'
  textarea.style.height = '1px'
  textarea.style.opacity = '0'
  textarea.style.pointerEvents = 'none'
  document.body.appendChild(textarea)
  textarea.focus({ preventScroll: true })
  textarea.select()
  textarea.setSelectionRange(0, text.length)

  try {
    return document.execCommand('copy')
  } finally {
    textarea.remove()
  }
}

async function copyText(text: string) {
  if (copyWithCommand(text)) return
  if (copyWithTextarea(text)) return

  if (navigator.clipboard?.writeText) {
    try {
      await Promise.race([
        navigator.clipboard.writeText(text),
        new Promise<never>((_, reject) => {
          window.setTimeout(() => reject(new Error('Clipboard timed out')), 700)
        }),
      ])
      return
    } catch {
      // Report a consistent failure below for restricted browser contexts.
    }
  }

  throw new Error('Clipboard unavailable')
}

export default function CodeInput(props: CodeInputProps) {
  const [segments, setSegments] = createSignal([...EMPTY_SEGMENTS])
  const [copied, setCopied] = createSignal(false)

  let syncingFromProps = false
  let copiedTimer = 0

  const syncFromCode = (code: string) => {
    syncingFromProps = true
    setSegments(parseSegments(code))
    queueMicrotask(() => {
      syncingFromProps = false
    })
  }

  const displayCode = () => getNormalizedCode(segments()) ?? joinPartial(segments())

  const handleCopy = async () => {
    const normalized = getNormalizedCode(segments())
    if (!normalized) return
    try {
      await copyText(normalized)
      setCopied(true)
      if (copiedTimer) window.clearTimeout(copiedTimer)
      copiedTimer = window.setTimeout(() => {
        setCopied(false)
        copiedTimer = 0
      }, 2400)
    } catch (err) {
      addToast('error', `Copy failed: ${err instanceof Error ? err.message : String(err)}`)
    }
  }

  onMount(() => {
    syncFromCode(props.code)
  })

  createEffect(() => {
    syncFromCode(props.code)
  })

  createEffect(() => {
    const currentSegments = segments()
    if (syncingFromProps) return

    const partialCode = joinPartial(currentSegments)
    props.onCodeChange(partialCode)
  })

  onCleanup(() => {
    if (copiedTimer) window.clearTimeout(copiedTimer)
  })

  return (
    <Card class={codeInputClass}>
      <div class={codeInputHeaderClass}>
        <label class={codeInputLabelClass}>
          Connection Code
        </label>
        <Button type="button" class={codeInputNewClass} variant="secondary" size="sm" onClick={props.onGenerate}>
          <RefreshCcw size={14} />
          New
        </Button>
      </div>

      <div class={codeInputRowClass}>
        <div class={codeInputGroupClass} role="group" aria-label="Connection Code">
          <div class={codeInputDisplayClass} title={displayCode()}>
            <For each={SEGMENT_INDEXES}>
              {(index) => (
                <>
                  <span class={cx(codeInputSegmentClass, !segments()[index] && codeInputPlaceholderSegmentClass)}>
                    {segments()[index] || SEGMENT_PLACEHOLDERS[index]}
                  </span>
                  <Show when={index < 3}>
                    <span class={codeInputSeparatorClass} aria-hidden="true">-</span>
                  </Show>
                </>
              )}
            </For>
          </div>
        </div>

        <div class={codeInputCopyWrapClass}>
          <IconButton
            class={cx(codeInputCopyClass, copied() && codeInputCopySuccessClass)}
            variant="ghost"
            label={copied() ? 'Connection Code copied' : 'Copy Connection Code'}
            onClick={handleCopy}
            disabled={!getNormalizedCode(segments())}
          >
            {copied() ? <Check size={14} /> : <Copy size={14} />}
          </IconButton>
          <Show when={copied()}>
            <span class={codeInputCopiedTipClass} role="status" aria-live="polite">Copied</span>
          </Show>
        </div>
      </div>

    </Card>
  )
}
