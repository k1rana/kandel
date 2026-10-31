/**
 * Release notes, fetched from the GitHub API and cached in the client.
 *
 * The notes are not in the update manifest, so they stay accurate even if a
 * published release's notes are edited. That costs a second request, against
 * the unauthenticated limit of 60 per hour per IP, so the cache is the point of
 * this module rather than an optimisation:
 *
 *   * Every reply is cached under the tag it describes. Re-opening the notes for
 *     a version already seen makes no request at all.
 *   * A revalidation sends `If-None-Match`, and GitHub does not count a `304`
 *     against the limit, so a refresh after the TTL is free.
 *   * If the fetch fails or the limit is hit, the cached body is served anyway
 *     and flagged stale. Notes for a released version do not change meaning, so
 *     a copy from yesterday beats an error.
 */

/** `owner/name` of the repository whose releases are read. */
const REPO = "k1rana/kandel";

/** Where the cache lives. Survives a reload; cleared with the webview's data. */
export const NOTES_CACHE_KEY = "kandel.releaseNotes";

/** How long a cached body is trusted before it is revalidated. */
export const NOTES_TTL_MS = 6 * 60 * 60 * 1000;

/** The cached notes for one tag. */
export interface CachedNotes {
  tag: string;
  body: string;
  /** The reply's `ETag`, for a conditional revalidation. Null when absent. */
  etag: string | null;
  fetchedAt: number;
}

export type NotesResult =
  /** `stale` is true when a cached body was served because the fetch failed. */
  | { status: "ok"; body: string; stale: boolean }
  | { status: "rate-limited"; resetAt: number | null }
  | { status: "error"; message: string };

/** The git tag a version is published under; the repo tags with a `v` prefix. */
export function tagFor(version: string): string {
  return version.startsWith("v") ? version : `v${version}`;
}

/** Whether a cached entry is still within the TTL. */
export function isFresh(entry: CachedNotes, now: number): boolean {
  return now - entry.fetchedAt < NOTES_TTL_MS;
}

/** Reads the cache, tolerating a missing or corrupt entry. */
export function readCache(storage: Pick<Storage, "getItem">): CachedNotes | null {
  try {
    const raw = storage.getItem(NOTES_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<CachedNotes>;
    if (typeof parsed?.tag !== "string" || typeof parsed?.body !== "string") return null;
    return {
      tag: parsed.tag,
      body: parsed.body,
      etag: typeof parsed.etag === "string" ? parsed.etag : null,
      fetchedAt: typeof parsed.fetchedAt === "number" ? parsed.fetchedAt : 0,
    };
  } catch {
    return null;
  }
}

/** Writes the cache. Storage can throw (private mode); that is not fatal. */
export function writeCache(storage: Pick<Storage, "setItem">, entry: CachedNotes): void {
  try {
    storage.setItem(NOTES_CACHE_KEY, JSON.stringify(entry));
  } catch {
    // The notes still render this session; only the cache is lost.
  }
}

/** The epoch ms from a `x-ratelimit-reset` header, if it is present. */
function resetFrom(headers: Headers): number | null {
  const raw = headers.get("x-ratelimit-reset");
  if (!raw) return null;
  const seconds = Number(raw);
  return Number.isFinite(seconds) ? seconds * 1000 : null;
}

/** Everything this module needs from the outside, so it is testable. */
export interface NotesDeps {
  storage?: Pick<Storage, "getItem" | "setItem">;
  fetchImpl?: typeof fetch;
  now?: () => number;
}

/**
 * Returns the release notes for `version`.
 *
 * Serves a fresh cached copy without a request; otherwise revalidates with the
 * stored ETag, and falls back to a stale copy rather than failing.
 */
export async function getReleaseNotes(
  version: string,
  deps: NotesDeps = {},
): Promise<NotesResult> {
  const storage = deps.storage ?? globalThis.localStorage;
  const doFetch = deps.fetchImpl ?? globalThis.fetch;
  const now = (deps.now ?? Date.now)();
  const tag = tagFor(version);

  const cached = storage ? readCache(storage) : null;
  const usable = cached && cached.tag === tag ? cached : null;

  if (usable && isFresh(usable, now)) {
    return { status: "ok", body: usable.body, stale: false };
  }

  const headers: Record<string, string> = { Accept: "application/vnd.github+json" };
  // Only meaningful when revalidating the same tag; a new tag has no stored
  // validator to send.
  if (usable?.etag) headers["If-None-Match"] = usable.etag;

  let response: Response;
  try {
    response = await doFetch(`https://api.github.com/repos/${REPO}/releases/tags/${tag}`, {
      headers,
    });
  } catch {
    // Offline, DNS, CORS: a cached copy is strictly better than an error.
    if (usable) return { status: "ok", body: usable.body, stale: true };
    return { status: "error", message: "Could not reach GitHub." };
  }

  // 304: unchanged, and not counted against the rate limit.
  if (response.status === 304 && usable) {
    if (storage) writeCache(storage, { ...usable, fetchedAt: now });
    return { status: "ok", body: usable.body, stale: false };
  }

  if (response.status === 403 || response.status === 429) {
    if (usable) return { status: "ok", body: usable.body, stale: true };
    return { status: "rate-limited", resetAt: resetFrom(response.headers) };
  }

  if (!response.ok) {
    if (usable) return { status: "ok", body: usable.body, stale: true };
    return { status: "error", message: `GitHub returned ${response.status}.` };
  }

  const release = (await response.json()) as { body?: unknown };
  const body = typeof release.body === "string" ? release.body : "";
  if (storage) {
    writeCache(storage, { tag, body, etag: response.headers.get("etag"), fetchedAt: now });
  }
  return { status: "ok", body, stale: false };
}
