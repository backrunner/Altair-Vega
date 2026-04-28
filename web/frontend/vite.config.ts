import { defineConfig } from 'vite'
import UnoCSS from 'unocss/vite'
import wasm from 'vite-plugin-wasm'
import topLevelAwait from 'vite-plugin-top-level-await'
import solidPlugin from 'vite-plugin-solid'
import { createDevRendezvousPlugin } from './dev-rendezvous'

export default defineConfig({
  plugins: [UnoCSS(), wasm(), topLevelAwait(), solidPlugin(), createDevRendezvousPlugin()],
  build: {
    target: 'esnext',
  },
  server: {
    host: '127.0.0.1',
    port: 4173,
  },
})
