import { peerColor, peerInitials } from '../lib/identity'
import { cx } from '../lib/cx'

type PeerAvatarProps = {
  endpointId: string
  size?: 'sm' | 'md' | 'lg'
}

const sizeClass = {
  sm: 'h-6 w-6',
  md: 'h-[30px] w-[30px]',
  lg: 'h-[38px] w-[38px]',
} as const

const initialsSizeClass = {
  sm: 'text-[length:0.72rem]',
  md: 'text-[length:0.72rem]',
  lg: 'text-[length:var(--text-sm)]',
} as const

const avatarClass = 'inline-flex shrink-0 items-center justify-center rounded-[var(--radius-full)] text-white'
const initialsClass = 'font-bold leading-none tracking-normal'

export default function PeerAvatar(props: PeerAvatarProps) {
  const size = () => props.size ?? 'md'

  return (
    <div
      class={cx(avatarClass, sizeClass[size()])}
      style={{ 'background-color': peerColor(props.endpointId) }}
      aria-hidden="true"
    >
      <span class={cx(initialsClass, initialsSizeClass[size()])}>{peerInitials(props.endpointId)}</span>
    </div>
  )
}
