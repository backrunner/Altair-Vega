import './Header.css'

import ConnectionStatus from './ConnectionStatus'
import { state, toggleTheme, toggleSidebar } from '../lib/state'

function SunIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="4" fill="none" stroke="currentColor" stroke-width="1.8" />
      <path
        d="M12 2.5v2.5M12 19v2.5M21.5 12H19M5 12H2.5M18.72 5.28l-1.77 1.77M7.05 16.95l-1.77 1.77M18.72 18.72l-1.77-1.77M7.05 7.05L5.28 5.28"
        fill="none"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-width="1.8"
      />
    </svg>
  )
}

function MoonIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M20 14.2A8.5 8.5 0 0 1 9.8 4 8.5 8.5 0 1 0 20 14.2Z"
        fill="none"
        stroke="currentColor"
        stroke-linejoin="round"
        stroke-width="1.8"
      />
    </svg>
  )
}

function MenuIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 7h16M4 12h16M4 17h16" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.8" />
    </svg>
  )
}

function CloseIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 6l12 12m0-12L6 18" fill="none" stroke="currentColor" stroke-linecap="round" stroke-width="1.8" />
    </svg>
  )
}

export default function Header() {
  const themeLabel = () => (state.theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode')
  const menuLabel = () => (state.sidebarOpen ? 'Close menu' : 'Open menu')

  return (
    <header class="header">
      <div class="header__brand-group">
        <button
          class="btn btn-ghost btn-icon header__icon-button header__menu-toggle"
          type="button"
          onClick={toggleSidebar}
          aria-label={menuLabel()}
          title={menuLabel()}
        >
          {state.sidebarOpen ? <CloseIcon /> : <MenuIcon />}
        </button>
        <div class="header__brand">
          <img class="header__logo" src="logo.png" alt="" aria-hidden="true" />
          <span>Altair-Vega</span>
        </div>
        <ConnectionStatus />
      </div>

      <div class="header__actions">
        <button class="btn btn-ghost btn-icon header__icon-button" type="button" onClick={toggleTheme} aria-label={themeLabel()} title={themeLabel()}>
          {state.theme === 'dark' ? <SunIcon /> : <MoonIcon />}
        </button>
      </div>
    </header>
  )
}
