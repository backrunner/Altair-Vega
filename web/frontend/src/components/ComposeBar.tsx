import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import { FileText, Image as ImageIcon, Paperclip, Send, X } from 'lucide-solid'

import { formatBytes, isImageMime } from '../lib/format'
import { state } from '../lib/state'
import { cx } from '../lib/cx'
import { IconButton } from './ui/Button'

type ComposeBarProps = {
  onSendMessage: (text: string) => void
  onSendFile: (file: File) => void
}

const composeBarClass = [
  'sticky bottom-0 flex flex-col gap-[var(--space-2)] border-t border-t-[var(--color-border)]',
  'bg-[color-mix(in_srgb,var(--color-surface)_88%,transparent)]',
  'p-[var(--space-2)_var(--space-3)_calc(var(--space-3)+env(safe-area-inset-bottom,0px))]',
  'backdrop-blur-[10px] min-[561px]:p-[var(--space-3)]',
].join(' ')
const composeBarDisabledClass = 'opacity-92'
const composeBarShellClass = 'grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[var(--space-2)]'
const composeBarButtonClass = [
  '!h-10 !min-h-10 !min-w-10 !w-10 self-center rounded-[var(--radius-md)] p-0',
  '[&_svg]:!h-[16px] [&_svg]:!w-[16px]',
].join(' ')
const composeBarInputWrapClass = [
  'flex min-h-10 min-w-0 flex-col justify-center gap-[var(--space-2)]',
  'border border-[color-mix(in_srgb,var(--color-border)_72%,var(--color-primary))]',
  'rounded-[var(--radius-module)] bg-[color-mix(in_srgb,var(--color-bg-inset)_66%,var(--color-surface))]',
  'px-[var(--space-3)] py-[7px]',
  'shadow-[inset_0_1px_0_color-mix(in_srgb,var(--color-surface-raised)_48%,transparent)]',
  'focus-within:border-[var(--color-accent)] focus-within:shadow-[0_0_0_3px_var(--color-accent-subtle)]',
].join(' ')
const composeBarAttachmentClass = [
  'grid min-h-10 w-full max-w-[min(100%,420px)] grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[var(--space-2)]',
  'rounded-[var(--radius-md)] border border-[var(--color-border-subtle)]',
  'bg-[color-mix(in_srgb,var(--color-surface-raised)_72%,var(--color-bg-muted))]',
  'p-[4px_6px_4px_4px] text-[length:var(--text-xs)]',
  'shadow-[inset_0_1px_0_color-mix(in_srgb,var(--color-surface)_60%,transparent)]',
].join(' ')
const composeBarAttachmentMediaClass = [
  'inline-flex h-8 w-8 shrink-0 items-center justify-center overflow-hidden rounded-[var(--radius-sm)]',
  'bg-[var(--color-primary-subtle)] text-[var(--color-primary)]',
  '[&_svg]:h-[15px] [&_svg]:w-[15px]',
].join(' ')
const composeBarAttachmentImageFallbackClass = 'bg-[var(--color-secondary-subtle)] text-[var(--color-secondary)]'
const composeBarAttachmentThumbClass = 'h-full w-full object-cover'
const composeBarAttachmentInfoClass = 'flex min-w-0 flex-col justify-center gap-[1px]'
const composeBarChipNameClass = [
  'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap',
  'text-[var(--color-text)] font-650 leading-[var(--leading-tight)]',
].join(' ')
const composeBarChipSizeClass = 'text-[var(--color-text-muted)] leading-[var(--leading-tight)]'
const composeBarChipRemoveClass = [
  'inline-flex h-7 w-7 shrink-0 items-center justify-center border-none rounded-[var(--radius-md)]',
  'bg-transparent p-0 text-[var(--color-text-secondary)] text-[length:1rem] leading-none',
  'transition hover:bg-[var(--color-bg-inset)] hover:text-[var(--color-text)]',
].join(' ')
const composeBarTextareaClass = [
  'm-0 h-auto min-h-5 max-h-24 w-full resize-none border-none bg-transparent p-0',
  'text-[var(--color-text)] text-[length:var(--text-sm)] leading-5',
  'focus:outline-none placeholder:text-[var(--color-text-muted)]',
  'placeholder:text-[length:var(--text-sm)] placeholder:leading-5',
].join(' ')
const composeBarSendClass = composeBarButtonClass

export default function ComposeBar(props: ComposeBarProps) {
  const [text, setText] = createSignal('')
  const [attachedFile, setAttachedFile] = createSignal<File | null>(null)
  const [previewUrl, setPreviewUrl] = createSignal<string | null>(null)
  const [previewFailed, setPreviewFailed] = createSignal(false)

  let textareaRef: HTMLTextAreaElement | undefined
  let fileInputRef: HTMLInputElement | undefined

  const canSend = () => Boolean(state.selectedPeerId) && (text().trim().length > 0 || attachedFile() !== null)
  const attachedIsImage = () => {
    const f = attachedFile()
    return f !== null && isImageMime(f.type)
  }
  const hasAttachmentPreview = () => attachedIsImage() && previewUrl() !== null && !previewFailed()

  const resizeTextarea = () => {
    const textarea = textareaRef
    if (!textarea) return
    textarea.style.height = '0px'
    const lineHeight = Number.parseFloat(window.getComputedStyle(textarea).lineHeight) || 24
    const maxHeight = lineHeight * 4
    textarea.style.height = `${Math.min(textarea.scrollHeight, maxHeight)}px`
    textarea.style.overflowY = textarea.scrollHeight > maxHeight ? 'auto' : 'hidden'
  }

  const revokePreview = () => {
    const url = previewUrl()
    if (url) URL.revokeObjectURL(url)
    setPreviewUrl(null)
    setPreviewFailed(false)
  }

  const attachFile = (file: File) => {
    revokePreview()
    setAttachedFile(file)
    if (isImageMime(file.type)) {
      setPreviewFailed(false)
      setPreviewUrl(URL.createObjectURL(file))
    }
  }

  const clearAttachment = () => {
    revokePreview()
    setAttachedFile(null)
    if (fileInputRef) fileInputRef.value = ''
  }

  const clearComposer = () => {
    setText('')
    clearAttachment()
  }

  onCleanup(revokePreview)

  const handleSubmit = () => {
    if (!canSend()) return
    const trimmed = text().trim()
    const file = attachedFile()
    if (trimmed) props.onSendMessage(trimmed)
    if (file) props.onSendFile(file)
    clearComposer()
  }

  const handleFileChange = (event: Event) => {
    const input = event.currentTarget as HTMLInputElement
    const file = input.files?.[0] ?? null
    if (file) attachFile(file)
  }

  const handlePaste = (event: ClipboardEvent) => {
    const items = event.clipboardData?.items
    if (!items) return
    for (let i = 0; i < items.length; i++) {
      const item = items[i]
      if (item.kind === 'file' && isImageMime(item.type)) {
        event.preventDefault()
        const file = item.getAsFile()
        if (file) {
          const ext = item.type.split('/')[1] || 'png'
          const named = new File([file], `pasted-image.${ext}`, { type: file.type })
          attachFile(named)
        }
        return
      }
    }
  }

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key !== 'Enter' || event.shiftKey) return
    event.preventDefault()
    handleSubmit()
  }

  createEffect(() => {
    text()
    queueMicrotask(resizeTextarea)
  })

  return (
    <div class={cx(composeBarClass, !state.selectedPeerId && composeBarDisabledClass)}>
      <div class={composeBarShellClass}>
        <input
          ref={fileInputRef}
          class="sr-only"
          type="file"
          onChange={handleFileChange}
          tabIndex={-1}
        />

        <IconButton
          type="button"
          class={composeBarButtonClass}
          variant="secondary"
          onClick={() => fileInputRef?.click()}
          disabled={!state.selectedPeerId}
          label="Attach file"
        >
          <Paperclip size={16} />
        </IconButton>

        <div class={composeBarInputWrapClass}>
          <Show when={attachedFile()}>
            {(file) => (
              <div class={composeBarAttachmentClass}>
                <Show
                  when={hasAttachmentPreview()}
                  fallback={
                    <span
                      class={cx(
                        composeBarAttachmentMediaClass,
                        attachedIsImage() && composeBarAttachmentImageFallbackClass,
                      )}
                      aria-hidden="true"
                    >
                      <Show when={attachedIsImage()} fallback={<FileText size={15} />}>
                        <ImageIcon size={15} />
                      </Show>
                    </span>
                  }
                >
                  <span class={composeBarAttachmentMediaClass}>
                    <img
                      src={previewUrl()!}
                      alt=""
                      class={composeBarAttachmentThumbClass}
                      onError={() => setPreviewFailed(true)}
                      onLoad={() => setPreviewFailed(false)}
                    />
                  </span>
                </Show>

                <div class={composeBarAttachmentInfoClass}>
                  <span class={composeBarChipNameClass} title={file().name}>{file().name}</span>
                  <span class={composeBarChipSizeClass}>{formatBytes(file().size)}</span>
                </div>

                <button
                  type="button"
                  class={composeBarChipRemoveClass}
                  onClick={clearAttachment}
                  aria-label={attachedIsImage() ? 'Remove attached image' : 'Remove attached file'}
                >
                  <X size={13} />
                </button>
              </div>
            )}
          </Show>

          <textarea
            ref={textareaRef}
            class={composeBarTextareaClass}
            rows={1}
            value={text()}
            placeholder={state.selectedPeerId ? 'Write a message' : 'Select a peer to start'}
            disabled={!state.selectedPeerId}
            onInput={(event) => setText(event.currentTarget.value)}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
          />
        </div>

        <IconButton
          type="button"
          class={composeBarSendClass}
          onClick={handleSubmit}
          disabled={!canSend()}
          label="Send"
        >
          <Send size={16} />
        </IconButton>
      </div>
    </div>
  )
}
