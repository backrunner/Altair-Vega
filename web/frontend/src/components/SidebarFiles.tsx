import { AlertTriangle, Download, Eraser, Files, Inbox, Trash2 } from 'lucide-solid'
import { For, Show, createSignal } from 'solid-js'

import { formatBytes, formatRelative, joinChunks } from '../lib/format'
import { peerName } from '../lib/identity'
import { addToast, removeReceivedFile, setReceivedFiles, state } from '../lib/state'
import { clearAllStoredFiles, deleteReceivedFile, loadReceivedFileChunks, loadReceivedFileManifest } from '../lib/storage'
import { Badge } from './ui/Badge'
import { Button, IconButton } from './ui/Button'
import { Card } from './ui/Card'
import { ContextMenuContent, ContextMenuItem, ContextMenuRoot, ContextMenuTrigger } from './ui/ContextMenu'
import {
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogRoot,
  DialogTitle,
} from './ui/Dialog'

const sidebarFilesClass = [
  'flex min-h-[94px] min-w-0 select-none flex-col gap-[var(--space-2)] overflow-hidden px-[var(--space-3)] pb-[var(--space-3)] pt-[var(--space-2)]',
  'flex-[1_1_0]',
  'transition-[background-color,border-color,box-shadow] duration-[var(--duration-normal)] ease-[var(--ease-out)]',
].join(' ')
const sidebarFilesHeaderClass = 'flex min-h-7 items-center justify-between gap-[var(--space-2)]'
const sidebarFilesTitleClass = 'inline-flex items-center gap-[var(--space-2)] text-[var(--color-text-secondary)] text-[length:var(--text-sm)] font-600 leading-[var(--leading-tight)]'
const sidebarFilesHeaderActionsClass = 'flex items-center gap-[var(--space-1)]'
const sidebarFilesListClass = [
  'flex min-h-0 flex-1 flex-col gap-[var(--space-1)] overflow-y-auto [overflow-y:overlay]',
  'overscroll-contain [scrollbar-gutter:auto]',
].join(' ')
const sidebarFilesEmptyClass = [
  'flex min-h-0 flex-1 flex-col items-center justify-center gap-[var(--space-2)]',
  'rounded-[var(--radius-md)] px-[var(--space-4)] py-[var(--space-4)] text-center',
].join(' ')
const sidebarFilesEmptyIconClass = [
  'inline-flex h-10 w-10 items-center justify-center rounded-[var(--radius-full)]',
  'border border-[color-mix(in_srgb,var(--color-primary)_16%,transparent)]',
  'bg-[var(--color-primary-subtle)] text-[var(--color-primary)] shadow-[var(--shadow-sm)]',
  '[&_svg]:h-[18px] [&_svg]:w-[18px]',
].join(' ')
const sidebarFilesEmptyTextClass = 'text-[var(--color-text-secondary)] text-[length:var(--text-sm)] font-620'
const sidebarFilesMenuTriggerClass = 'block w-full shrink-0 min-w-0'
const sidebarFilesItemClass = [
  'grid min-h-[42px] min-w-0 shrink-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-[var(--space-2)]',
  'rounded-[var(--radius-md)] bg-[var(--color-bg-subtle)] p-[var(--space-2)]',
].join(' ')
const sidebarFilesMainClass = 'flex min-w-0 flex-col gap-[2px]'
const sidebarFilesNameClass = 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--color-text)] text-[length:var(--text-sm)] font-620'
const sidebarFilesMetaClass = [
  'grid min-w-0 grid-cols-3 items-center gap-[var(--space-2)] overflow-hidden text-[var(--color-text-muted)]',
  'text-[length:var(--text-xs)] whitespace-nowrap',
  '[&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis',
  '[&_span]:text-left [&_span:nth-child(3)]:text-right',
].join(' ')
const sidebarFilesActionsClass = 'flex shrink-0 items-center'
const sidebarFilesClearAllClass = [
  '!h-6 !min-h-6 !min-w-6 !w-6 !rounded-[var(--radius-sm)] !p-0',
  'text-[var(--color-text-muted)]',
  'not-disabled:hover:!bg-transparent not-disabled:hover:text-[var(--color-primary)]',
  'focus-visible:!outline-none focus-visible:!shadow-none',
  '[&_svg]:!h-[13px] [&_svg]:!w-[13px]',
].join(' ')
const sidebarFilesDownloadClass = 'h-8 min-w-8 w-8 rounded-[var(--radius-md)]'
const clearDialogContentClass = 'min-[561px]:!w-[min(420px,100%)]'
const clearDialogBodyClass = [
  'flex items-start gap-[var(--space-3)]',
  'px-[var(--space-4)] py-[var(--space-4)] min-[561px]:px-[var(--space-6)]',
].join(' ')
const clearDialogIconClass = [
  'mt-[2px] inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-[var(--radius-full)]',
  'border border-[color-mix(in_srgb,var(--color-danger)_24%,transparent)]',
  'bg-[var(--color-danger-subtle)] text-[var(--color-danger)]',
  '[&_svg]:h-[18px] [&_svg]:w-[18px]',
].join(' ')
const clearDialogCopyClass = 'min-w-0 text-[var(--color-text-secondary)] text-[length:var(--text-sm)] leading-[var(--leading-normal)]'

export default function SidebarFiles() {
  const [busyKey, setBusyKey] = createSignal<string | null>(null)
  const [clearConfirmOpen, setClearConfirmOpen] = createSignal(false)

  const sortedFiles = () => [...state.receivedFiles].sort((a, b) => b.storedAt - a.storedAt)
  const fileCount = () => state.receivedFiles.length

  const handleDownload = async (storageKey: string) => {
    setBusyKey(`dl:${storageKey}`)
    try {
      const manifest = await loadReceivedFileManifest(storageKey)
      const file = manifest ?? state.receivedFiles.find((e) => e.storageKey === storageKey)
      if (!file || !file.completed) {
        addToast('warning', 'File not ready')
        return
      }
      const chunks = await loadReceivedFileChunks(storageKey)
      const bytes = joinChunks(chunks, file.sizeBytes)
      const blob = new Blob([new Uint8Array(bytes).buffer], { type: file.mimeType || 'application/octet-stream' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = file.name
      a.click()
      URL.revokeObjectURL(url)
    } catch (err) {
      addToast('error', `Download failed: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusyKey(null)
    }
  }

  const handleRemove = async (storageKey: string) => {
    setBusyKey(`rm:${storageKey}`)
    try {
      await deleteReceivedFile(storageKey)
      removeReceivedFile(storageKey)
    } catch (err) {
      addToast('error', `Remove failed: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusyKey(null)
    }
  }

  const handleClearAll = async () => {
    setBusyKey('clear-all')
    try {
      await clearAllStoredFiles()
      setReceivedFiles([])
      addToast('success', 'All stored files cleared')
      setClearConfirmOpen(false)
    } catch (err) {
      addToast('error', `Clear failed: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusyKey(null)
    }
  }

  return (
    <Card class={sidebarFilesClass}>
      <div class={sidebarFilesHeaderClass}>
        <div class={sidebarFilesTitleClass}>
          <Files size={15} />
          Files
        </div>
        <div class={sidebarFilesHeaderActionsClass}>
          <Badge variant="secondary">{fileCount()}</Badge>
          <Show when={fileCount() > 0}>
            <IconButton
              class={sidebarFilesClearAllClass}
              label="Clear all files"
              variant="ghost"
              onClick={() => setClearConfirmOpen(true)}
              disabled={busyKey() !== null}
            >
              <Eraser size={15} />
            </IconButton>
          </Show>
        </div>
      </div>

      <Show
        when={sortedFiles().length > 0}
        fallback={
          <div class={sidebarFilesEmptyClass} role="status" aria-live="polite">
            <span class={sidebarFilesEmptyIconClass} aria-hidden="true">
              <Inbox />
            </span>
            <span class={sidebarFilesEmptyTextClass}>No files yet</span>
          </div>
        }
      >
        <div class={sidebarFilesListClass}>
          <For each={sortedFiles()}>
            {(file) => (
              <ContextMenuRoot>
                <ContextMenuTrigger class={sidebarFilesMenuTriggerClass}>
                  <div class={sidebarFilesItemClass}>
                    <div class={sidebarFilesMainClass}>
                      <div class={sidebarFilesNameClass} title={file.name}>{file.name}</div>
                      <div class={sidebarFilesMetaClass}>
                        <span title={peerName(file.endpointId)}>{peerName(file.endpointId)}</span>
                        <span>{formatBytes(file.sizeBytes)}</span>
                        <span>{formatRelative(file.storedAt)}</span>
                      </div>
                    </div>
                    <div class={sidebarFilesActionsClass}>
                      <IconButton
                        class={sidebarFilesDownloadClass}
                        label={`Download ${file.name}`}
                        variant="ghost"
                        disabled={!file.completed || busyKey() !== null}
                        onClick={() => void handleDownload(file.storageKey)}
                      >
                        <Download size={14} />
                      </IconButton>
                    </div>
                  </div>
                </ContextMenuTrigger>
                <ContextMenuContent>
                  <ContextMenuItem
                    destructive
                    disabled={busyKey() !== null}
                    onSelect={() => void handleRemove(file.storageKey)}
                  >
                    <Trash2 />
                    Remove file
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenuRoot>
            )}
          </For>
        </div>
      </Show>

      <DialogRoot
        open={clearConfirmOpen()}
        onOpenChange={(open) => {
          if (busyKey() !== 'clear-all') setClearConfirmOpen(open)
        }}
      >
        <DialogContent class={clearDialogContentClass}>
          <DialogHeader>
            <DialogTitle>Clear Files</DialogTitle>
            <DialogDescription>
              This removes all received file records and stored chunks from this browser.
            </DialogDescription>
          </DialogHeader>

          <div class={clearDialogBodyClass}>
            <span class={clearDialogIconClass} aria-hidden="true">
              <AlertTriangle />
            </span>
            <p class={clearDialogCopyClass}>
              {fileCount()} {fileCount() === 1 ? 'file' : 'files'} will be cleared. This does not remove peers or chat messages.
            </p>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setClearConfirmOpen(false)}
              disabled={busyKey() === 'clear-all'}
            >
              Cancel
            </Button>
            <Button
              type="button"
              variant="destructive"
              onClick={() => void handleClearAll()}
              disabled={busyKey() === 'clear-all'}
            >
              Clear files
            </Button>
          </DialogFooter>
        </DialogContent>
      </DialogRoot>
    </Card>
  )
}
