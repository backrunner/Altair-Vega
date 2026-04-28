import { onMount, onCleanup } from 'solid-js'
import { X } from 'lucide-solid'

import { IconButton } from './ui/Button'

type ImageViewerProps = {
  src: string
  alt: string
  onClose: () => void
}

const imageViewerClass = [
  'fixed inset-0 z-[1200] flex cursor-zoom-out items-center justify-center',
  'bg-[rgba(0,0,0,0.85)] backdrop-blur-sm animate-[fade-in_var(--duration-normal)_var(--ease-out)]',
].join(' ')
const imageViewerCloseClass = [
  'absolute right-[var(--space-4)] top-[var(--space-4)] z-[1]',
  'text-[rgba(255,255,255,0.8)] hover:bg-[rgba(255,255,255,0.12)] hover:text-white',
].join(' ')
const imageViewerImageClass = [
  'max-h-[90vh] max-w-[90vw] cursor-default rounded-[var(--radius-md)] object-contain',
  'shadow-[0_8px_40px_rgba(0,0,0,0.5)] animate-[dialog-in_var(--duration-normal)_var(--ease-out)]',
].join(' ')

export default function ImageViewer(props: ImageViewerProps) {
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') props.onClose()
  }

  onMount(() => {
    document.addEventListener('keydown', handleKeyDown)
  })

  onCleanup(() => {
    document.removeEventListener('keydown', handleKeyDown)
  })

  return (
    <div class={imageViewerClass} role="dialog" aria-label="Image viewer" onClick={props.onClose}>
      <IconButton class={imageViewerCloseClass} variant="ghost" label="Close viewer" onClick={props.onClose}>
        <X size={22} />
      </IconButton>
      <img
        class={imageViewerImageClass}
        src={props.src}
        alt={props.alt}
        onClick={(e) => e.stopPropagation()}
      />
    </div>
  )
}
