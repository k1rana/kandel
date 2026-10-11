# Contributing

Kandel is a Rust workspace with a Svelte frontend. This page covers the dev setup and the test commands. For what the app does and how to run the built binaries, see the [README](./README.md).

## Development

```bash
pnpm install   # first time only: Tauri CLI and frontend dependencies
pnpm dev       # build core + UI, start Vite, then launch the core with -ui
```

Set `KANDEL_CONFIG` to point the core at a scratch config. Ctrl+C stops everything.

> [!NOTE]
> Use `pnpm dev`, not `tauri dev`. Only the window the core spawns gets the status pipe, so a UI opened by `tauri dev` renders without live state.

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

Integration tests in the `kandel-testkit` crate cover both graceful stop paths (`stop_gui` for `WM_CLOSE`, `stop_console` for `CTRL_BREAK_EVENT`). They need an interactive Windows session, which the CI runner provides:

```bash
cargo test -p kandel-testkit
```

## Commit messages

Pushing to `main` cuts a release, and the next version comes from your commits, so use Conventional Commits: `fix` → patch, `feat` → minor, `BREAKING CHANGE` → major.
