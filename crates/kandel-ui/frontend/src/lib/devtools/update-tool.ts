/**
 * The updater's devtools tool: a panel plus `window.kandel.updater.*`.
 *
 * The real check needs a published release with a signed manifest, which makes
 * the states (current, available, failed, checking) awkward to reach while
 * designing. Nothing here downloads or runs an installer: it swaps the check
 * function and seeds the notes cache, so Install is inert under a mock and says
 * so when clicked.
 */

import { NOTES_CACHE_KEY } from "../releaseNotes";
import type { CheckFn } from "../update";
import {
  checkForUpdate,
  notesState,
  resetAfterMock,
  setCheck,
  updateState,
} from "../update.svelte";
import { registerDevTool } from "./registry";
import type { Component } from "svelte";
import UpdateTool from "./UpdateTool.svelte";

/** The default version a mock offers, chosen to look newer than any dev build. */
const MOCK_VERSION = "9.9.9";

/** Notes in the shape semantic-release publishes. */
const MOCK_NOTES = `## [9.9.9](https://github.com/k1rana/kandel/compare/v9.9.8...v9.9.9) (2026-10-31)

### Bug Fixes

* **ui:** show release notes in a modal ([abc1234](https://github.com/k1rana/kandel/commit/abc1234))
* **updater:** fetch notes from the client and cache them ([def5678](https://github.com/k1rana/kandel/commit/def5678))

### Features

* **tray:** show a notice when a release is available ([cafe012](https://github.com/k1rana/kandel/commit/cafe012))

A paragraph of prose, to check how wrapped copy reads in the modal.`;

/** Settles a tick later, so the in-flight state is observable rather than skipped. */
function settle<T>(value: T): Promise<T> {
  return new Promise((resolve) => setTimeout(() => resolve(value), 150));
}

const availableCheck =
  (version: string): CheckFn =>
  () =>
    settle({ version });
const currentCheck = (): CheckFn => () => settle(null);
const failCheck = (): CheckFn => () =>
  settle(null).then(() => Promise.reject(new Error("mocked check failure")));
/** Never settles, so the spinner stays on screen. */
const pendingCheck = (): CheckFn => () => new Promise(() => {});

/** Installs `fn` and runs it, moving every consumer of the store at once. */
function use(fn: CheckFn): void {
  setCheck(fn);
  void checkForUpdate();
}

/**
 * Seeds the real notes cache for `version`.
 *
 * The modal reads the GitHub API through `getReleaseNotes`, which serves a
 * fresh cache entry without a request. Writing one here means the mock drives
 * the production path rather than bypassing it, and a bogus tag never reaches
 * GitHub.
 */
function seedNotes(version: string, body: string): void {
  const tag = version.startsWith("v") ? version : `v${version}`;
  try {
    localStorage.setItem(
      NOTES_CACHE_KEY,
      JSON.stringify({ tag, body, etag: null, fetchedAt: Date.now() }),
    );
    void import("../update.svelte").then((m) => m.retryNotes());
  } catch {
    // Storage unavailable: the modal falls back to its error state.
  }
}

let lastVersion = MOCK_VERSION;

const actions = {
  available: (version: string = MOCK_VERSION) => {
    lastVersion = version;
    seedNotes(version, MOCK_NOTES);
    use(availableCheck(version));
  },
  current: () => use(currentCheck()),
  fail: () => use(failCheck()),
  pending: () => use(pendingCheck()),
  /** Replaces the notes the modal will show for the current mock version. */
  notes: (body: string) => seedNotes(lastVersion, body),
  /** Back to the real endpoint. */
  real: () => resetAfterMock(),
  /** The current state, for inspection. */
  state: () => ({ update: updateState(), notes: notesState() }),
};

registerDevTool({
  id: "updater",
  label: "Updater",
  // The cast is the one place the real props are known.
  component: UpdateTool as Component<Record<string, unknown>>,
  props: { actions, version: MOCK_VERSION },
  actions,
});
