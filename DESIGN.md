# Design

<!-- impeccable:design-schema 1 -->

Visual system for the Kandel management UI (`crates/kandel-ui/frontend`). The
world is the **Perk** reference: electric lime on warm parchment, recorded from
the built surface. Tokens live in `src/theme.css`; the machine-readable mirror is
`.impeccable/design.json`.

## Direction

An editorial status ledger. Warm paper, one electric-lime voice, and a printed
ledger of supervised apps — a deliberate refusal of the category default (a dark
neon dashboard of identical glossy cards).

Surface mode: **Operate**. The world lends only type, palette, density, and one
signature move; layout, navigation, and controls stay standard so the task is
never obscured.

## Colour

Achromatic except for one lime, which is the only filled action and the primary
health signal. Amber and red appear strictly as meaning-bearing state colour on
degraded/failed entries — never as decoration.

| Token         | Value     | Role                                                        |
| ------------- | --------- | ----------------------------------------------------------- |
| `--lime`      | `#beff50` | Only filled action; the "alive" health charge.              |
| `--ink`       | `#14140f` | All text, rules, icon fills, button text.                   |
| `--charcoal`  | `#30302a` | Dark island surfaces (rare).                                |
| `--graphite`  | `#6e6e64` | Secondary text, muted copy.                                 |
| `--stone`     | `#919183` | Faint strokes and outlines.                                 |
| `--smoke`     | `#b9b9b7` | Placeholder wash; neutral charge.                           |
| `--ash`       | `#d2d2c8` | Borders, dividers, structure lines.                         |
| `--parchment` | `#f5f5eb` | Card surfaces (the ledger).                                 |
| `--canvas`    | `#ffffff` | Page ground; raised surfaces, inputs, bars.                 |
| `--alert`     | `#b83c28` | Failed state only (tuned to clear AA on parchment).         |
| `--degraded`  | `#96581a` | Starting/Restarting/Retrying state only.                    |

### Surface stack

Elevation is tonal, never a shadow. The stack is:

`canvas #ffffff` → `parchment #f5f5eb` → `lime #beff50` → `charcoal #30302a`.

A row "lifts" on hover by turning lighter, not by gaining a shadow.

## Dark mode

Dark mode is the **same warm world after dark**, not a second identity. It is a
token override on `[data-theme="dark"]` in `theme.css`; no component carries a
dark-specific colour.

- The ground is the world's own ink `#14140f`, **never a blue-black slate**.
  A cold ground is the category default and would break the warm-paper
  identity.
- Surfaces lift by turning **lighter and warmer**, the exact inverse of light:
  ground `#14140f` → card `#1e1e17` → hover `#26261e`; inset fields drop to
  `#0e0e0a`.
- **Lime is unchanged** (`#beff50`) — it is the brand and the health signal in
  both themes, at 15:1 on the dark ground.
- `--alert` and `--degraded` lighten to `#ff8f75` / `#e0a44a` so state reads on
  a dark ground.
- Overlays (`--scrim`, `--veil`) cast from black, and `--charge-ring` inverts
  to a light outline.
- `color-scheme` is set on the root so native controls, scrollbars, and the
  canvas behind the page follow.

### Theme selection

The topbar toggle cycles **Auto → Light → Dark**. `Auto` follows the OS via
`prefers-color-scheme` and updates live. The choice persists in `localStorage`
under `kandel.theme`; the rendered theme is applied as `data-theme` on the
`<html>` element, and the OS window chrome follows via `setTheme`.

### Dark contrast (WCAG, measured)

| Pair                    | Ratio |
| ----------------------- | ----- |
| ink on ground           | 16.8  |
| ink on card             | 15.3  |
| ink on inset field      | 17.6  |
| graphite (secondary)    | 6.6   |
| stone (faint)           | 3.9   |
| alert                   | 7.5   |
| degraded                | 7.6   |
| lime on ground          | 15.5  |

## Typography

One family carries every voice: **Sono** (self-hosted at
`src/assets/fonts/sono-*-var.woff2`), proportional instance. Weights **400** and
**500** only — never 600/700. Tracking is `-0.03em` at 28px and above, default at
body sizes, and `0.1em` on the uppercase micro-caps.

| Role        | Size | Weight | Line height | Tracking |
| ----------- | ---- | ------ | ----------- | -------- |
| display     | 60px | 500    | 0.95        | -0.03em  |
| heading     | 28px | 500    | 1.14        | -0.03em  |
| subheading  | 22px | 500    | 1.18        | -0.03em  |
| body        | 16px | 400    | 1.5         | default  |
| body-sm     | 14px | 400/500| 1.43        | default  |
| caption     | 12px | 400/500| 1.33        | default  |
| eyebrow     | 10px | 500    | 1.4         | 0.1em    |

All-caps micro-labels are reserved for structural markers (ledger column
headers, the status word, the tally). There is no italic and no condensed cut;
the restraint is the signature.

## Shape

| Element | Radius  |
| ------- | ------- |
| inputs  | 8px     |
| inner   | 18px    |
| cards   | 28px    |
| buttons | 28px    |
| pills   | 9999px  |

Radii are not mixed within a component type: every button that is a pill is a
pill, every card is 28px.

## Spacing

4px base unit. Table rows and card padding use 16–20px; sections and the page
gutter use 24px; the empty state breathes at 48px.

## Components

- **Topbar** — a white 54px bar: wordmark, a micro-caps live tally, the lime
  "Add app" pill, and three window controls. The bar is the window drag region.
- **Ledger** — a parchment card, 28px, with an ink-ruled micro-caps header and
  one row per app: state charge, name, command line, state word, right-aligned
  tabular PID and restart count.
- **State charge** — a 10px rounded square: lime (running), amber (starting,
  restarting, retrying), red (failed), hollow stone ring (stopped/paused). A real
  border, never a shadow. The state word carries the meaning; colour never stands
  alone.
- **Editor panel** — a white 28px panel over a `rgba(20,20,15,0.32)` scrim. The
  header and footer are pinned; only the form body scrolls. Focus is trapped
  and moved to the name field on open.
- **Row menu** — a white popover with a 1px ink border and 18px radius.
- **Drop veil** — a full-window overlay shown while a file is dragged over the
  window.

## Motion

One authored moment: the editor panel rises 14px with `0.2s` `--ease-out`.
Interactive transitions (hover, focus, state) are `0.12–0.15s` and limited to
background, colour, and border. There is no entrance animation on list rows and
no scroll-triggered motion. `prefers-reduced-motion` collapses all of it.

## Browser surfaces

These ship as part of the design rather than at platform defaults:

- text selection: lime background, ink text;
- scrollbars: warm, themed from `--ash`/`--stone`;
- focus ring: 2px ink at a 2px offset (ink, because lime already means "alive");
- numeric data (`PID`, restarts): tabular figures via `.tnum`;
- checkboxes: `accent-color: var(--ink)`; number steppers kept as a real
  affordance and ink-tinted.

## Accessibility

Body and secondary text clear WCAG AA (4.5:1) on both parchment and white — the
weakest pair is graphite-on-parchment at 4.69:1. State is never conveyed by
colour alone: every charge is paired with a text word. Focus is visible on all
interactive elements, and the editor traps and restores focus.

## Imagery

None. The system is typographic and tonal; it ships no photography, no
illustration, and no icon set beyond a handful of drawn 10–12px control glyphs
(window controls, plus, close, chevrons). The lime never appears in an image
because there are no images.

## Provenance

- Fonts: Sono (Etcetera Type Co., SIL OFL 1.1), self-hosted woff2 subsets.
- Illustrations/raster assets: none produced.
- Visual world: user-supplied reference, "Perk" (styles.refero.design).
