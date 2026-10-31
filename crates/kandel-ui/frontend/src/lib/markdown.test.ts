import { describe, expect, it } from "vitest";

import { parseReleaseNotes } from "./markdown";

describe("parseReleaseNotes", () => {
  it("reads a version heading", () => {
    const blocks = parseReleaseNotes("## [1.2.0](https://example.com) (2026-10-31)");
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ kind: "heading", level: 2 });
    expect(blocks[0].kind === "heading" && blocks[0].spans).toEqual([
      { kind: "link", text: "1.2.0" },
      { kind: "text", text: " (2026-10-31)" },
    ]);
  });

  it("groups consecutive bullets into one list", () => {
    const blocks = parseReleaseNotes("### Fixes\n\n* one\n* two\n");
    expect(blocks.map((b) => b.kind)).toEqual(["heading", "list"]);
    const list = blocks[1];
    expect(list.kind === "list" && list.items).toHaveLength(2);
  });

  it("renders a bold commit scope", () => {
    const blocks = parseReleaseNotes("* **config:** tolerate broken entries");
    const list = blocks[0];
    const first = list.kind === "list" ? list.items[0] : [];
    expect(first[0]).toEqual({ kind: "strong", text: "config:" });
  });

  it("keeps inline code", () => {
    const blocks = parseReleaseNotes("Run `pnpm dev` first.");
    const para = blocks[0];
    expect(para.kind === "paragraph" && para.spans).toEqual([
      { kind: "text", text: "Run " },
      { kind: "code", text: "pnpm dev" },
      { kind: "text", text: " first." },
    ]);
  });

  it("drops a link's URL, keeping its text", () => {
    const blocks = parseReleaseNotes("See [the compare](https://example.com/a/b).");
    const para = blocks[0];
    const spans = para.kind === "paragraph" ? para.spans : [];
    expect(spans).toEqual([
      { kind: "text", text: "See " },
      { kind: "link", text: "the compare" },
      { kind: "text", text: "." },
    ]);
  });

  it("joins a wrapped paragraph into one block", () => {
    const blocks = parseReleaseNotes("one\ntwo\n\nthree");
    expect(blocks).toHaveLength(2);
    const first = blocks[0];
    expect(first.kind === "paragraph" && first.spans).toEqual([{ kind: "text", text: "one two" }]);
  });

  it("returns nothing for empty notes", () => {
    expect(parseReleaseNotes("")).toEqual([]);
    expect(parseReleaseNotes("\n\n  \n")).toEqual([]);
  });

  it("never emits markup, whatever the source contains", () => {
    // The renderer builds elements from these blocks, so a script tag is only
    // ever text.
    const blocks = parseReleaseNotes("<script>alert(1)</script>");
    const para = blocks[0];
    expect(para.kind === "paragraph" && para.spans).toEqual([
      { kind: "text", text: "<script>alert(1)</script>" },
    ]);
  });
});
