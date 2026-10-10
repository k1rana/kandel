---
version: 1
slug: "crates-kandel-ui-frontend-src-app-svelte"
primary_target: "crates/kandel-ui/frontend/src/App.svelte"
related_targets: []
---

## Mode

Operate — the visitor completes a task (add, edit, pause, restart a supervised
app). Working screen first; the world serves it.

## Audience, job, action

Windows users managing background apps kept alive in their interactive session.
Job: see what is running and fix it fast. Action: add/edit an entry, or
enable/disable/restart one. Frequency: occasional, in short sessions.
Constraint: no marketing claims; no real user data exists.

## Direction contract

**THESIS** — The surface is an editorial status ledger: warm paper, one
electric-lime voice, and a printed ledger of managed apps. It refuses the
category default: a dark neon dashboard of identical glossy cards.

**OWN-WORLD** — Perk: parchment `#f5f5eb` ground, pure-white surfaces, off-black
ink `#14140f`, ash rules `#d2d2c8`, graphite secondary text. Electric lime
`#beff50` is the only filled action and the primary health signal; a degraded
and a failed state add amber (`#96581a`) and red (`#b83c28`) strictly as
meaning-bearing state colour, as Operate allows. Type is Sono, weights 400/500
only, `-0.03em` tracking at 28px+. Soft radii (cards/buttons 28px, inputs 8px,
pills 9999px). No shadows anywhere — tonal contrast separates layers.

**STORY** — The visitor understands at a glance which apps are alive, believes
the ledger is complete and current, and edits one entry without leaving it.

**FIRST VIEWPORT** — Parchment shell. A white top bar: "Kandel" wordmark at
left, live tally in micro-caps, and the lime "Add app" pill at right. Below, a
single white ledger (28px radius) with an ash-ruled header (`APP`, `STATE`,
`PID`, `RESTARTS`) and one row per app. Each row leads with a state charge — a
small filled rounded square, lime when running — so lime tracks down the page
as the health signal. Whole-window drop target for registering executables.

**FORM** — Ledger list (refuses a card grid and a hero-metric band). Position 1
of the considered structures. Pinned brief; code-led, seed key none.

**FINISH** — unreviewed and undocumented is unfinished; this build ends with the
finish review, the verdict, DESIGN.md, and every shipping raster carrying its
provenance.
