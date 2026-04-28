/* @refresh reload */
import 'virtual:uno.css'
import { render } from 'solid-js/web'
import App from './App'

const root = document.getElementById('root')
if (!root) throw new Error('Root element not found')

// Apply saved theme immediately
const savedTheme = window.localStorage.getItem('altair-vega:theme')
if (savedTheme === 'light' || savedTheme === 'dark') {
  document.documentElement.setAttribute('data-theme', savedTheme)
  document.documentElement.style.colorScheme = savedTheme
} else if (window.matchMedia('(prefers-color-scheme: light)').matches) {
  document.documentElement.setAttribute('data-theme', 'light')
  document.documentElement.style.colorScheme = 'light'
}

render(() => <App />, root)
