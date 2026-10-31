/** Update-check state: the model, separate from the plugin call and the UI. */

export type UpdateStatus = "idle" | "checking" | "current" | "available" | "failed";

export interface UpdateState {
  status: UpdateStatus;
  /** The version on offer, or null when none is. */
  version: string | null;
  /** When the last completed check finished, or 0 if none has. */
  checkedAt: number;
}

export const INITIAL_UPDATE_STATE: UpdateState = {
  status: "idle",
  version: null,
  checkedAt: 0,
};

/** What a check that ran to completion produced. A rejection is not one of these. */
export type CheckOutcome =
  | { status: "available"; version: string }
  | { status: "current" };

/**
 * The shape the store needs from a check.
 *
 * Narrower than the plugin's `Update`, so a test or devtools mock can answer
 * without constructing one, and the real implementation stays assignable.
 */
export type CheckFn = () => Promise<{ version: string } | null>;

/**
 * Maps a check result onto an outcome.
 *
 * Kept separate from the call itself so the mapping is testable without the
 * plugin. A thrown check is deliberately *not* represented here: the caller
 * must give it its own `failed` state rather than folding it into `current`,
 * because "could not check" and "nothing newer" are different facts and the
 * UI must not show the first as the second.
 */
export function outcomeOf(update: { version: string } | null): CheckOutcome {
  return update ? { status: "available", version: update.version } : { status: "current" };
}

/** Folds a finished outcome into the next state, stamping when it finished. */
export function applyOutcome(outcome: CheckOutcome, now: number): UpdateState {
  if (outcome.status === "available") {
    return { status: "available", version: outcome.version, checkedAt: now };
  }
  return { status: "current", version: null, checkedAt: now };
}

/**
 * Marks a check as in flight, keeping the last result for the dot and tooltip.
 *
 * Used for a *silent* refresh (the startup check, or re-checking when the notes
 * modal opens), where blanking the panel mid-flight would flicker.
 */
export function markChecking(state: UpdateState): UpdateState {
  return { ...state, status: "checking" };
}

/**
 * Marks a check the user asked for, blanking the previous result.
 *
 * A manual check should visibly resolve: leaving the old version and notes on
 * screen while the request is in flight would let a stale result be mistaken
 * for the answer to the click.
 */
export function markCheckingManual(): UpdateState {
  return { status: "checking", version: null, checkedAt: 0 };
}

/** Records a failed check. `checkedAt` still moves: the attempt did happen. */
export function markFailed(now: number): UpdateState {
  return { status: "failed", version: null, checkedAt: now };
}

/** A coarse age for a timestamp: "just now", "5 min ago", "2 h ago". */
export function relativeAge(from: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - from) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.round(hours / 24)} d ago`;
}

/** The status as a word, for the tooltip that names the last result. */
export function statusWord(status: UpdateStatus): string {
  switch (status) {
    case "idle":
      return "Not checked";
    case "checking":
      return "Checking";
    case "current":
      return "Up to date";
    case "available":
      return "Update available";
    case "failed":
      return "Check failed";
  }
}
