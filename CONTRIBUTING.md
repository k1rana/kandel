# Contributing

Kandel is a Rust workspace with a Svelte frontend. This page covers the dev setup and the test commands. For what the app does and how to run the built binaries, see the [README](./README.md).

## Development

```bash
pnpm install   # first time only: Tauri CLI and frontend dependencies
pnpm dev       # build core + UI, start Vite, then launch the core with -ui
```

`pnpm dev` runs the whole stack. It builds both binaries, waits for the Vite dev server, then starts the core with the `-ui` flag, which owns the tray and opens the UI window. That window gets hot reload from Vite, and it also gets live status, because the core is the process that spawned it. Ctrl+C stops everything. Set `KANDEL_CONFIG` to point the core at a scratch config.

> [!NOTE]
> `tauri dev` alone is not enough here. It opens its own window with no status pipe, so the UI renders without live state. `pnpm dev` exists to avoid that.

## Tests

```bash
# Rust: portable config logic, reconciliation, Windows supervision
cargo test --workspace

# Frontend: pure helper functions
pnpm run test:ui

# Everything, plus lints
pnpm test
pnpm run check
```

The end-to-end smoke test starts the core against a scratch config, checks that it spawns the managed child, then confirms that exiting the core reaps the process tree through the Job Object:

```bash
bash scripts/smoke_core.sh
```

Integration tests in the `kandel-testkit` crate cover both graceful stop paths by spawning real processes:

```bash
cargo test -p kandel-testkit
```

`stop_gui` drives the `WM_CLOSE` path and `stop_console` the `CTRL_BREAK_EVENT` path. Each asserts the target exited with code 0, which a force-terminated process never records (that path always yields 1), so the tests fail if a graceful stop silently regresses to a kill. They need an interactive Windows session, which the CI runner provides.
