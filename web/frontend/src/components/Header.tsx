import { Menu, Moon, Settings, Sun } from 'lucide-solid'

import ConnectionStatus from './ConnectionStatus'
import SettingsDialog from './SettingsDialog'
import { IconButton } from './ui/Button'
import { setSettingsOpen, state, toggleSidebar, toggleTheme } from '../lib/state'

type HeaderProps = {
  onConnect: () => void
  onDisconnect: () => void
  onReconnect: () => void
}

const navbarClass = [
  'relative z-[120] flex select-none min-h-[58px] shrink-0 items-center justify-between',
  'gap-[var(--space-2)] border border-[var(--color-border)] rounded-[var(--radius-module)]',
  'bg-[color-mix(in_srgb,var(--color-surface)_88%,transparent)]',
  'py-[var(--space-2)] pl-[var(--space-2)] pr-[var(--space-2)]',
  'shadow-[var(--shadow-md)] backdrop-blur-[14px]',
  'min-[769px]:gap-[var(--space-4)] min-[769px]:pl-[var(--space-4)] min-[769px]:pr-[var(--space-3)]',
].join(' ')

const navbarClusterClass = 'flex min-w-0 flex-1 items-center gap-[6px]'
const navbarPrimaryClass = 'flex min-w-0 items-center gap-[8px] min-[561px]:gap-[14px] min-[769px]:gap-[var(--space-4)]'
const navbarBrandClass = [
  'inline-flex shrink-0 items-center gap-[5px] min-[561px]:gap-[10px] min-[769px]:gap-[var(--space-2)]',
  'text-[var(--color-text)] text-[length:0.86rem] min-[769px]:text-[length:var(--text-base)] font-bold whitespace-nowrap',
].join(' ')
const navbarLogoClass = 'h-[30px] w-[30px] rounded-[var(--radius-md)] object-cover shadow-[0_8px_24px_color-mix(in_srgb,var(--color-primary)_24%,transparent)] min-[769px]:h-8 min-[769px]:w-8'
const navbarActionsClass = 'flex shrink-0 items-center gap-[2px] min-[769px]:gap-[var(--space-1)]'
const navbarIconButtonClass = [
  '!h-8 !min-h-8 !min-w-8 !w-8 !rounded-[var(--radius-md)]',
  'min-[769px]:!h-[36px] min-[769px]:!min-h-[36px] min-[769px]:!min-w-[36px] min-[769px]:!w-[36px]',
].join(' ')
const navbarMenuButtonClass = [
  '!h-8 !min-h-8 !max-h-8 !min-w-8 !w-8 !max-w-8 !flex-none !rounded-[var(--radius-md)] !p-0',
  'min-[769px]:hidden [&_svg]:!h-[16px] [&_svg]:!w-[16px]',
].join(' ')

export default function Header(props: HeaderProps) {
  const themeLabel = () => (state.theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode')
  const sidebarLabel = () => (state.sidebarOpen ? 'Close sidebar' : 'Open sidebar')

  return (
    <header class={navbarClass}>
      <div class={navbarClusterClass}>
        <IconButton
          class={navbarMenuButtonClass}
          variant="ghost"
          label={sidebarLabel()}
          aria-controls="app-sidebar"
          aria-expanded={state.sidebarOpen}
          onClick={toggleSidebar}
        >
          <Menu size={18} />
        </IconButton>

        <div class={navbarPrimaryClass}>
          <div class={navbarBrandClass}>
            <img class={navbarLogoClass} src="logo.png" alt="" aria-hidden="true" />
            <span>Altair-Vega</span>
          </div>

          <ConnectionStatus
            onConnect={props.onConnect}
            onDisconnect={props.onDisconnect}
            onReconnect={props.onReconnect}
          />
        </div>
      </div>

      <div class={navbarActionsClass}>
        <IconButton
          class={navbarIconButtonClass}
          variant="ghost"
          label={themeLabel()}
          onClick={toggleTheme}
        >
          {state.theme === 'dark' ? <Sun size={18} /> : <Moon size={18} />}
        </IconButton>
        <IconButton
          class={navbarIconButtonClass}
          variant="ghost"
          label="Open settings"
          onClick={() => setSettingsOpen(true)}
        >
          <Settings size={18} />
        </IconButton>
      </div>

      <SettingsDialog />
    </header>
  )
}
