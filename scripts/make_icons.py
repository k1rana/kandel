"""Generates the Kandel tray/application icons.

The mark is a **K whose stem is a candle**: a tall vertical stroke with a lime
flame sitting on top of it. "Kandel" is a pun on _candle_ (and on _kanál_,
"channel"), and the flame doubles as the app's own health signal — lime is the
colour the interface uses for a process that is alive and running.

Palette is taken from the design system (`crates/kandel-ui/frontend/src/theme.css`):

  * ground  — ink        #14140f  (the app's dark surface)
  * letter  — parchment  #f5f5eb  (the app's primary text on dark)
  * flame   — lime       #beff50  (the app's "alive" signal)

Produces:
  * crates/kandel-core/assets/icon.png              (tray icon, 64x64 RGBA)
  * crates/kandel-core/assets/icon.ico              (core exe resource, multi-size)
  * crates/kandel-ui/icons/icon.png                 (Tauri icon, 512x512)
  * crates/kandel-ui/icons/32x32.png
  * crates/kandel-ui/icons/128x128.png
  * crates/kandel-ui/icons/128x128@2x.png
  * crates/kandel-ui/icons/icon.ico                 (Windows, multi-size)
  * crates/kandel-ui/frontend/src/assets/brand-mark.svg  (in-app mark, e.g. About)

The SVG is emitted from the same geometry as the rasters, so the mark drawn in
the interface cannot drift from the application icon.

Run from the repository root:  python scripts/make_icons.py
"""

from __future__ import annotations

import math
import os

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# ── Palette (mirrors the design tokens) ──────────────────────────────────────
INK = (20, 20, 15, 255)  # #14140f
PARCHMENT = (245, 245, 235, 255)  # #f5f5eb
LIME = (190, 255, 80, 255)  # #beff50

# The master is rendered once at a high resolution and every output size is
# downsampled from it, so all sizes stay visually identical and antialiased —
# including the 16px tray and .ico entries.
MASTER = 1024

# ── Geometry, in fractions of the master square ──────────────────────────────
# Badge: a rounded square, the same soft corner the UI uses.
BADGE_INSET = 0.02
BADGE_RADIUS = 0.22

# The K. Its vertical stem doubles as the candle body.
STEM_LEFT = 0.315
STEM_RIGHT = 0.425
STEM_TOP = 0.455
STEM_BOTTOM = 0.865
STROKE = STEM_RIGHT - STEM_LEFT  # one weight for the stem and both arms

# Arms meet the stem near its middle and splay to the letter's right edge.
JUNCTION_X = STEM_RIGHT
JUNCTION_Y = 0.66
ARM_TOP = (0.760, 0.455)
ARM_BOTTOM = (0.760, 0.865)

# Flame: a teardrop whose body overhangs the stem's top, so the two shapes
# always overlap and no seam appears when the icon is downsampled. Its body is
# a little wider than the stroke (a flame reads as wider than its candle), but
# not so wide that it looks like a cone.
FLAME_TIP_Y = 0.100
FLAME_BASE_Y = 0.500  # past STEM_TOP, so the shapes intersect
FLAME_HALF_W = 0.076


INK_HEX = "#14140f"
PARCHMENT_HEX = "#f5f5eb"
LIME_HEX = "#beff50"


def brand_mark_svg(dim: int = 100) -> str:
    """Builds the mark as an SVG, using the same fractions as the raster.

    `dim` is the viewBox edge. All geometry is expressed in the 0..1 space the
    raster uses, so both outputs stay in step.
    """

    def u(v: float) -> str:
        # Percent of the viewBox, trimmed to a compact number.
        return f"{round(v * dim, 3):g}"

    mid = (STEM_LEFT + STEM_RIGHT) / 2
    flame_r = FLAME_HALF_W
    body_cy = FLAME_BASE_Y - flame_r

    # K first, then the flame over the stem's top — the same draw order the
    # raster uses, so the stem reads as the candle body in both.
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {dim} {dim}" role="img" aria-label="Kandel">
  <rect x="{u(BADGE_INSET)}" y="{u(BADGE_INSET)}" width="{u(1 - 2 * BADGE_INSET)}" height="{u(1 - 2 * BADGE_INSET)}" rx="{u(BADGE_RADIUS)}" fill="{INK_HEX}"/>
  <rect x="{u(mid - STROKE / 2)}" y="{u(STEM_TOP)}" width="{u(STROKE)}" height="{u(STEM_BOTTOM - STEM_TOP)}" fill="{PARCHMENT_HEX}"/>
  <path d="M{u(ARM_TOP[0])} {u(ARM_TOP[1])} L{u(JUNCTION_X)} {u(JUNCTION_Y)} L{u(ARM_BOTTOM[0])} {u(ARM_BOTTOM[1])}" fill="none" stroke="{PARCHMENT_HEX}" stroke-width="{u(STROKE)}"/>
  <circle cx="{u(mid)}" cy="{u(body_cy)}" r="{u(flame_r)}" fill="{LIME_HEX}"/>
  <path d="M{u(mid)} {u(FLAME_TIP_Y)} L{u(mid + flame_r)} {u(body_cy)} L{u(mid - flame_r)} {u(body_cy)} Z" fill="{LIME_HEX}"/>
</svg>
"""


def _thick_line(
    draw: ImageDraw.ImageDraw,
    p0: tuple[float, float],
    p1: tuple[float, float],
    width: float,
    fill: tuple[int, int, int, int],
) -> None:
    """Draws a line of `width` as a filled quad, so it has crisp square ends."""
    (x0, y0), (x1, y1) = p0, p1
    dx, dy = x1 - x0, y1 - y0
    length = math.hypot(dx, dy)
    if length == 0:
        return
    # Unit normal, scaled to half the stroke width.
    nx, ny = -dy / length * width / 2, dx / length * width / 2
    draw.polygon(
        [
            (x0 + nx, y0 + ny),
            (x1 + nx, y1 + ny),
            (x1 - nx, y1 - ny),
            (x0 - nx, y0 - ny),
        ],
        fill=fill,
    )


def render_master() -> Image.Image:
    """Renders the mark at MASTER resolution, in RGBA."""
    s = MASTER
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    def px(v: float) -> float:
        return v * s

    # Badge ground.
    inset = px(BADGE_INSET)
    draw.rounded_rectangle(
        [inset, inset, s - 1 - inset, s - 1 - inset],
        radius=px(BADGE_RADIUS),
        fill=INK,
    )

    # Letter K first, so the flame can sit over the top of the stem.
    mid = (STEM_LEFT + STEM_RIGHT) / 2
    stem_half = STROKE / 2
    draw.rectangle(
        [px(mid - stem_half), px(STEM_TOP), px(mid + stem_half), px(STEM_BOTTOM)],
        fill=PARCHMENT,
    )
    _thick_line(
        draw,
        (px(JUNCTION_X), px(JUNCTION_Y)),
        (px(ARM_TOP[0]), px(ARM_TOP[1])),
        px(STROKE),
        PARCHMENT,
    )
    _thick_line(
        draw,
        (px(JUNCTION_X), px(JUNCTION_Y)),
        (px(ARM_BOTTOM[0]), px(ARM_BOTTOM[1])),
        px(STROKE),
        PARCHMENT,
    )

    # Flame drawn last: a circle for the body plus a triangle for the tip,
    # unioned in one colour. Painting it over the stem makes the stem read as
    # the candle body (its top disappears into the flame) and guarantees the
    # two shapes overlap, so no seam can appear when the icon is downsampled.
    cx = px((STEM_LEFT + STEM_RIGHT) / 2)
    tip_y = px(FLAME_TIP_Y)
    base_y = px(FLAME_BASE_Y)
    r = px(FLAME_HALF_W)
    body_cy = base_y - r
    draw.ellipse([cx - r, body_cy - r, cx + r, body_cy + r], fill=LIME)
    draw.polygon([(cx, tip_y), (cx + r, body_cy), (cx - r, body_cy)], fill=LIME)

    return img


def render(master: Image.Image, size: int) -> Image.Image:
    """Downsamples the master to a square `size` icon."""
    return master.resize((size, size), Image.LANCZOS)


def main() -> None:
    core_assets = os.path.join(ROOT, "crates", "kandel-core", "assets")
    ui_icons = os.path.join(ROOT, "crates", "kandel-ui", "icons")
    ui_assets = os.path.join(ROOT, "crates", "kandel-ui", "frontend", "src", "assets")
    os.makedirs(core_assets, exist_ok=True)
    os.makedirs(ui_icons, exist_ok=True)
    os.makedirs(ui_assets, exist_ok=True)

    master = render_master()

    # Windows .ico, built from the same master so every embedded size matches.
    # Emitted once and shared by both binaries: the core embeds it as its exe
    # resource (see `crates/kandel-core/build.rs`), and Tauri bundles the UI's.
    ico_sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]

    # Tray icon (64x64 is plenty for the notification area).
    render(master, 64).save(os.path.join(core_assets, "icon.png"))

    # The core binary's own icon resource, so Explorer and the shell show the
    # Kandel mark instead of a generic executable glyph.
    master.save(os.path.join(core_assets, "icon.ico"), format="ICO", sizes=ico_sizes)

    # Tauri icon set.
    render(master, 512).save(os.path.join(ui_icons, "icon.png"))
    render(master, 32).save(os.path.join(ui_icons, "32x32.png"))
    render(master, 128).save(os.path.join(ui_icons, "128x128.png"))
    render(master, 256).save(os.path.join(ui_icons, "128x128@2x.png"))

    master.save(
        os.path.join(ui_icons, "icon.ico"),
        format="ICO",
        sizes=ico_sizes,
    )

    # The in-app mark, from the same geometry as the rasters.
    with open(os.path.join(ui_assets, "brand-mark.svg"), "w", encoding="utf-8") as f:
        f.write(brand_mark_svg())

    print("Icons written to", core_assets, "and", ui_icons)
    print("Mark written to", ui_assets)


if __name__ == "__main__":
    main()
