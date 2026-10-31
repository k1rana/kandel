/**
 * A small markdown subset parser for release notes.
 *
 * GitHub release bodies are markdown. Rather than pull in a full parser plus a
 * sanitizer, this maps the subset `semantic-release` emits onto a block model
 * the UI renders with ordinary elements. Nothing is ever handed to `{@html}`,
 * so note text cannot reach the webview as markup however it is written.
 */

/** A run of text inside a block, with its emphasis. */
export interface Inline {
  kind: "text" | "strong" | "code" | "link";
  text: string;
}

export type Block =
  | { kind: "heading"; level: number; spans: Inline[] }
  | { kind: "list"; items: Inline[][] }
  | { kind: "paragraph"; spans: Inline[] };

/**
 * Splits a line into styled runs.
 *
 * Links keep only their text: the webview has no way to hand an arbitrary URL
 * to the OS browser (the Rust command takes no URL argument), so a link must
 * not look clickable. The URL is deliberately dropped rather than rendered dead.
 */
function parseInline(text: string): Inline[] {
  const spans: Inline[] = [];
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\([^)\s]+\)/g;
  let last = 0;
  let match: RegExpExecArray | null;

  while ((match = re.exec(text))) {
    if (match.index > last) spans.push({ kind: "text", text: text.slice(last, match.index) });
    if (match[1] !== undefined) spans.push({ kind: "strong", text: match[1] });
    else if (match[2] !== undefined) spans.push({ kind: "code", text: match[2] });
    else if (match[3] !== undefined) spans.push({ kind: "link", text: match[3] });
    last = re.lastIndex;
  }
  if (last < text.length) spans.push({ kind: "text", text: text.slice(last) });
  return spans;
}

/** Parses release-note markdown into blocks. */
export function parseReleaseNotes(markdown: string): Block[] {
  const blocks: Block[] = [];
  let items: Inline[][] | null = null;
  let paragraph: string[] = [];

  const flushParagraph = () => {
    if (!paragraph.length) return;
    blocks.push({ kind: "paragraph", spans: parseInline(paragraph.join(" ")) });
    paragraph = [];
  };
  const flushList = () => {
    if (!items) return;
    blocks.push({ kind: "list", items });
    items = null;
  };

  for (const raw of markdown.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trim();
    if (!line) {
      flushParagraph();
      flushList();
      continue;
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      flushParagraph();
      flushList();
      blocks.push({ kind: "heading", level: heading[1].length, spans: parseInline(heading[2]) });
      continue;
    }

    const bullet = /^[-*+]\s+(.*)$/.exec(line);
    if (bullet) {
      flushParagraph();
      (items ??= []).push(parseInline(bullet[1]));
      continue;
    }

    // A plain line continues the run of text; markdown soft-wraps it.
    flushList();
    paragraph.push(line);
  }

  flushParagraph();
  flushList();
  return blocks;
}
