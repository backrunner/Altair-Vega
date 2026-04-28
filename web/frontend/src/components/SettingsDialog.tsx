import { For, Show, createSignal } from 'solid-js'
import { Activity, Check, Copy, Cpu, Fingerprint, RadioTower, Trash2 } from 'lucide-solid'

import { addToast, setSettingsOpen, state } from '../lib/state'
import { cx } from '../lib/cx'
import { Badge } from './ui/Badge'
import { Button, IconButton } from './ui/Button'
import { DialogContent, DialogDescription, DialogHeader, DialogRoot, DialogTitle } from './ui/Dialog'
import { Input } from './ui/Input'

const RENDEZVOUS_URL_STORAGE_KEY = 'altair-vega:rendezvous-url'
const RENDEZVOUS_HISTORY_KEY = 'altair-vega:rendezvous-history'
const DEFAULT_RENDEZVOUS_URL = import.meta.env.VITE_DEFAULT_RENDEZVOUS_URL ?? ''

type RendezvousOption = 'same-origin' | 'webrtc-local' | 'custom'

const settingsBodyClass = [
  'flex select-none min-h-0 flex-col gap-[var(--space-4)]',
  'overflow-y-auto px-[var(--space-4)] py-[var(--space-5)] min-[561px]:px-[var(--space-6)]',
].join(' ')

const settingsSectionClass = [
  'flex flex-col gap-[var(--space-3)] rounded-[var(--radius-module)]',
  'border border-[var(--color-border)] bg-[color-mix(in_srgb,var(--color-surface-raised)_72%,var(--color-bg-subtle))]',
  'p-[var(--space-3)] shadow-[var(--shadow-sm)]',
].join(' ')
const settingsSectionHeaderClass = 'flex min-w-0 flex-col gap-[2px]'
const settingsSectionTitleClass = 'text-[var(--color-text)] text-[length:var(--text-sm)] font-680'
const settingsSectionDescriptionClass = 'text-[var(--color-text-muted)] text-[length:var(--text-xs)] leading-[var(--leading-normal)]'
const serviceOptionsClass = [
  'grid grid-cols-1 gap-[var(--space-2)] rounded-[var(--radius-module)]',
  'bg-[color-mix(in_srgb,var(--color-bg-subtle)_70%,transparent)] p-[var(--space-1)]',
].join(' ')

const serviceOptionClass = [
  'group relative grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[var(--space-3)]',
  'rounded-[calc(var(--radius-module)-6px)] border border-[var(--color-border-subtle)] bg-[var(--color-surface)]',
  'px-[var(--space-3)] py-[10px] text-left shadow-none outline-none',
  'transition-[background-color,border-color,box-shadow,transform,opacity] duration-[var(--duration-fast)] ease-[var(--ease-out)]',
  'not-disabled:hover:-translate-y-px not-disabled:hover:border-[color-mix(in_srgb,var(--color-primary)_38%,var(--color-border))]',
  'not-disabled:hover:bg-[color-mix(in_srgb,var(--color-surface-raised)_72%,var(--color-bg-subtle))]',
  'disabled:cursor-not-allowed disabled:opacity-[0.55]',
  'focus-visible:outline-none focus-visible:border-[var(--color-primary)] focus-visible:shadow-[0_0_0_3px_var(--color-primary-subtle)]',
].join(' ')

const serviceOptionSelectedClass = [
  'border-[color-mix(in_srgb,var(--color-primary)_68%,var(--color-border))]',
  'bg-[color-mix(in_srgb,var(--color-primary-subtle)_54%,var(--color-surface))]',
  'shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--color-primary)_28%,transparent)]',
].join(' ')

const serviceRadioClass = [
  'inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-[var(--radius-full)]',
  'border border-[var(--color-border)] bg-[var(--color-surface-raised)]',
  'transition duration-[var(--duration-fast)] ease-[var(--ease-out)]',
].join(' ')
const serviceRadioSelectedClass = [
  'border-[var(--color-primary)] bg-[var(--color-surface)]',
  'shadow-[0_0_0_3px_var(--color-primary-subtle)]',
].join(' ')
const serviceRadioDotClass = [
  'inline-block h-[6px] w-[6px] shrink-0 rounded-[var(--radius-full)]',
  'bg-[var(--color-primary)] shadow-[0_0_0_1px_color-mix(in_srgb,var(--color-primary)_18%,transparent)]',
].join(' ')
const serviceCopyClass = 'flex min-w-0 flex-col gap-[3px]'
const serviceLabelClass = 'min-w-0 text-[var(--color-text)] text-[length:var(--text-sm)] font-650 leading-[var(--leading-tight)] break-words'
const serviceDescriptionClass = 'min-w-0 text-[var(--color-text-muted)] text-[length:var(--text-xs)] leading-[var(--leading-normal)] break-words'
const customServicePanelClass = [
  'flex flex-col gap-[var(--space-2)] rounded-[var(--radius-md)]',
  'border border-[var(--color-border-subtle)] bg-[color-mix(in_srgb,var(--color-bg-subtle)_58%,transparent)]',
  'p-[var(--space-3)]',
].join(' ')
const customServiceLabelClass = 'text-[var(--color-text-secondary)] text-[length:var(--text-xs)] font-650'

const settingsFieldRowClass = 'grid grid-cols-[minmax(0,1fr)] items-start gap-[var(--space-2)] min-[561px]:grid-cols-[minmax(0,1fr)_auto]'
const settingsErrorClass = 'text-[var(--color-danger)] text-[length:var(--text-xs)]'
const historyListClass = 'flex flex-col gap-[var(--space-1)] rounded-[var(--radius-md)] bg-[var(--color-surface)] p-[var(--space-1)]'
const historyItemClass = 'grid min-w-0 grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-[var(--space-1)]'
const historyUrlClass = [
  'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap border-none rounded-[var(--radius-md)]',
  'bg-transparent p-[var(--space-2)] text-left text-[var(--color-accent)]',
  'text-[length:var(--text-xs)] font-[var(--font-mono)] hover:bg-[var(--color-bg-muted)]',
  'focus-visible:outline-none focus-visible:shadow-[0_0_0_2px_var(--color-accent-subtle)]',
].join(' ')

const identityGridClass = 'grid min-w-0 grid-cols-1 gap-[var(--space-3)]'
const identityEndpointCardClass = [
  'flex min-w-0 flex-col gap-[var(--space-3)] rounded-[var(--radius-module)]',
  'border border-[var(--color-border-subtle)] bg-[var(--color-surface)] p-[var(--space-3)]',
  'shadow-[var(--shadow-sm)]',
].join(' ')
const identityCardHeaderClass = 'flex min-w-0 items-center gap-[var(--space-3)]'
const identityIconClass = [
  'inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-[var(--radius-full)]',
  'border border-[color-mix(in_srgb,var(--color-primary)_16%,transparent)]',
  'bg-[var(--color-primary-subtle)] text-[var(--color-primary)]',
  '[&_svg]:h-[17px] [&_svg]:w-[17px]',
].join(' ')
const identityHeaderCopyClass = 'flex min-w-0 flex-1 flex-col gap-[2px]'
const identityCardTitleClass = 'text-[var(--color-text)] text-[length:var(--text-sm)] font-680 leading-[var(--leading-tight)]'
const identityCardDescriptionClass = 'text-[var(--color-text-muted)] text-[length:var(--text-xs)] leading-[var(--leading-normal)]'
const identityCodeRailClass = [
  'grid min-w-0 grid-cols-[minmax(0,1fr)_32px] items-center gap-[var(--space-2)]',
  'rounded-[var(--radius-md)] border border-[var(--color-border-subtle)]',
  'bg-[color-mix(in_srgb,var(--color-bg-subtle)_66%,transparent)] p-[var(--space-1)]',
].join(' ')
const identityCodeClass = [
  'min-h-8 min-w-0 select-text overflow-x-auto whitespace-nowrap rounded-[calc(var(--radius-md)-2px)]',
  'bg-[color-mix(in_srgb,var(--color-bg)_82%,var(--color-surface))]',
  'px-[var(--space-2)] py-[8px] text-[var(--color-text)]',
  'text-[length:0.72rem] leading-[var(--leading-tight)] [font-family:var(--font-mono)]',
  '[scrollbar-width:none] [&::-webkit-scrollbar]:hidden',
].join(' ')
const identityCopyButtonClass = [
  '!h-8 !min-h-8 !min-w-8 !w-8 rounded-[calc(var(--radius-md)-2px)]',
  '[&_svg]:!h-[14px] [&_svg]:!w-[14px]',
].join(' ')
const identityCopySuccessClass = '!border-[color-mix(in_srgb,var(--color-success)_24%,transparent)] !bg-[var(--color-success-subtle)] !text-[var(--color-success)]'
const identitySideClass = 'flex min-w-0 flex-wrap items-center gap-[var(--space-2)]'
const identityStatusCapsuleClass = [
  'inline-flex min-h-8 min-w-0 max-w-full items-center gap-[var(--space-2)] rounded-[var(--radius-full)] border',
  'border-[color-mix(in_srgb,var(--color-primary)_14%,var(--color-border-subtle))]',
  'bg-[color-mix(in_srgb,var(--color-surface-raised)_58%,var(--color-bg-subtle))]',
  'px-[10px] py-[6px] text-[length:0.74rem] leading-none',
  'shadow-[inset_0_1px_0_color-mix(in_srgb,var(--color-surface-raised)_44%,transparent)]',
].join(' ')
const identityStatusIconClass = 'h-[13px] w-[13px] shrink-0 text-[var(--color-primary)]'
const identityStatusLabelClass = 'shrink-0 text-[var(--color-text-muted)] font-650'
const identityStatusValueClass = 'min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[var(--color-text)] font-720'
const identityStatusDotClass = 'inline-block h-[6px] w-[6px] shrink-0 rounded-[var(--radius-full)]'
const identitySuccessDotClass = 'bg-[var(--color-success)] shadow-[0_0_0_3px_var(--color-success-subtle)]'
const identityNeutralDotClass = 'bg-[var(--color-secondary)] shadow-[0_0_0_3px_var(--color-secondary-subtle)]'
const identityWarningDotClass = 'bg-[var(--color-warning)] shadow-[0_0_0_3px_var(--color-warning-subtle)]'
const identityDangerDotClass = 'bg-[var(--color-danger)] shadow-[0_0_0_3px_var(--color-danger-subtle)]'

function loadHistory(): string[] {
  try {
    const raw = window.localStorage.getItem(RENDEZVOUS_HISTORY_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((s: unknown) => typeof s === 'string' && s.length > 0) : []
  } catch {
    return []
  }
}

function saveHistory(urls: string[]) {
  const filtered = DEFAULT_RENDEZVOUS_URL
    ? urls.filter((u) => u !== DEFAULT_RENDEZVOUS_URL)
    : urls
  window.localStorage.setItem(RENDEZVOUS_HISTORY_KEY, JSON.stringify(filtered.slice(0, 10)))
}

function fullHistory(): string[] {
  const user = loadHistory()
  if (!DEFAULT_RENDEZVOUS_URL) return user
  return [...user.filter((u) => u !== DEFAULT_RENDEZVOUS_URL), DEFAULT_RENDEZVOUS_URL]
}

function addToHistory(url: string) {
  const history = loadHistory().filter((u) => u !== url)
  history.unshift(url)
  saveHistory(history)
}

function removeFromHistory(url: string) {
  if (url === DEFAULT_RENDEZVOUS_URL) return
  saveHistory(loadHistory().filter((u) => u !== url))
}

function validateWsUrl(input: string): string | null {
  const trimmed = input.trim()
  if (!trimmed) return null
  try {
    const url = new URL(trimmed)
    if (url.protocol !== 'ws:' && url.protocol !== 'wss:') return null
    return url.href
  } catch {
    try {
      const url = new URL(`wss://${trimmed}`)
      if (url.protocol !== 'wss:') return null
      return url.href
    } catch {
      return null
    }
  }
}

function detectCurrentOption(): RendezvousOption {
  const stored = window.localStorage.getItem(RENDEZVOUS_URL_STORAGE_KEY)?.trim()
  return stored ? 'custom' : 'same-origin'
}

function currentCustomUrl(): string {
  return window.localStorage.getItem(RENDEZVOUS_URL_STORAGE_KEY)?.trim() ?? ''
}

function formatConnectionState(value: string) {
  return value
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(' ')
}

export default function SettingsDialog() {
  const [copied, setCopied] = createSignal(false)
  const [selectedOption, setSelectedOption] = createSignal<RendezvousOption>(detectCurrentOption())
  const [customInput, setCustomInput] = createSignal(currentCustomUrl() || DEFAULT_RENDEZVOUS_URL)
  const [inputError, setInputError] = createSignal<string | null>(null)
  const [history, setHistory] = createSignal(fullHistory())
  let dialogContentRef: HTMLElement | undefined

  const wasmStatus = () => {
    if (state.node) return 'Loaded'
    if (state.connectionState === 'starting') return 'Starting'
    return 'Unavailable'
  }

  const wasmDotClass = () => {
    if (state.node) return identitySuccessDotClass
    if (state.connectionState === 'starting') return identityWarningDotClass
    return identityNeutralDotClass
  }

  const connectionDotClass = () => {
    if (state.connectionState === 'connected' || state.connectionState === 'ready') return identitySuccessDotClass
    if (state.connectionState === 'connecting' || state.connectionState === 'reconnecting' || state.connectionState === 'fallback') return identityWarningDotClass
    if (state.connectionState === 'error' || state.connectionState === 'disconnected') return identityDangerDotClass
    return identityNeutralDotClass
  }

  const defaultLabel = () => {
    if (DEFAULT_RENDEZVOUS_URL) return DEFAULT_RENDEZVOUS_URL
    return 'Same origin'
  }

  const handleOptionChange = (option: RendezvousOption) => {
    setSelectedOption(option)
    setInputError(null)

    if (option === 'same-origin') {
      window.localStorage.removeItem(RENDEZVOUS_URL_STORAGE_KEY)
      addToast('info', 'Rendezvous set to same origin. Reload to apply.')
      return
    }

    if (option === 'webrtc-local') {
      addToast('info', 'WebRTC local discovery is not yet available')
      setSelectedOption(detectCurrentOption())
      return
    }

    if (!customInput()) setCustomInput(DEFAULT_RENDEZVOUS_URL)
  }

  const handleApplyCustomUrl = () => {
    const validated = validateWsUrl(customInput())
    if (!validated) {
      setInputError('Enter a valid ws:// or wss:// URL')
      return
    }
    setInputError(null)
    window.localStorage.setItem(RENDEZVOUS_URL_STORAGE_KEY, validated)
    addToHistory(validated)
    setHistory(fullHistory())
    setCustomInput(validated)
    setSelectedOption('custom')
    addToast('success', 'Rendezvous URL saved. Reload to apply.')
  }

  const handlePickHistory = (url: string) => {
    setCustomInput(url)
    setInputError(null)
    window.localStorage.setItem(RENDEZVOUS_URL_STORAGE_KEY, url)
    addToHistory(url)
    setHistory(fullHistory())
    setSelectedOption('custom')
    addToast('success', 'Rendezvous URL saved. Reload to apply.')
  }

  const handleRemoveHistory = (url: string) => {
    removeFromHistory(url)
    setHistory(fullHistory())
  }

  const handleCopyEndpointId = async () => {
    if (!state.endpointId) return
    try {
      await navigator.clipboard.writeText(state.endpointId)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1200)
    } catch (err) {
      addToast('error', `Copy failed: ${err instanceof Error ? err.message : String(err)}`)
    }
  }

  const handleOpenAutoFocus = (event: Event) => {
    event.preventDefault()
    queueMicrotask(() => {
      dialogContentRef?.focus({ preventScroll: true })
    })
  }

  return (
    <DialogRoot open={state.settingsOpen} onOpenChange={setSettingsOpen}>
      <DialogContent
        ref={(el) => {
          dialogContentRef = el
        }}
        class="focus:outline-none focus-visible:outline-none"
        onOpenAutoFocus={handleOpenAutoFocus}
      >
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>
            Rendezvous service and browser identity.
          </DialogDescription>
        </DialogHeader>

        <div class={settingsBodyClass}>
          <section class={settingsSectionClass}>
            <div class={settingsSectionHeaderClass}>
              <div class={settingsSectionTitleClass}>Service</div>
              <div class={settingsSectionDescriptionClass}>Choose the rendezvous service used when peers discover each other.</div>
            </div>

            <div class={serviceOptionsClass} role="radiogroup" aria-label="Rendezvous service">
              <button
                type="button"
                class={cx(serviceOptionClass, selectedOption() === 'same-origin' && serviceOptionSelectedClass)}
                role="radio"
                aria-checked={selectedOption() === 'same-origin'}
                onClick={() => handleOptionChange('same-origin')}
              >
                <span class={cx(serviceRadioClass, selectedOption() === 'same-origin' && serviceRadioSelectedClass)} aria-hidden="true">
                  <Show when={selectedOption() === 'same-origin'}>
                    <span class={serviceRadioDotClass} />
                  </Show>
                </span>
                <span class={serviceCopyClass}>
                  <span class={serviceLabelClass}>Default service</span>
                  <span class={serviceDescriptionClass}>{defaultLabel()}</span>
                </span>
                <Badge variant="default">Default</Badge>
              </button>

              <button
                type="button"
                class={cx(serviceOptionClass, selectedOption() === 'custom' && serviceOptionSelectedClass)}
                role="radio"
                aria-checked={selectedOption() === 'custom'}
                onClick={() => handleOptionChange('custom')}
              >
                <span class={cx(serviceRadioClass, selectedOption() === 'custom' && serviceRadioSelectedClass)} aria-hidden="true">
                  <Show when={selectedOption() === 'custom'}>
                    <span class={serviceRadioDotClass} />
                  </Show>
                </span>
                <span class={serviceCopyClass}>
                  <span class={serviceLabelClass}>Custom WebSocket URL</span>
                  <span class={serviceDescriptionClass}>Use a dedicated worker endpoint</span>
                </span>
              </button>

              <button
                type="button"
                class={serviceOptionClass}
                role="radio"
                aria-checked="false"
                disabled
                onClick={() => handleOptionChange('webrtc-local')}
              >
                <span class={serviceRadioClass} aria-hidden="true" />
                <span class={serviceCopyClass}>
                  <span class={serviceLabelClass}>Local discovery</span>
                  <span class={serviceDescriptionClass}>Same-network WebRTC</span>
                </span>
                <Badge variant="secondary">Soon</Badge>
              </button>
            </div>

            <Show when={selectedOption() === 'custom'}>
              <div class={customServicePanelClass}>
                <div class={customServiceLabelClass}>Custom endpoint</div>
                <div class={settingsFieldRowClass}>
                  <Input
                    type="text"
                    class="text-[length:var(--text-xs)] font-[var(--font-mono)]"
                    value={customInput()}
                    placeholder="wss://example.com/__altair_vega_rendezvous"
                    onInput={(event) => {
                      setCustomInput(event.currentTarget.value)
                      setInputError(null)
                    }}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') handleApplyCustomUrl()
                    }}
                  />
                  <Button type="button" onClick={handleApplyCustomUrl}>Apply</Button>
                </div>

                <Show when={inputError()}>
                  <div class={settingsErrorClass}>{inputError()}</div>
                </Show>

                <Show when={history().length > 0}>
                  <div class={customServiceLabelClass}>Recent</div>
                  <div class={historyListClass}>
                    <For each={history()}>
                      {(url) => {
                        const isDefault = () => DEFAULT_RENDEZVOUS_URL && url === DEFAULT_RENDEZVOUS_URL
                        return (
                          <div class={historyItemClass}>
                            <button class={historyUrlClass} type="button" onClick={() => handlePickHistory(url)} title={url}>
                              {url}
                            </button>
                            <Show when={isDefault()}>
                              <Badge variant="secondary">Default</Badge>
                            </Show>
                            <Show when={!isDefault()}>
                              <IconButton
                                label={`Remove ${url}`}
                                variant="ghost"
                                onClick={() => handleRemoveHistory(url)}
                              >
                                <Trash2 size={15} />
                              </IconButton>
                            </Show>
                          </div>
                        )
                      }}
                    </For>
                  </div>
                </Show>
              </div>
            </Show>
          </section>

          <section class={settingsSectionClass}>
            <div class={settingsSectionHeaderClass}>
              <div class={settingsSectionTitleClass}>Identity</div>
              <div class={settingsSectionDescriptionClass}>View this browser endpoint and the active service state.</div>
            </div>

            <div class={identityGridClass}>
              <div class={identityEndpointCardClass}>
                <div class={identityCardHeaderClass}>
                  <span class={identityIconClass} aria-hidden="true">
                    <Fingerprint />
                  </span>
                  <div class={identityHeaderCopyClass}>
                    <div class={identityCardTitleClass}>Endpoint ID</div>
                    <div class={identityCardDescriptionClass}>This browser's peer address for room discovery.</div>
                  </div>
                </div>

                <div class={identityCodeRailClass}>
                  <code class={identityCodeClass} title={state.endpointId || 'Not assigned'}>{state.endpointId || 'Not assigned'}</code>
                  <IconButton
                    class={cx(identityCopyButtonClass, copied() && identityCopySuccessClass)}
                    label={copied() ? 'Endpoint copied' : 'Copy endpoint ID'}
                    variant="outline"
                    onClick={() => void handleCopyEndpointId()}
                    disabled={!state.endpointId}
                  >
                    {copied() ? <Check size={15} /> : <Copy size={15} />}
                  </IconButton>
                </div>
              </div>

              <div class={identitySideClass}>
                <div class={identityStatusCapsuleClass}>
                  <Cpu class={identityStatusIconClass} />
                  <span class={identityStatusLabelClass}>Runtime</span>
                  <span class={cx(identityStatusDotClass, wasmDotClass())} aria-hidden="true" />
                  <span class={identityStatusValueClass}>{wasmStatus()}</span>
                </div>

                <div class={identityStatusCapsuleClass}>
                  <Activity class={identityStatusIconClass} />
                  <span class={identityStatusLabelClass}>State</span>
                  <span class={cx(identityStatusDotClass, connectionDotClass())} aria-hidden="true" />
                  <span class={identityStatusValueClass}>{formatConnectionState(state.connectionState)}</span>
                </div>

                <div class={identityStatusCapsuleClass}>
                  <RadioTower class={identityStatusIconClass} />
                  <span class={identityStatusLabelClass}>Service</span>
                  <span class={identityStatusValueClass}>{selectedOption() === 'custom' ? 'Custom' : 'Default'}</span>
                </div>
              </div>
            </div>
          </section>
        </div>
      </DialogContent>
    </DialogRoot>
  )
}
