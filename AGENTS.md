# Faith & Fire Workspace Rules

## Managing Running Services Across Conversations

Use `scripts/dev.py` as a normal synchronous command (never launch `server.exe` or `ng serve` as conversation-scoped background daemons):

- Check ports / PIDs / WASM readiness: `python scripts/dev.py status`
- Start backend (`:3000`) & Angular (`:4200`): `python scripts/dev.py start`
- Stop services: `python scripts/dev.py stop` (or `stop server` / `stop web`)
- Restart services: `python scripts/dev.py restart` (or `restart server` / `restart web`)
- Rebuild & reload after edits:
  - Backend Rust changes: `python scripts/dev.py rebuild server` (automatically stops `server.exe` first so Windows releases the binary lock, rebuilds with `--features editor`, and restarts it)
  - Client WASM changes: `python scripts/dev.py rebuild wasm` (runs `scripts/build_wasm.py`; the running Angular dev server picks up updated files automatically)
  - Full stack update: `python scripts/dev.py rebuild`
- Inspect output logs: `python scripts/dev.py logs -n 50` (or `logs server` / `logs web`)

## Windows & Build Conventions

- Use `; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }` instead of `&&` when chaining PowerShell commands.
- Keep all editor code gated behind `#[cfg(feature = "editor")]` in Rust so `cargo check -p client-wasm` (no features) and `npx ng build --configuration production` compile cleanly.
- Before running `cargo build -p server` on Windows directly, stop any running `server.exe` via `python scripts/dev.py stop server`.
- Run Angular editor component tests with `Set-Location web; npx ng test --watch=false --include src/app/editor-ui/editor-ui.component.spec.ts`.
