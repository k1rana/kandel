# Contributing

Kandel is a Rust workspace with a Svelte frontend. This page covers the dev setup, the test commands, and the commit conventions. For what the app does and how to run the built binaries, see the [README](./README.md).

## Development

```bash
pnpm install   # first time only: Tauri CLI and frontend dependencies
pnpm dev       # build core + UI, start Vite, then launch the core with -ui
```

`pnpm dev` runs the core against a repo-local scratch config at `.dev/config.toml`, created on first run and gitignored, so your real `%USERPROFILE%\.config\kandel\config.toml` is never touched. Set `KANDEL_CONFIG` to point at a different file. Ctrl+C stops everything.

Dev builds use a separate identity from release builds (the single-instance lock, the status pipe, and the tray icon all differ), so a `pnpm dev` core can run at the same time as an installed Kandel without the two colliding. Two dev cores still cannot run together.

> [!NOTE]
> `pnpm dev` runs Vite and the core together. A UI opened any other way (including `tauri dev`) attaches to the core's status pipe on its own, so it shows live state whenever a core is running.

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
