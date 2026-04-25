# Development

## Architecture

Altair Vega has three release surfaces:

- Native CLI: Rust binary for pairing, text transfer, file transfer, browser bridging, and two-peer folder sync.
- Browser app: static SolidJS frontend plus a Rust/WASM peer package built with `wasm-pack`.
- Rendezvous Worker: Cloudflare Worker/Durable Object WebSocket room service used only for peer discovery and signaling.

Native peers use `iroh` directly for endpoint connections, local discovery, file transfer, and native sync. Browser peers use the WASM peer package for browser-to-browser transfer behavior and the frontend rendezvous client for WebSocket signaling. The rendezvous Worker coordinates room membership and forwards signaling messages; it does not persist room contents or transferred payloads.

High-level data flow:

- Native-to-native local path: short code scopes discovery, peers exchange bootstrap data locally, then transfer over `iroh`.
- Native/browser rendezvous path: peers join a Worker room by code, exchange signaling/bootstrap messages, then transfer through the supported peer transport.
- Native sync path: one peer hosts a docs namespace, a second peer follows or joins, and filesystem changes reconcile through the sync manifest policy.

## Project Structure

- `src/`: native CLI entrypoint, shared protocol types, pairing, transfer, runtime, and sync logic.
- `src/main.rs`: CLI command surface and native rendezvous/local-discovery orchestration.
- `src/control.rs`: shared control frames and transfer metadata.
- `src/files.rs`: native file transfer and resume behavior.
- `src/sync.rs`: native filesystem scan, manifest diff/merge, and local apply policy.
- `src/sync_docs.rs`: docs-backed native sync host/follow/join behavior.
- `web/browser-wasm/`: Rust/WASM browser peer package exposed to the frontend.
- `web/frontend/`: SolidJS static web app, browser state, and rendezvous client.
- `web/rendezvous-worker/`: Cloudflare Worker and Durable Object rendezvous service.
- `scripts/`: disposable native launcher scripts for POSIX and PowerShell.

## Development Setup

Required tools:

- Rust stable with the `wasm32-unknown-unknown` target.
- Node.js and npm.
- `wasm-pack` for browser WASM builds.
- Cloudflare Wrangler through `web/rendezvous-worker` dependencies for Worker development and deployment.
- PowerShell if validating the Windows launcher syntax/behavior locally.

Install Rust target:

```sh
rustup target add wasm32-unknown-unknown
```

Install JavaScript dependencies:

```sh
npm install --prefix web/frontend
npm install --prefix web/rendezvous-worker
```

Prepare local environment defaults:

```sh
cp .env.example .env.development
```

Use `.env` for deployment-specific values only. Do not commit real secrets, private keys, or production release credentials.

## Common Tasks

Run the native CLI directly from source:

```sh
cargo run -- --help
cargo run -- pair
```

Build the browser WASM package and frontend:

```sh
npm run build:wasm:release --prefix web/frontend
npm run build --prefix web/frontend
```

Run the frontend dev server:

```sh
npm run dev --prefix web/frontend
```

Run the rendezvous Worker locally:

```sh
npm run dev --prefix web/rendezvous-worker
```

Build the native release binary:

```sh
cargo build --release
```

## Testing

Native validation:

```sh
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cargo build --release
```

Browser validation:

```sh
npm run build:wasm:release --prefix web/frontend
npm run build --prefix web/frontend
```

Worker validation:

```sh
npm run dev --prefix web/rendezvous-worker
```

Release validation still requires manual drills on actual target systems for macOS, Windows, browser matrix coverage, launcher behavior, reconnect behavior, and long-running native sync.

## Release Discipline

Product functionality is frozen during release hardening. Changes should be limited to bug fixes, validation shorthand, packaging, release automation, and documentation. Do not add new product features unless the release scope is explicitly reopened.
