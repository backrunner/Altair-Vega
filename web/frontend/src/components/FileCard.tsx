import { Show, createSignal, createEffect, onCleanup } from 'solid-js'

import { formatBytes, formatPercent, joinChunks, isImageMime, isSafeToOpen } from '../lib/format'
import { loadReceivedFileChunks, loadReceivedFileManifest } from '../lib/storage'
import { addToast } from '../lib/state'
import type { ChatMessage } from '../lib/types'
import { cx } from '../lib/cx'
import ImageViewer from './ImageViewer'
import { Badge } from './ui/Badge'
import { Button } from './ui/Button'

type FileCardProps = {
  direction: 'sent' | 'received'
  fileTransfer?: NonNullable<ChatMessage['fileTransfer']>
  file?: NonNullable<ChatMessage['fileTransfer']>
}

const fileCardClass = [
  'flex min-w-[200px] max-w-[320px] items-start gap-[var(--space-3)] overflow-hidden',
  'border rounded-[var(--radius-lg)] p-[var(--space-3)]',
].join(' ')
const fileCardSentClass = 'border-[var(--color-message-file-sent-border)] bg-[var(--color-message-file-sent-bg)]'
const fileCardReceivedClass = 'border-[var(--color-message-file-received-border)] bg-[var(--color-message-file-received-bg)]'
const fileCardFailedClass = [
  'border-[color-mix(in_srgb,var(--color-danger)_38%,var(--color-border))]',
  'bg-[color-mix(in_srgb,var(--color-danger-subtle)_72%,var(--color-surface))]',
].join(' ')
const fileCardImageClass = 'flex-col gap-[var(--space-2)]'
const fileCardClickableClass = [
  'cursor-pointer transition hover:border-[var(--color-accent)]',
  'hover:shadow-[0_0_0_2px_var(--color-accent-subtle)]',
  'focus-visible:outline-none focus-visible:border-[var(--color-accent)]',
  'focus-visible:shadow-[0_0_0_3px_var(--color-accent-subtle)]',
].join(' ')
const fileCardImagePreviewClass = 'max-h-[200px] w-full rounded-[var(--radius-md)] object-cover'
const fileCardIconClass = [
  'inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-[var(--radius-md)]',
  '[&_svg]:h-5 [&_svg]:w-5',
].join(' ')
const fileCardIconSentClass = 'bg-[var(--color-primary-subtle)] text-[var(--color-primary)]'
const fileCardIconReceivedClass = 'bg-[var(--color-bg-muted)] text-[var(--color-text-secondary)]'
const fileCardBodyClass = 'flex min-w-0 flex-1 flex-col gap-[var(--space-2)] overflow-hidden'
const fileCardBodyImageClass = 'w-0 min-w-full'
const fileCardHeaderClass = 'flex min-w-0 flex-col gap-[2px] overflow-hidden'
const fileCardNameClass = 'overflow-hidden text-ellipsis whitespace-nowrap text-[var(--color-text)] text-[length:var(--text-sm)] font-650'
const fileCardSizeClass = 'text-[var(--color-text-secondary)] text-[length:var(--text-xs)]'
const fileCardStatusClass = 'inline-flex items-center gap-[var(--space-1)] text-[var(--color-text-secondary)] text-[length:var(--text-sm)]'
const fileCardStatusSentClass = 'text-[var(--color-success)]'
const fileCardCheckClass = 'font-bold'
const fileCardProgressWrapClass = 'flex flex-col gap-[var(--space-2)]'
const progressBarClass = 'h-1 overflow-hidden rounded-[var(--radius-full)] bg-[var(--color-bg-muted)]'
const progressBarFillClass = 'h-full rounded-[var(--radius-full)] bg-[var(--color-accent)] transition-all duration-[var(--duration-normal)] ease-[var(--ease-out)]'
const progressBarFillFailedClass = 'bg-[var(--color-danger)]'
const fileCardProgressMetaClass = 'flex items-center justify-between gap-[var(--space-2)] text-[var(--color-text-secondary)] text-[length:var(--text-xs)]'
const fileCardFailureClass = 'flex min-w-0 flex-col items-start gap-[var(--space-1)]'
const fileCardErrorClass = [
  'max-h-10 overflow-hidden break-words',
  'text-[var(--color-danger)] text-[length:var(--text-xs)] leading-[var(--leading-tight)]',
].join(' ')
const fileCardActionsClass = 'flex min-w-0 items-center gap-[var(--space-2)]'
const fileCardHashClass = 'ml-auto shrink-0 text-[var(--color-text-muted)] text-[length:var(--text-xs)] font-[var(--font-mono)]'

export default function FileCard(props: FileCardProps) {
  const [downloading, setDownloading] = createSignal(false)
  const [imageUrl, setImageUrl] = createSignal<string | null>(null)
  const [imagePreviewFailed, setImagePreviewFailed] = createSignal(false)
  const [viewerOpen, setViewerOpen] = createSignal(false)
  const ft = () => props.fileTransfer ?? props.file!

  const isReceived = () => props.direction === 'received'
  const isCompleted = () => ft().completed
  const isFailed = () => ft().failed === true
  const isImage = () => isImageMime(ft().mimeType)
  const progressPercent = () => formatPercent(ft().bytesComplete, ft().sizeBytes)
  const hashPrefix = () => ft().hashHex.slice(0, 8)
  const errorMessage = () => ft().error?.trim()
  const hasImagePreview = () => isImage() && imageUrl() !== null && !imagePreviewFailed()

  // Build image preview URL (sent via blobUrl, received from IndexedDB)
  createEffect(async () => {
    if (!isImage()) {
      setImageUrl(null)
      setImagePreviewFailed(false)
      return
    }
    const blob = ft().blobUrl
    if (blob) {
      setImagePreviewFailed(false)
      setImageUrl(blob)
      return
    }
    if (isReceived() && isCompleted()) {
      try {
        const url = await buildBlobUrl()
        if (url) {
          setImagePreviewFailed(false)
          setImageUrl(url)
        }
      } catch { /* fall back to generic icon */ }
    }
  })

  onCleanup(() => {
    const url = imageUrl()
    // Only revoke URLs we created from IndexedDB, not blobUrls from sender
    if (url && !ft().blobUrl) URL.revokeObjectURL(url)
  })

  /** Build a blob URL from IndexedDB for received files */
  const buildBlobUrl = async (): Promise<string | null> => {
    const manifest = await loadReceivedFileManifest(ft().storageKey)
    if (!manifest) return null
    const chunks = await loadReceivedFileChunks(ft().storageKey)
    const bytes = joinChunks(chunks, manifest.sizeBytes)
    const blob = new Blob([new Uint8Array(bytes).buffer], {
      type: manifest.mimeType || ft().mimeType,
    })
    return URL.createObjectURL(blob)
  }

  /** Resolve a usable blob URL for the file */
  const resolveUrl = async (): Promise<string | null> => {
    // Sent files always have a blobUrl
    if (ft().blobUrl) return ft().blobUrl!
    // Received files: build from IndexedDB
    if (isReceived()) return buildBlobUrl()
    return null
  }

  /** Open: images → lightbox, safe types → new tab, others → download */
  const handleOpen = async () => {
    if (!isCompleted() || isFailed()) return
    const mime = ft().mimeType

    if (isImage() && !imagePreviewFailed()) {
      if (!imageUrl()) {
        const url = await resolveUrl()
        if (url) {
          setImagePreviewFailed(false)
          setImageUrl(url)
        }
      }
      if (imageUrl()) {
        setViewerOpen(true)
      } else {
        addToast('warning', 'Image not available for preview')
      }
      return
    }

    const url = await resolveUrl()
    if (!url) {
      addToast('warning', 'File not available')
      return
    }

    if (isImage() && imagePreviewFailed()) {
      triggerDownload(url, ft().name)
    } else if (isSafeToOpen(mime)) {
      window.open(url, '_blank', 'noopener,noreferrer')
    } else {
      triggerDownload(url, ft().name)
    }
  }

  const handleDownload = async () => {
    if (!isCompleted() || isFailed() || downloading()) return
    setDownloading(true)
    try {
      const url = await resolveUrl()
      if (!url) return
      triggerDownload(url, ft().name)
    } finally {
      setDownloading(false)
    }
  }

  const clickable = () => isCompleted() && !isFailed()

  return (
    <>
      <article
        class={cx(
          fileCardClass,
          props.direction === 'sent' ? fileCardSentClass : fileCardReceivedClass,
          isFailed() && fileCardFailedClass,
          hasImagePreview() && fileCardImageClass,
          clickable() && fileCardClickableClass,
        )}
        onClick={() => { if (clickable()) void handleOpen() }}
        role={clickable() ? 'button' : undefined}
        tabIndex={clickable() ? 0 : undefined}
        onKeyDown={(e: KeyboardEvent) => {
          if (clickable() && (e.key === 'Enter' || e.key === ' ')) {
            e.preventDefault()
            void handleOpen()
          }
        }}
      >
        <Show
          when={hasImagePreview()}
          fallback={
            <div
              class={cx(fileCardIconClass, props.direction === 'sent' ? fileCardIconSentClass : fileCardIconReceivedClass)}
              aria-hidden="true"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M7 3.75h7l4.25 4.25v12.25a1.5 1.5 0 0 1-1.5 1.5h-9.5a1.5 1.5 0 0 1-1.5-1.5v-15a1.5 1.5 0 0 1 1.5-1.5Z" />
                <path d="M14 3.75V8h4.25" />
                <path d="M8.5 12.25h7" />
                <path d="M8.5 15.75h7" />
              </svg>
            </div>
          }
        >
          <img
            src={imageUrl()!}
            alt={ft().name}
            class={fileCardImagePreviewClass}
            loading="lazy"
            onError={() => {
              setImagePreviewFailed(true)
              setViewerOpen(false)
            }}
            onLoad={() => setImagePreviewFailed(false)}
          />
        </Show>

        <div class={cx(fileCardBodyClass, hasImagePreview() && fileCardBodyImageClass)}>
          <div class={fileCardHeaderClass}>
            <div class={fileCardNameClass} title={ft().name}>{ft().name}</div>
            <div class={fileCardSizeClass}>{formatBytes(ft().sizeBytes)}</div>
          </div>

          <Show when={props.direction === 'sent' && isCompleted() && !isFailed()}>
            <div class={cx(fileCardStatusClass, fileCardStatusSentClass)}>
              <span class={fileCardCheckClass} aria-hidden="true">✓</span>
              <span>Sent</span>
            </div>
          </Show>

          <Show when={!isCompleted()}>
            <div class={fileCardProgressWrapClass}>
              <div class={progressBarClass} aria-hidden="true">
                <div
                  class={cx(progressBarFillClass, isFailed() && progressBarFillFailedClass)}
                  style={{ width: progressPercent() }}
                />
              </div>
              <div class={fileCardProgressMetaClass}>
                <span>{formatBytes(ft().bytesComplete)} of {formatBytes(ft().sizeBytes)}</span>
                <span>{progressPercent()}</span>
              </div>
              <Show
                when={isFailed()}
                fallback={<Badge variant="warning">{isReceived() ? 'Incomplete' : 'Sending'}</Badge>}
              >
                <div class={fileCardFailureClass}>
                  <Badge variant="destructive">Failed</Badge>
                  <Show when={errorMessage()}>
                    {(message) => <span class={fileCardErrorClass} title={message()}>{message()}</span>}
                  </Show>
                </div>
              </Show>
            </div>
          </Show>

          <Show when={isReceived() && isCompleted()}>
            <div class={fileCardActionsClass}>
              <Button
                type="button"
                variant="secondary"
                onClick={(e) => { e.stopPropagation(); void handleDownload() }}
                disabled={downloading()}
              >
                {downloading() ? 'Preparing...' : 'Download'}
              </Button>
              <span class={fileCardHashClass} title={ft().hashHex}>#{hashPrefix()}</span>
            </div>
          </Show>
        </div>
      </article>

      <Show when={viewerOpen() && imageUrl() && !imagePreviewFailed()}>
        <ImageViewer
          src={imageUrl()!}
          alt={ft().name}
          onClose={() => setViewerOpen(false)}
        />
      </Show>
    </>
  )
}

function triggerDownload(url: string, filename: string) {
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.click()
}
