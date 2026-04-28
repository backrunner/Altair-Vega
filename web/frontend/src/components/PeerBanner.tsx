import { Check, Users, UsersRound } from 'lucide-solid'
import { For, Show } from 'solid-js'

import { peerName } from '../lib/identity'
import { selectPeer, state } from '../lib/state'
import { cx } from '../lib/cx'
import { Badge } from './ui/Badge'
import { Card } from './ui/Card'
import EmptyState from './EmptyState'
import PeerAvatar from './PeerAvatar'

const peerBannerClass = [
  'flex min-h-[94px] min-w-0 select-none flex-col gap-[var(--space-2)] overflow-hidden px-[var(--space-3)] pb-[var(--space-3)] pt-[var(--space-2)]',
  'flex-[1_1_0]',
  'transition-[background-color,border-color,box-shadow] duration-[var(--duration-normal)] ease-[var(--ease-out)]',
].join(' ')
const peerHeaderClass = 'flex min-h-7 items-center justify-between gap-[var(--space-2)]'
const peerTitleClass = 'inline-flex items-center gap-[var(--space-2)] text-[var(--color-text-secondary)] text-[length:var(--text-sm)] font-600 leading-[var(--leading-tight)]'
const peerListClass = [
  'flex min-h-0 flex-1 flex-col gap-[var(--space-1)] overflow-y-auto [overflow-y:overlay]',
  'overscroll-contain [scrollbar-gutter:auto]',
].join(' ')
const peerListItemClass = [
  'grid min-h-[38px] w-full min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[var(--space-2)]',
  'border border-transparent rounded-[var(--radius-md)] bg-transparent px-[var(--space-2)] py-[5px]',
  'text-left text-inherit transition duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'hover:bg-[var(--color-bg-subtle)]',
].join(' ')
const peerListItemSelectedClass = [
  '!border-[color-mix(in_srgb,var(--color-accent)_42%,var(--color-border))]',
  '!bg-[color-mix(in_srgb,var(--color-accent)_14%,var(--color-surface))]',
  '!shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--color-accent)_18%,transparent)]',
  'hover:!bg-[color-mix(in_srgb,var(--color-accent)_18%,var(--color-surface))]',
].join(' ')
const peerListInfoClass = 'flex min-w-0 items-center gap-[var(--space-2)]'
const peerListNameClass = 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--color-text)] text-[length:var(--text-sm)] font-620'
const peerListEndpointClass = 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--color-text-muted)] font-[var(--font-mono)] text-[length:0.68rem]'

function peerTypeLabel(peerType?: string, label?: string) {
  if (label) return label
  if (peerType === 'browser-web') return 'Browser'
  if (peerType === 'native-cli') return 'Native CLI'
  if (!peerType) return 'Peer'

  return peerType
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(' ')
}

function shortEndpoint(endpointId: string) {
  if (endpointId.length <= 16) return endpointId
  return `${endpointId.slice(0, 8)}...${endpointId.slice(-6)}`
}

export default function PeerBanner() {
  return (
    <Card class={peerBannerClass}>
      <div class={peerHeaderClass}>
        <div class={peerTitleClass}>
          <Users size={15} />
          Peers
        </div>
        <Badge variant="secondary">{state.peers.length}</Badge>
      </div>

      <Show
        when={state.peers.length > 0}
        fallback={<EmptyState variant="compact" icon={<UsersRound />} message="No peers" />}
      >
        <div class={peerListClass} role="listbox" aria-label="Select a peer">
          <For each={state.peers}>
            {(peer) => {
              const selected = () => peer.endpointId === state.selectedPeerId
              return (
                <button
                  type="button"
                  class={cx(peerListItemClass, selected() && peerListItemSelectedClass)}
                  role="option"
                  aria-selected={selected()}
                  onClick={() => selectPeer(peer.endpointId)}
                  title={peer.endpointId}
                >
                  <PeerAvatar endpointId={peer.endpointId} size="sm" />
                  <span class={peerListInfoClass}>
                    <span class={peerListNameClass}>{peerName(peer.endpointId)}</span>
                    <span class={peerListEndpointClass}>{shortEndpoint(peer.endpointId)}</span>
                  </span>
                  <Show
                    when={selected()}
                    fallback={<Badge variant="secondary">{peerTypeLabel(peer.peerType, peer.label)}</Badge>}
                  >
                    <Check size={16} color="var(--color-accent)" />
                  </Show>
                </button>
              )
            }}
          </For>
        </div>
      </Show>
    </Card>
  )
}
