import { describe, expect, it } from "vitest";

import {
  getReleaseNotes,
  isFresh,
  NOTES_CACHE_KEY,
  NOTES_TTL_MS,
  readCache,
  tagFor,
  writeCache,
} from "./releaseNotes";

/** An in-memory `localStorage`. */
function fakeStorage(seed: string | null = null) {
  const store = new Map<string, string>();
  if (seed !== null) store.set(NOTES_CACHE_KEY, seed);
  return {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    raw: store,
  };
}

/** A `fetch` returning one canned reply. */
function fakeFetch(reply: {
  status: number;
  body?: unknown;
  etag?: string;
  reset?: string;
}) {
  const calls: Array<{ url: string; headers: Record<string, string> }> = [];
  const impl = (async (url: string, init?: { headers?: Record<string, string> }) => {
    calls.push({ url: String(url), headers: init?.headers ?? {} });
    return {
      status: reply.status,
      ok: reply.status >= 200 && reply.status < 300,
      headers: {
        get: (name: string) => {
          if (name === "etag") return reply.etag ?? null;
          if (name === "x-ratelimit-reset") return reply.reset ?? null;
          return null;
        },
      },
      json: async () => reply.body ?? {},
    } as unknown as Response;
  }) as unknown as typeof fetch;
  return { impl, calls };
}

function cached(over: Partial<Parameters<typeof writeCache>[1]> = {}) {
  return JSON.stringify({
    tag: "v1.2.0",
    body: "## 1.2.0",
    etag: null,
    fetchedAt: 0,
    ...over,
  });
}

describe("tagFor", () => {
  it("prefixes a bare version", () => {
    expect(tagFor("1.2.0")).toBe("v1.2.0");
  });

  it("leaves an already-tagged version alone", () => {
    expect(tagFor("v1.2.0")).toBe("v1.2.0");
  });
});

describe("isFresh", () => {
  it("is fresh inside the TTL and stale outside it", () => {
    const entry = { tag: "v1", body: "", etag: null, fetchedAt: 1000 };
    expect(isFresh(entry, 1000 + NOTES_TTL_MS - 1)).toBe(true);
    expect(isFresh(entry, 1000 + NOTES_TTL_MS)).toBe(false);
  });
});

describe("cache round trip", () => {
  it("survives a write and read", () => {
    const storage = fakeStorage();
    writeCache(storage, { tag: "v1.2.0", body: "notes", etag: '"abc"', fetchedAt: 5 });
    expect(readCache(storage)).toEqual({
      tag: "v1.2.0",
      body: "notes",
      etag: '"abc"',
      fetchedAt: 5,
    });
  });

  it("rejects a corrupt entry rather than throwing", () => {
    expect(readCache(fakeStorage("{ not json"))).toBeNull();
    expect(readCache(fakeStorage('{"tag":1}'))).toBeNull();
  });
});

describe("getReleaseNotes", () => {
  it("serves a fresh cache entry without any request", async () => {
    const storage = fakeStorage(cached({ fetchedAt: 1000 }));
    const { impl, calls } = fakeFetch({ status: 200 });
    const result = await getReleaseNotes("1.2.0", {
      storage,
      fetchImpl: impl,
      now: () => 1000 + 10,
    });
    expect(result).toEqual({ status: "ok", body: "## 1.2.0", stale: false });
    expect(calls).toHaveLength(0);
  });

  it("fetches and caches on a miss, sending no validator", async () => {
    const storage = fakeStorage();
    const { impl, calls } = fakeFetch({ status: 200, body: { body: "## 1.3.0" }, etag: '"e1"' });
    const result = await getReleaseNotes("1.3.0", { storage, fetchImpl: impl, now: () => 42 });
    expect(result).toEqual({ status: "ok", body: "## 1.3.0", stale: false });
    expect(calls[0].url).toContain("/releases/tags/v1.3.0");
    expect(calls[0].headers["If-None-Match"]).toBeUndefined();
    expect(readCache(storage)).toEqual({
      tag: "v1.3.0",
      body: "## 1.3.0",
      etag: '"e1"',
      fetchedAt: 42,
    });
  });

  it("revalidates a stale entry with the stored ETag", async () => {
    const storage = fakeStorage(cached({ etag: '"old"', fetchedAt: 0 }));
    const { impl, calls } = fakeFetch({ status: 304 });
    const result = await getReleaseNotes("1.2.0", {
      storage,
      fetchImpl: impl,
      now: () => NOTES_TTL_MS + 10,
    });
    expect(calls[0].headers["If-None-Match"]).toBe('"old"');
    expect(result).toEqual({ status: "ok", body: "## 1.2.0", stale: false });
    // The 304 refreshes the timestamp, so the next read is free again.
    expect(readCache(storage)?.fetchedAt).toBe(NOTES_TTL_MS + 10);
  });

  it("ignores a cache entry for a different tag", async () => {
    const storage = fakeStorage(cached({ tag: "v1.1.0" }));
    const { impl, calls } = fakeFetch({ status: 200, body: { body: "## 1.2.0" } });
    const result = await getReleaseNotes("1.2.0", { storage, fetchImpl: impl, now: () => 5 });
    expect(calls).toHaveLength(1);
    expect(result).toEqual({ status: "ok", body: "## 1.2.0", stale: false });
  });

  it("serves a stale cached body when GitHub reports the limit", async () => {
    const storage = fakeStorage(cached({ fetchedAt: 0 }));
    const { impl } = fakeFetch({ status: 403, reset: "1700000000" });
    const result = await getReleaseNotes("1.2.0", {
      storage,
      fetchImpl: impl,
      now: () => NOTES_TTL_MS + 1,
    });
    expect(result).toEqual({ status: "ok", body: "## 1.2.0", stale: true });
  });

  it("reports rate-limited with the reset time when nothing is cached", async () => {
    const storage = fakeStorage();
    const { impl } = fakeFetch({ status: 403, reset: "1700000000" });
    const result = await getReleaseNotes("1.2.0", { storage, fetchImpl: impl, now: () => 0 });
    expect(result).toEqual({ status: "rate-limited", resetAt: 1_700_000_000_000 });
  });

  it("falls back to the cache when the network fails", async () => {
    const storage = fakeStorage(cached({ fetchedAt: 0 }));
    const impl = (async () => {
      throw new Error("offline");
    }) as unknown as typeof fetch;
    const result = await getReleaseNotes("1.2.0", {
      storage,
      fetchImpl: impl,
      now: () => NOTES_TTL_MS + 1,
    });
    expect(result).toEqual({ status: "ok", body: "## 1.2.0", stale: true });
  });

  it("reports an error when the network fails with nothing cached", async () => {
    const storage = fakeStorage();
    const impl = (async () => {
      throw new Error("offline");
    }) as unknown as typeof fetch;
    const result = await getReleaseNotes("1.2.0", { storage, fetchImpl: impl, now: () => 0 });
    expect(result).toEqual({ status: "error", message: "Could not reach GitHub." });
  });

  it("treats a missing body as empty notes", async () => {
    const storage = fakeStorage();
    const { impl } = fakeFetch({ status: 200, body: { name: "v1.2.0" } });
    const result = await getReleaseNotes("1.2.0", { storage, fetchImpl: impl, now: () => 0 });
    expect(result).toEqual({ status: "ok", body: "", stale: false });
  });

  it("tolerates storage that throws, still returning the notes", async () => {
    const hostile = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    const { impl } = fakeFetch({ status: 200, body: { body: "## ok" } });
    const result = await getReleaseNotes("1.2.0", {
      storage: hostile,
      fetchImpl: impl,
      now: () => 0,
    });
    expect(result).toEqual({ status: "ok", body: "## ok", stale: false });
  });
});
