import { Link2, RefreshCcw } from 'lucide-solid'
import { createEffect, createSignal, onCleanup } from 'solid-js'

import { state } from '../lib/state'
import { cx } from '../lib/cx'
import { Button } from './ui/Button'

type ConnectionStatusProps = {
  onConnect: () => void
  onDisconnect: () => void
  onReconnect: () => void
}

const connectionStatusBaseClass = 'flex min-h-[26px] max-w-[min(60vw,260px)] select-none items-center gap-[6px]'
const connectionStatusInteractiveClass = 'group'
const statusButtonBaseClass = [
  'inline-flex min-h-[26px] shrink-0 items-center justify-center gap-[5px]',
  'border rounded-[var(--radius-full)] px-[7px] py-[3px]',
  'text-[length:0.7rem] font-650 leading-[var(--leading-tight)] whitespace-nowrap',
  'transition-all duration-[var(--duration-normal)] ease-[var(--ease-out)]',
  'disabled:cursor-not-allowed disabled:opacity-55',
  '[&_svg]:h-[13px] [&_svg]:w-[13px] [&_svg]:shrink-0',
].join(' ')
const statusConnectClass = [
  'border-[color-mix(in_srgb,var(--color-primary)_18%,transparent)]',
  'bg-[var(--color-primary-subtle)] text-[var(--color-primary)]',
  'not-disabled:hover:bg-[var(--color-primary)] not-disabled:hover:text-[var(--color-primary-text)]',
  'not-disabled:hover:shadow-[0_8px_24px_color-mix(in_srgb,var(--color-primary)_20%,transparent)]',
].join(' ')
const statusDisconnectedClass = [
  'border-[color-mix(in_srgb,var(--color-danger)_26%,transparent)]',
  'bg-[var(--color-danger-subtle)] text-[var(--color-danger)]',
  'not-disabled:hover:border-[color-mix(in_srgb,var(--color-primary)_18%,transparent)]',
  'not-disabled:hover:bg-[var(--color-primary)] not-disabled:hover:text-[var(--color-primary-text)]',
  'not-disabled:hover:shadow-[0_8px_24px_color-mix(in_srgb,var(--color-primary)_20%,transparent)]',
].join(' ')
const statusDisconnectedLockedClass = [
  'border-[color-mix(in_srgb,var(--color-danger)_26%,transparent)]',
  'bg-[var(--color-danger-subtle)] text-[var(--color-danger)]',
].join(' ')
const statusDividerClass = 'h-[12px] w-px bg-current opacity-22'
const connectedActionsClass = [
  'grid max-w-0 grid-cols-[auto] overflow-hidden opacity-0 translate-x-[-4px]',
  'transition-all duration-[var(--duration-normal)] ease-[var(--ease-out)]',
  'pointer-events-none group-hover:max-w-[124px] group-hover:opacity-100 group-hover:translate-x-0 group-hover:pointer-events-auto',
  'group-focus-within:max-w-[124px] group-focus-within:opacity-100 group-focus-within:translate-x-0 group-focus-within:pointer-events-auto',
].join(' ')
const reconnectButtonClass = '!min-h-[26px] !rounded-[var(--radius-full)] px-[var(--space-2)] py-[3px] text-[length:0.7rem] [&_svg]:!h-[13px] [&_svg]:!w-[13px]'
const pillNeutralClass = 'bg-[var(--color-secondary-subtle)] text-[var(--color-secondary)]'
const pillSuccessClass = 'bg-[var(--color-success-subtle)] text-[var(--color-success)]'
const pillWarningClass = 'bg-[var(--color-warning-subtle)] text-[var(--color-warning)]'
const pillDangerClass = 'bg-[var(--color-danger-subtle)] text-[var(--color-danger)]'
const pillConnectedActionClass = [
  'border-[color-mix(in_srgb,var(--color-success)_24%,transparent)]',
  pillSuccessClass,
  'hover:border-[color-mix(in_srgb,var(--color-danger)_34%,transparent)] hover:bg-[var(--color-danger-subtle)] hover:text-[var(--color-danger)]',
  'group-hover:border-[color-mix(in_srgb,var(--color-danger)_34%,transparent)] group-hover:bg-[var(--color-danger-subtle)] group-hover:text-[var(--color-danger)]',
  'group-focus-within:border-[color-mix(in_srgb,var(--color-danger)_34%,transparent)] group-focus-within:bg-[var(--color-danger-subtle)] group-focus-within:text-[var(--color-danger)]',
].join(' ')
const pillConnectedLockedClass = [
  'border-[color-mix(in_srgb,var(--color-success)_24%,transparent)]',
  pillSuccessClass,
].join(' ')
const pillFallbackActionClass = [
  'border-[color-mix(in_srgb,var(--color-warning)_24%,transparent)]',
  pillWarningClass,
  'hover:border-[color-mix(in_srgb,var(--color-danger)_34%,transparent)] hover:bg-[var(--color-danger-subtle)] hover:text-[var(--color-danger)]',
  'group-hover:border-[color-mix(in_srgb,var(--color-danger)_34%,transparent)] group-hover:bg-[var(--color-danger-subtle)] group-hover:text-[var(--color-danger)]',
  'group-focus-within:border-[color-mix(in_srgb,var(--color-danger)_34%,transparent)] group-focus-within:bg-[var(--color-danger-subtle)] group-focus-within:text-[var(--color-danger)]',
].join(' ')
const pillBusyClass = [
  'border-[color-mix(in_srgb,var(--color-secondary)_18%,transparent)]',
  pillNeutralClass,
].join(' ')
const statusDotClass = 'inline-block h-[6px] w-[6px] shrink-0 rounded-[var(--radius-full)]'
const statusDotNeutralClass = 'bg-[var(--color-secondary)]'
const statusDotSuccessClass = 'bg-[var(--color-success)]'
const statusDotWarningClass = 'bg-[var(--color-warning)]'
const statusDotDangerClass = 'bg-[var(--color-danger)]'
const statusDotPulseClass = 'animate-[pulse_2s_ease-in-out_infinite]'
const statusConfirmationClass = 'disabled:!cursor-default disabled:!opacity-100'
const CONNECTED_CONFIRMATION_MS = 2400
const DISCONNECTED_CONFIRMATION_MS = 2400

const CONNECTION_STATUS_META = {
  starting: {
    label: 'Starting...',
    pillClass: pillBusyClass,
    dotClass: statusDotNeutralClass,
  },
  ready: {
    label: 'Ready',
    pillClass: statusConnectClass,
    dotClass: statusDotNeutralClass,
  },
  connecting: {
    label: 'Connecting...',
    pillClass: pillBusyClass,
    dotClass: cx(statusDotNeutralClass, statusDotPulseClass),
  },
  connected: {
    label: 'Connected',
    pillClass: pillConnectedActionClass,
    dotClass: statusDotSuccessClass,
  },
  reconnecting: {
    label: 'Reconnecting...',
    pillClass: cx('border-[color-mix(in_srgb,var(--color-warning)_24%,transparent)]', pillWarningClass),
    dotClass: cx(statusDotWarningClass, statusDotPulseClass),
  },
  fallback: {
    label: 'Local only',
    pillClass: pillFallbackActionClass,
    dotClass: statusDotWarningClass,
  },
  disconnected: {
    label: 'Disconnected',
    pillClass: statusDisconnectedClass,
    dotClass: statusDotDangerClass,
  },
  error: {
    label: 'Error',
    pillClass: cx('border-[color-mix(in_srgb,var(--color-danger)_24%,transparent)]', pillDangerClass),
    dotClass: statusDotDangerClass,
  },
} as const

export default function ConnectionStatus(props: ConnectionStatusProps) {
  const [pendingUserConnect, setPendingUserConnect] = createSignal(false)
  const [pendingUserDisconnect, setPendingUserDisconnect] = createSignal(false)
  const [confirmingConnected, setConfirmingConnected] = createSignal(false)
  const [confirmingDisconnected, setConfirmingDisconnected] = createSignal(false)
  let connectedConfirmationTimer = 0
  let disconnectedConfirmationTimer = 0

  const clearConnectedConfirmationTimer = () => {
    if (!connectedConfirmationTimer) return
    window.clearTimeout(connectedConfirmationTimer)
    connectedConfirmationTimer = 0
  }

  const clearDisconnectedConfirmationTimer = () => {
    if (!disconnectedConfirmationTimer) return
    window.clearTimeout(disconnectedConfirmationTimer)
    disconnectedConfirmationTimer = 0
  }

  const meta = () => CONNECTION_STATUS_META[state.connectionState]
  const isConnected = () => state.connectionState === 'connected' || state.connectionState === 'fallback'
  const isBusy = () => state.connectionState === 'starting' || state.connectionState === 'connecting' || state.connectionState === 'reconnecting'
  const isDisconnected = () => state.connectionState === 'disconnected'
  const isReady = () => state.connectionState === 'ready'
  const isConfirmingConnected = () => state.connectionState === 'connected' && confirmingConnected()
  const isConfirmingDisconnected = () => state.connectionState === 'disconnected' && confirmingDisconnected()
  const isConnectedInteractive = () => isConnected() && !isConfirmingConnected()
  const isDisconnectedInteractive = () => isDisconnected() && !isConfirmingDisconnected() && canConnect()
  const canConnect = () => Boolean(state.node) && !isBusy()

  const handleConnect = () => {
    props.onConnect()
    if (state.connectionState === 'connecting' || state.connectionState === 'connected') {
      setPendingUserConnect(true)
    }
  }

  const handleDisconnect = () => {
    props.onDisconnect()
    setPendingUserDisconnect(true)
  }

  const handleReconnect = () => {
    props.onReconnect()
    if (state.connectionState === 'connecting' || state.connectionState === 'reconnecting' || state.connectionState === 'connected') {
      setPendingUserConnect(true)
    }
  }

  createEffect(() => {
    const connectionState = state.connectionState
    const pendingConnect = pendingUserConnect()
    const pendingDisconnect = pendingUserDisconnect()

    if (connectionState === 'connected' && pendingConnect) {
      setPendingUserConnect(false)
      setConfirmingConnected(true)
      clearConnectedConfirmationTimer()
      connectedConfirmationTimer = window.setTimeout(() => {
        setConfirmingConnected(false)
        connectedConfirmationTimer = 0
      }, CONNECTED_CONFIRMATION_MS)
      return
    }

    if (connectionState === 'disconnected' && pendingDisconnect) {
      setPendingUserDisconnect(false)
      setConfirmingDisconnected(true)
      clearDisconnectedConfirmationTimer()
      disconnectedConfirmationTimer = window.setTimeout(() => {
        setConfirmingDisconnected(false)
        disconnectedConfirmationTimer = 0
      }, DISCONNECTED_CONFIRMATION_MS)
      return
    }

    if (connectionState !== 'connecting' && connectionState !== 'reconnecting' && connectionState !== 'connected') {
      setPendingUserConnect(false)
      setConfirmingConnected(false)
      clearConnectedConfirmationTimer()
    }

    if (connectionState !== 'disconnected') {
      setPendingUserDisconnect(false)
      setConfirmingDisconnected(false)
      clearDisconnectedConfirmationTimer()
    }
  })

  onCleanup(() => {
    clearConnectedConfirmationTimer()
    clearDisconnectedConfirmationTimer()
  })

  return (
    <div class={cx(connectionStatusBaseClass, isConnectedInteractive() && connectionStatusInteractiveClass)} aria-live="polite">
      {isConnected() ? (
        <>
          <button
            type="button"
            class={cx(
              statusButtonBaseClass,
              isConfirmingConnected() ? pillConnectedLockedClass : meta().pillClass,
              isConfirmingConnected() && statusConfirmationClass,
            )}
            disabled={isConfirmingConnected()}
            onClick={handleDisconnect}
          >
            <span
              class={cx(
                statusDotClass,
                meta().dotClass,
                isConnectedInteractive() && 'group-hover:hidden group-focus-within:hidden',
              )}
              aria-hidden="true"
            />
            <span class={cx(isConnectedInteractive() && 'group-hover:hidden group-focus-within:hidden')}>{meta().label}</span>
            {isConnectedInteractive() && <span class="hidden group-hover:inline group-focus-within:inline">Disconnect</span>}
          </button>
          {isConnectedInteractive() && (
            <div class={connectedActionsClass}>
              <Button
                type="button"
                class={reconnectButtonClass}
                size="sm"
                onClick={handleReconnect}
              >
                <RefreshCcw size={14} />
                Reconnect
              </Button>
            </div>
          )}
        </>
      ) : isReady() ? (
        <button
          type="button"
          class={cx(statusButtonBaseClass, statusConnectClass)}
          disabled={!canConnect()}
          onClick={handleConnect}
        >
          <Link2 size={13} />
          <span>Connect</span>
        </button>
      ) : (
        <button
          type="button"
          class={cx(
            statusButtonBaseClass,
            isConfirmingDisconnected() ? statusDisconnectedLockedClass : meta().pillClass,
            isDisconnectedInteractive() && 'group',
            isConfirmingDisconnected() && statusConfirmationClass,
          )}
          disabled={!canConnect() || isConfirmingDisconnected()}
          onClick={isDisconnected() ? handleReconnect : handleConnect}
        >
          <span class={cx(isDisconnectedInteractive() && 'group-hover:hidden group-focus-within:hidden')}>
            <span class={cx(statusDotClass, meta().dotClass)} aria-hidden="true" />
          </span>
          <span class={cx(isDisconnectedInteractive() && 'group-hover:hidden group-focus-within:hidden')}>{meta().label}</span>
          {!isBusy() && !isDisconnected() && (
            <>
              <span class={statusDividerClass} aria-hidden="true" />
              <Link2 size={14} />
              <span>Connect</span>
            </>
          )}
          {isDisconnectedInteractive() && (
            <span class="hidden items-center gap-[5px] group-hover:inline-flex group-focus-within:inline-flex">
              <RefreshCcw size={13} />
              <span>Reconnect</span>
            </span>
          )}
        </button>
      )}
    </div>
  )
}
