/**
 * The one source of update state.
 *
 * A module-level store rather than a prop: the wordmark's dot (`Titlebar`) and
 * the update panel (`About`) are separate components, and a manual re-check in
 * one has to move the other at the same moment. The pure model lives in
 * `update.ts`; this file owns the plugin call and the shared state.
 *
 * `.svelte.ts` so the runes compile in a plain module.
 */

import type { Update } from "@tauri-apps/plugin-updater";

import { findUpdate } from "./api";
import { getReleaseNotes } from "./releaseNotes";
import type { CheckFn } from "./update";
import {
  applyOutcome,
  INITIAL_UPDATE_STATE,
  markChecking,
  markCheckingManual,
  markFailed,
  outcomeOf,
  type UpdateState,
} from "./update";

let state = $state<UpdateState>({ ...INITIAL_UPDATE_STATE });

/**
 * The raw result behind `state.version`.
 *
 * Held as the narrow shape the check returns rather than the plugin's `Update`,
 * so a swapped-in check (a test, the devtools mock) is representable. The
 * installable handle is recovered by {@link currentUpdate}.
 */
let result: { version: string; body?: string | null } | null = null;

/** The check in flight, if any, so concurrent callers share one request. */
let inFlight: Promise<void> | null = null;

/**
 * The check the store runs. Swappable so a devtools mock can drive the UI
 * through every state without a release to point at.
 */
let check: CheckFn = findUpdate;

/** The current state. Read inside a reactive scope to track it. */
export function updateState(): UpdateState {
  return state;
}

/**
 * The installable update handle, or null.
 *
 * Returns null for anything that is not a real plugin `Update` (a mocked check),
 * so an install can never run against a result that was not fetched from the
 * endpoint. Callers treat null as "nothing to install".
 */
export function currentUpdate(): Update | null {
  return result && "downloadAndInstall" in result ? (result as Update) : null;
}

/** Whether a check is running. */
export function isChecking(): boolean {
  return state.status === "checking";
}

/**
 * Checks for a newer release and records the result.
 *
 * `manual` blanks the previous result first, so a user-asked check visibly
 * resolves instead of briefly showing the old version as if it were the answer.
 * Concurrent calls always share one request, so opening About while the startup
 * check is still running does not fire a second one.
 */
export function checkForUpdate(manual = false): Promise<void> {
  if (inFlight) return inFlight;
  state = manual ? markCheckingManual() : markChecking(state);
  inFlight = check()
    .then((update) => {
      result = update;
      state = applyOutcome(outcomeOf(update), Date.now());
    })
    .catch(() => {
      // Its own state, never folded into "up to date": the user is about to be
      // told something about their build, and a failed check cannot support
      // the claim that it is current.
      result = null;
      state = markFailed(Date.now());
    })
    .finally(() => {
      inFlight = null;
    });
  return inFlight;
}

/**
 * Replaces the check the store runs. Local development only.
 *
 * Paired with {@link resetAfterMock} so the real handle and the in-flight guard
 * are cleared too: leaving either behind would let a later real check be
 * skipped, or let an install act on a mocked handle.
 */
export function setCheck(fn: CheckFn): void {
  check = fn;
  reset();
}

/** Puts the real check back, at a clean initial state. */
export function resetAfterMock(): void {
  check = findUpdate;
  reset();
}

function reset(): void {
  result = null;
  inFlight = null;
  state = { ...INITIAL_UPDATE_STATE };
  notes = { status: "idle", body: "", stale: false, message: "", resetAt: null };
  notesFor = null;
}

// ── Release notes ────────────────────────────────────────────────────────────
// Fetched from the GitHub API (see releaseNotes.ts) rather than carried in the
// update manifest, so editing a published release's notes is reflected. The
// cache in that module is what keeps the unauthenticated rate limit at bay, so
// this only ever asks for the version currently on offer.

export interface NotesState {
  status: "idle" | "loading" | "loaded" | "rate-limited" | "error";
  body: string;
  /** True when a cached body was shown because the fetch failed. */
  stale: boolean;
  message: string;
  resetAt: number | null;
}

let notes = $state<NotesState>({
  status: "idle",
  body: "",
  stale: false,
  message: "",
  resetAt: null,
});

/** The version `notes` describes, so a re-opened modal does not refetch. */
let notesFor: string | null = null;
let notesInFlight: Promise<void> | null = null;

/** The notes for the version currently on offer. */
export function notesState(): NotesState {
  return notes;
}

/**
 * Loads the notes for `version`, unless they are already loaded for it.
 *
 * Concurrent callers share one request, and a version already shown is not
 * fetched again, so opening the modal repeatedly costs nothing.
 */
export function loadNotes(version: string): Promise<void> {
  if (notesFor === version && notes.status !== "error") return Promise.resolve();
  if (notesInFlight) return notesInFlight;

  notesFor = version;
  notes = { status: "loading", body: "", stale: false, message: "", resetAt: null };
  notesInFlight = getReleaseNotes(version)
    .then((result) => {
      if (result.status === "ok") {
        notes = { status: "loaded", body: result.body, stale: result.stale, message: "", resetAt: null };
      } else if (result.status === "rate-limited") {
        notes = { status: "rate-limited", body: "", stale: false, message: "", resetAt: result.resetAt };
      } else {
        notes = { status: "error", body: "", stale: false, message: result.message, resetAt: null };
      }
    })
    .finally(() => {
      notesInFlight = null;
    });
  return notesInFlight;
}

/** Forces the next {@link loadNotes} for the same version to fetch again. */
export function retryNotes(): Promise<void> {
  notesFor = null;
  notesInFlight = null;
  const version = state.version ?? "";
  return version ? loadNotes(version) : Promise.resolve();
}
