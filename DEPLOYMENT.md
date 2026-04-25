# Deployment

This guide is for operators who want to host their own Altair Vega browser app and rendezvous service.

Altair Vega's hosted deployment has two public parts:

- A static browser app built from `web/frontend`.
- A Cloudflare Worker with Durable Objects built from `web/rendezvous-worker`.

The rendezvous service only coordinates peer discovery and signaling. It should not store transferred messages, files, sync data, private keys, or long-lived user state.

## Prerequisites

- A Cloudflare account with Workers and Durable Objects enabled.
- Node.js and npm.
- Rust stable with `wasm32-unknown-unknown` installed.
- `wasm-pack` available on `PATH`.
- A static hosting target for `web/frontend/dist/`.

Install the Rust target:

```sh
rustup target add wasm32-unknown-unknown
```

Install package dependencies:

```sh
npm install --prefix web/rendezvous-worker
npm install --prefix web/frontend
```

## Configuration

The browser app needs the public WebSocket URL of your rendezvous Worker at build time.

Use `.env.example` as a template:

```sh
cp .env.example .env
```

Set this value for browser hosting:

```sh
VITE_DEFAULT_RENDEZVOUS_URL=wss://your-worker.example.com/__altair_vega_rendezvous
```

If the Worker is served from the same origin as the web app, leave `VITE_DEFAULT_RENDEZVOUS_URL` empty and set a same-origin path instead:

```sh
VITE_RENDEZVOUS_PATH=__altair_vega_rendezvous
```

Relative paths support subdirectory deployments. For example, a web app hosted at `/tools/altair-vega/` resolves `__altair_vega_rendezvous` to `/tools/altair-vega/__altair_vega_rendezvous`. Use an absolute path like `/__altair_vega_rendezvous` when the Worker is mounted at the origin root.

If you also build your own native binary, set the matching native default before compiling:

```sh
ALTAIR_VEGA_DEFAULT_RENDEZVOUS=wss://your-worker.example.com/__altair_vega_rendezvous
```

Native users can override the compiled default at runtime with `--room-url <URL>`.

## Rendezvous Worker

The Worker project lives in `web/rendezvous-worker`.

Review `web/rendezvous-worker/wrangler.toml` and set the Worker name, account configuration, and route/custom domain according to your Cloudflare setup.

Run a local Worker during setup:

```sh
npm run dev --prefix web/rendezvous-worker
```

Run a deploy dry run:

```sh
npm run deploy:dry-run --prefix web/rendezvous-worker
```

Deploy to Cloudflare:

```sh
npm run deploy --prefix web/rendezvous-worker
```

After deployment, your browser app should use the Worker's `wss://` URL ending in `/__altair_vega_rendezvous`.

You can also use the operator helper after setting `.env`:

```sh
scripts/deploy.sh
```

For a non-publishing check that still builds the frontend, run:

```sh
scripts/deploy.sh --dry-run
```

## Browser App

Build the browser WASM package and static frontend with your hosted Worker URL:

```sh
VITE_DEFAULT_RENDEZVOUS_URL=wss://your-worker.example.com/__altair_vega_rendezvous npm run build:wasm:release --prefix web/frontend
VITE_DEFAULT_RENDEZVOUS_URL=wss://your-worker.example.com/__altair_vega_rendezvous npm run build --prefix web/frontend
```

Publish the generated static files from:

```text
web/frontend/dist/
```

Any static host is acceptable if it serves the files over HTTPS and preserves the generated asset paths.

## Native Binary Defaults

You can use a released native binary with `--room-url <URL>`, or compile a binary that defaults to your Worker URL:

```sh
ALTAIR_VEGA_DEFAULT_RENDEZVOUS=wss://your-worker.example.com/__altair_vega_rendezvous cargo build --release
```

Example runtime override:

```sh
altair-vega pair --room-url wss://your-worker.example.com/__altair_vega_rendezvous
```

## Smoke Test

After deploying both the Worker and frontend:

- Open the hosted browser app over HTTPS.
- Start a pairing flow from the browser app and confirm it shows a short code.
- Run a native command using the same Worker URL, such as `altair-vega pair --room-url <URL> <CODE>`.
- Confirm the peers connect or report a clear room/connection error.
- Test a small text transfer before using larger files.

## Operational Notes

- Keep the Worker URL stable; browser builds embed `VITE_DEFAULT_RENDEZVOUS_URL`.
- For same-origin subdirectory hosting, keep `VITE_RENDEZVOUS_PATH` relative and route the Worker to the matching prefixed path.
- Use HTTPS for the static app and `wss://` for the Worker URL in production.
- Treat Cloudflare account tokens and deployment credentials as secrets.
- Do not add persistent storage of transfer payloads to the rendezvous service.
- Rebuild and republish the browser app when changing the default rendezvous URL.
- Prefer a custom domain for production so native and browser users can rely on a stable endpoint.

## Troubleshooting

- `426` or WebSocket upgrade errors usually mean the URL is wrong or the request is not reaching the Worker WebSocket endpoint.
- `403` errors usually mean the Worker origin allowlist rejected the caller.
- `409` errors usually mean the room is full; start a fresh code.
- `410` errors usually mean the room expired; start a fresh code.
- Browser connection failures often come from using `ws://` on an HTTPS page; use `wss://` in production.
- Native users can pass `--room-url` to verify a hosted Worker without rebuilding the binary.
