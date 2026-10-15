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
  * crates/kandel-ui/icons/installer.ico            (the mark + a "down" badge)
  * crates/kandel-ui/frontend/src/assets/brand-mark.svg  (in-app mark, e.g. About)
  * crates/kandel-ui/windows/header.bmp             (installer header, 150x57)
  * crates/kandel-ui/windows/sidebar.bmp            (installer sidebar, 164x314)

The SVG is emitted from the same geometry as the rasters, so the mark drawn in
the interface cannot drift from the application icon.

The installer icon is a second variant of the same mark: the plain app icon
carries an extra bottom-right badge (a lime disc holding a down arrow) so the
setup executable is distinguishable from the installed application at a glance.
It reuses the identical geometry, so the two can never drift either.

The installer's header and sidebar bitmaps are drawn here too, from the same
mark and the same design tokens, so the wizard wears the app's own palette
(canvas white, ink, parchment) instead of NSIS's grey defaults.

Run from the repository root:  python scripts/make_icons.py
"""

from __future__ import annotations

import math
import os

from PIL import Image, ImageDraw, ImageFont

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

# ── Installer badge geometry ─────────────────────────────────────────────────
# The installer variant adds a bottom-right badge: a lime disc (the same
# "alive" colour as the flame, so it belongs to the palette) carrying a dark
# down arrow. It reads as "this runs an installer", while the plain mark stays on
# the installed app (tray, exe, in-app).
#
# The disc is kept fully inside the badge's rounded corner: for the corner arc
# centred at (1 - BADGE_RADIUS) with radius BADGE_RADIUS, a disc of radius R at
# (c, c) fits while sqrt(2) * (c - (1 - BADGE_RADIUS)) + R <= BADGE_RADIUS.
DISC_CX = 0.780
DISC_CY = 0.780
DISC_R = 0.175

# Down arrow, centred in the disc. The shaft meets the head, which widens to
# ARROW_HALF_W; both are drawn in ink so they read against the lime.
ARROW_SHAFT_HALF_W = 0.036
ARROW_SHAFT_TOP = 0.680
ARROW_HEAD_TOP = 0.795
ARROW_HALF_W = 0.082
ARROW_TIP_Y = 0.885


INK_HEX = "#14140f"
PARCHMENT_HEX = "#f5f5eb"
LIME_HEX = "#beff50"

# Header/sidebar art. The wizard is repainted from these same tokens (see
# `crates/kandel-ui/windows/hooks.nsh`); keeping the pixels and the NSIS colour
# defines in one palette is what makes the installer look like the app.
CANVAS = (255, 255, 255)  # #ffffff — the page ground
GRAPHITE = (110, 110, 100)  # #6e6e64 — secondary, muted copy

# Sono, self-hosted by the frontend; the same family the UI uses, so the
# wordmark on the wizard matches the one in the app.
SONO = os.path.join(
    ROOT, "crates", "kandel-ui", "frontend", "src", "assets", "fonts", "sono-latin-var.woff2"
)

# Bitmap sizes MUI2 expects, and the wizard pages are laid out for.
HEADER_SIZE = (150, 57)
SIDEBAR_SIZE = (164, 314)


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


def render_installer_master(master: Image.Image) -> Image.Image:
    """The mark with the installer badge: a lime disc and a down arrow.

    Drawn from the same master, so the badge sits on the identical mark the app
    uses and the two outputs cannot drift.
    """
    img = master.copy()
    draw = ImageDraw.Draw(img)
    s = MASTER

    def px(v: float) -> float:
        return v * s

    # Lime disc over the ink ground.
    cx, cy, r = px(DISC_CX), px(DISC_CY), px(DISC_R)
    draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=LIME)

    # Down arrow in ink: a shaft rectangle joined to a triangle head. The shaft
    # runs into the head so the two shapes overlap and no seam can appear.
    shaft_hw = px(ARROW_SHAFT_HALF_W)
    draw.rectangle(
        [cx - shaft_hw, px(ARROW_SHAFT_TOP), cx + shaft_hw, px(ARROW_HEAD_TOP)],
        fill=INK,
    )
    head_hw = px(ARROW_HALF_W)
    draw.polygon(
        [
            (cx - head_hw, px(ARROW_HEAD_TOP)),
            (cx + head_hw, px(ARROW_HEAD_TOP)),
            (cx, px(ARROW_TIP_Y)),
        ],
        fill=INK,
    )

    return img


def render(master: Image.Image, size: int) -> Image.Image:
    """Downsamples the master to a square `size` icon."""
    return master.resize((size, size), Image.LANCZOS)


def _sono(size: int, weight: int) -> ImageFont.FreeTypeFont:
    """Loads the self-hosted Sono at `weight` (only the Weight axis varies)."""
    font = ImageFont.truetype(SONO, size)
    font.set_variation_by_axes([weight])
    return font


def render_header(master: Image.Image) -> Image.Image:
    """The 150x57 header strip: the mark and the wordmark, on parchment.

    Sits on the wizard's header, whose background the NSIS hooks paint the same
    parchment — so the bitmap blends into the strip instead of showing a seam.
    """
    w, h = HEADER_SIZE
    img = Image.new("RGB", (w, h), PARCHMENT)
    draw = ImageDraw.Draw(img)

    mark_size = 30
    mark = render(master, mark_size)
    wordmark = _sono(16, 500)
    text = "Kandel"
    text_w = draw.textlength(text, font=wordmark)

    gap = 7
    group_w = mark_size + gap + text_w
    x = (w - group_w) / 2

    img.paste(mark, (round(x), (h - mark_size) // 2), mark)
    # Vertically centred on the mark, using the glyph box rather than the em box.
    ty = (h - (wordmark.getbbox(text)[3] - wordmark.getbbox(text)[1])) / 2 - wordmark.getbbox(text)[1]
    draw.text((x + mark_size + gap, ty), text, font=wordmark, fill=INK)

    return img


def render_sidebar(master: Image.Image) -> Image.Image:
    """The 164x314 Welcome/Finish sidebar: a dark island carrying the mark.

    The wizard's own background is parchment (set in the NSIS hooks), so this
    reads as the one dark surface in an otherwise light wizard — the same
    tonal, shadowless stack the app uses.
    """
    w, h = SIDEBAR_SIZE
    img = Image.new("RGB", (w, h), INK)
    draw = ImageDraw.Draw(img)

    mark_size = 84
    mark = render(master, mark_size)
    img.paste(mark, ((w - mark_size) // 2, 76), mark)

    cx = w / 2

    wordmark = _sono(24, 500)
    text = "Kandel"
    tw = draw.textlength(text, font=wordmark)
    box = wordmark.getbbox(text)
    draw.text((cx - tw / 2, 188 - box[1]), text, font=wordmark, fill=PARCHMENT)

    # A lime rule: the brand's one colour, held to a single short stroke.
    draw.rectangle([cx - 22, 232, cx + 22, 234], fill=LIME)

    eyebrow = _sono(9, 500)
    cap = "PROCESS SUPERVISION"
    # Tracking (0.1em) drawn by hand: PIL has no letter-spacing.
    spacing = 0.9
    widths = [draw.textlength(c, font=eyebrow) for c in cap]
    total = sum(widths) + spacing * (len(cap) - 1)
    x = cx - total / 2
    for c, cw in zip(cap, widths):
        draw.text((x, 250 - eyebrow.getbbox(cap)[1]), c, font=eyebrow, fill=GRAPHITE)
        x += cw + spacing

    return img


def main() -> None:
    core_assets = os.path.join(ROOT, "crates", "kandel-core", "assets")
    ui_icons = os.path.join(ROOT, "crates", "kandel-ui", "icons")
    ui_assets = os.path.join(ROOT, "crates", "kandel-ui", "frontend", "src", "assets")
    ui_windows = os.path.join(ROOT, "crates", "kandel-ui", "windows")
    os.makedirs(core_assets, exist_ok=True)
    os.makedirs(ui_icons, exist_ok=True)
    os.makedirs(ui_assets, exist_ok=True)
    os.makedirs(ui_windows, exist_ok=True)

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

    # The installer's own icon: the same mark with the installer badge. Kept
    # separate from `icon.ico` so the installed app keeps the plain mark.
    render_installer_master(master).save(
        os.path.join(ui_icons, "installer.ico"),
        format="ICO",
        sizes=ico_sizes,
    )

    # The in-app mark, from the same geometry as the rasters.
    with open(os.path.join(ui_assets, "brand-mark.svg"), "w", encoding="utf-8") as f:
        f.write(brand_mark_svg())

    # Installer wizard art. NSIS wants 24-bit BMPs (no alpha), so the RGBA mark
    # is composited onto the strip colour and down-converted here.
    render_header(master).save(os.path.join(ui_windows, "header.bmp"))
    render_sidebar(master).save(os.path.join(ui_windows, "sidebar.bmp"))

    print("Icons written to", core_assets, "and", ui_icons)
    print("Header/sidebar written to", ui_windows)
    print("Mark written to", ui_assets)


if __name__ == "__main__":
    main()
