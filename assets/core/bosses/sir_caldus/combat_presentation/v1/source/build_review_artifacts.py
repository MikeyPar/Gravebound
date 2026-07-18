#!/usr/bin/env python3
"""Deterministically build the unregistered M03 B6 Caldus combat-review pack."""

from __future__ import annotations

import argparse
import hashlib
import math
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFont


TILE = 32
ARENA_TILES = 18
ARENA_SIZE = ARENA_TILES * TILE
ARENA_CENTER = (9 * TILE, 9 * TILE)
ARENA_RADIUS = 8 * TILE
PACK_RELATIVE = Path("assets/core/bosses/sir_caldus/combat_presentation/v1")

INK = (8, 10, 13, 255)
VOID = (11, 15, 18, 255)
WALL = (25, 28, 31, 255)
WALL_LIGHT = (56, 56, 54, 255)
STONE = (43, 44, 43, 255)
STONE_LIGHT = (60, 59, 55, 255)
STONE_DARK = (29, 31, 32, 255)
WET = (30, 42, 44, 255)
BRASS = (139, 103, 54, 255)
BRASS_LIGHT = (198, 158, 86, 255)
BONE = (236, 226, 203, 255)
RED = (136, 38, 35, 255)
RED_LIGHT = (221, 83, 65, 255)
VIOLET = (126, 75, 155, 255)
VIOLET_LIGHT = (201, 149, 226, 255)
GAP = (91, 215, 199, 255)
PLAYER = (166, 209, 198, 255)


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    path = Path("C:/Windows/Fonts/seguisb.ttf" if bold else "C:/Windows/Fonts/segoeui.ttf")
    if path.exists():
        return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def point(angle_degrees: float, radius: float, center: tuple[float, float] = ARENA_CENTER) -> tuple[int, int]:
    angle = math.radians(angle_degrees)
    return round(center[0] + math.cos(angle) * radius), round(center[1] + math.sin(angle) * radius)


def build_arena() -> Image.Image:
    """Render the exact 18x18 Caldus Bell Court shell at 32 pixels per tile."""
    image = Image.new("RGBA", (ARENA_SIZE, ARENA_SIZE), VOID)
    draw = ImageDraw.Draw(image)

    # Solid shell and exact walkable circle: center (9,9), radius 8 tiles.
    draw.ellipse((16, 16, ARENA_SIZE - 17, ARENA_SIZE - 17), fill=WALL, outline=INK, width=2)
    draw.ellipse(
        (
            ARENA_CENTER[0] - ARENA_RADIUS,
            ARENA_CENTER[1] - ARENA_RADIUS,
            ARENA_CENTER[0] + ARENA_RADIUS,
            ARENA_CENTER[1] + ARENA_RADIUS,
        ),
        fill=STONE,
        outline=WALL_LIGHT,
        width=3,
    )

    # Western three-tile door centered at (0,9), joined to the circular court.
    draw.rectangle((0, 7.5 * TILE, 2 * TILE, 10.5 * TILE), fill=STONE)
    draw.line((0, round(7.5 * TILE), 2 * TILE, round(7.5 * TILE)), fill=WALL_LIGHT, width=3)
    draw.line((0, round(10.5 * TILE), 2 * TILE, round(10.5 * TILE)), fill=WALL_LIGHT, width=3)
    for x in range(4, 2 * TILE, 16):
        draw.line((x, round(7.5 * TILE) + 5, x, round(10.5 * TILE) - 5), fill=(76, 67, 54, 255), width=2)

    # Muted radial flagstone wedges and wet seams remain well below hostile contrast.
    for radius in range(2 * TILE, ARENA_RADIUS, 2 * TILE):
        draw.ellipse(
            (
                ARENA_CENTER[0] - radius,
                ARENA_CENTER[1] - radius,
                ARENA_CENTER[0] + radius,
                ARENA_CENTER[1] + radius,
            ),
            outline=STONE_DARK,
            width=2,
        )
    for angle in range(0, 360, 30):
        inner = point(angle, 42)
        outer = point(angle, ARENA_RADIUS - 5)
        draw.line((*inner, *outer), fill=STONE_DARK, width=2)
    for angle in range(15, 360, 60):
        for radius in range(56, ARENA_RADIUS - 16, 64):
            x, y = point(angle, radius)
            draw.line((x - 6, y + 2, x + 5, y - 2), fill=WET, width=2)

    # Four understated endpoint sockets; these are navigation landmarks, not active cues.
    for x, y in ((1 * TILE, 9 * TILE), (17 * TILE, 9 * TILE), (9 * TILE, 1 * TILE), (9 * TILE, 17 * TILE)):
        draw.rectangle((x - 9, y - 9, x + 9, y + 9), fill=(35, 35, 34, 255), outline=(91, 73, 51, 255), width=2)
        draw.line((x - 5, y, x + 5, y), fill=BRASS, width=2)
        draw.line((x, y - 5, x, y + 5), fill=BRASS, width=2)

    # Boss-origin dais and west staging threshold are deliberately muted.
    cx, cy = ARENA_CENTER
    draw.ellipse((cx - 25, cy - 25, cx + 25, cy + 25), fill=(36, 36, 34, 255), outline=(88, 71, 48, 255), width=2)
    for angle in range(0, 360, 45):
        x0, y0 = point(angle, 17)
        x1, y1 = point(angle, 22)
        draw.line((x0, y0, x1, y1), fill=BRASS, width=2)
    stage_x, stage_y = round(2.5 * TILE), 9 * TILE
    draw.line((stage_x - 7, stage_y - 18, stage_x - 7, stage_y + 18), fill=(92, 74, 52, 255), width=2)
    draw.line((stage_x + 7, stage_y - 18, stage_x + 7, stage_y + 18), fill=(92, 74, 52, 255), width=2)

    draw.rectangle((0, 0, ARENA_SIZE - 1, ARENA_SIZE - 1), outline=INK, width=1)
    return image


def telegraph_material(veil: bool, reduced: bool) -> Image.Image:
    """Transparent repeating cue material; the simulation owns every geometry value."""
    image = Image.new("RGBA", (32, 32), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    edge = VIOLET_LIGHT if veil else RED_LIGHT
    body = VIOLET if veil else RED
    if not reduced:
        draw.rectangle((0, 2, 31, 29), fill=(*body[:3], 48))
    draw.rectangle((0, 5, 31, 26), fill=(*body[:3], 142))
    draw.line((0, 5, 31, 5), fill=edge, width=3)
    draw.line((0, 26, 31, 26), fill=edge, width=3)
    draw.line((0, 15, 31, 15), fill=BONE, width=3 if reduced else 2)
    # Pointed chevrons carry Physical/Veil threat in grayscale and without motion.
    for x in (3, 16, 29):
        draw.line((x - 4, 10, x + 1, 15), fill=BONE, width=2)
        draw.line((x + 1, 15, x - 4, 20), fill=BONE, width=2)
    return image


def arena_mask() -> Image.Image:
    mask = Image.new("L", (ARENA_SIZE, ARENA_SIZE), 0)
    draw = ImageDraw.Draw(mask)
    draw.ellipse(
        (
            ARENA_CENTER[0] - ARENA_RADIUS,
            ARENA_CENTER[1] - ARENA_RADIUS,
            ARENA_CENTER[0] + ARENA_RADIUS,
            ARENA_CENTER[1] + ARENA_RADIUS,
        ),
        fill=255,
    )
    draw.rectangle((0, round(7.5 * TILE), 2 * TILE, round(10.5 * TILE)), fill=255)
    return mask


def clip_to_arena(image: Image.Image) -> Image.Image:
    alpha = image.getchannel("A")
    alpha = ImageChops.multiply(alpha, arena_mask())
    image.putalpha(alpha)
    return image


def hostile_line(
    draw: ImageDraw.ImageDraw,
    start: tuple[int, int],
    end: tuple[int, int],
    edge: tuple[int, int, int, int],
    reduced: bool,
    severe: bool = False,
) -> None:
    outer = 15 if severe else 11
    draw.line((*start, *end), fill=edge, width=outer)
    draw.line((*start, *end), fill=BONE, width=4 if severe else 3)


def gap_bracket(draw: ImageDraw.ImageDraw, center: tuple[int, int], angle: float, radius: int, span: float, reduced: bool) -> None:
    # Two boundary bars and inward chevrons make the absence readable without color.
    for boundary in (angle - span / 2, angle + span / 2):
        inner = point(boundary, radius - 20, center)
        outer = point(boundary, radius + 9, center)
        draw.line((*inner, *outer), fill=GAP, width=5)
        tip = point(boundary + (-4 if boundary < angle else 4), radius - 8, center)
        draw.line((*outer, *tip), fill=BONE, width=3)


def shield_arc(reduced: bool) -> Image.Image:
    image = Image.new("RGBA", (ARENA_SIZE, ARENA_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    # Review orientation is east; the runtime rotates to the server-owned target lock.
    angles = (-30, -15, 0, 15, 30)
    if not reduced:
        sector = [ARENA_CENTER] + [point(angle, 238) for angle in range(-30, 31, 3)]
        draw.polygon(sector, fill=(139, 38, 35, 38))
    for angle in angles:
        hostile_line(draw, point(angle, 28), point(angle, 238), RED_LIGHT, reduced)
        # Five pointed projectile-origin marks are visible at minimum scale.
        px, py = point(angle, 64)
        front = point(angle, 73)
        left = point(angle - 90, 5, (px, py))
        right = point(angle + 90, 5, (px, py))
        draw.polygon((front, left, right), fill=BONE, outline=RED)
    draw.arc((ARENA_CENTER[0] - 238, ARENA_CENTER[1] - 238, ARENA_CENTER[0] + 238, ARENA_CENTER[1] + 238), -30, 30, fill=BONE, width=3)
    return clip_to_arena(image)


def radial_ring(index_count: int, omitted: set[int], veil: bool, reduced: bool, origin: tuple[int, int], radius: int) -> Image.Image:
    image = Image.new("RGBA", (ARENA_SIZE, ARENA_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    edge = VIOLET_LIGHT if veil else RED_LIGHT
    if not reduced:
        draw.ellipse((origin[0] - 48, origin[1] - 48, origin[0] + 48, origin[1] + 48), outline=(*edge[:3], 88), width=9)
    draw.ellipse((origin[0] - 34, origin[1] - 34, origin[0] + 34, origin[1] + 34), outline=edge, width=4)
    step = 360.0 / index_count
    for index in range(index_count):
        if index in omitted:
            continue
        angle = index * step
        hostile_line(draw, point(angle, 48, origin), point(angle, radius, origin), edge, reduced)
        px, py = point(angle, 82, origin)
        front = point(angle, 91, origin)
        left = point(angle - 90, 5, (px, py))
        right = point(angle + 90, 5, (px, py))
        draw.polygon((front, left, right), fill=BONE, outline=VIOLET if veil else RED)
    return clip_to_arena(image)


def bell_ring(reduced: bool, gap_start: int = 0) -> Image.Image:
    omitted = {(gap_start + offset) % 18 for offset in range(3)}
    image = radial_ring(18, omitted, True, reduced, ARENA_CENTER, 244)
    draw = ImageDraw.Draw(image)
    step = 20.0
    gap_center = (gap_start + 1) * step
    gap_bracket(draw, ARENA_CENTER, gap_center, 178, 56, reduced)
    return image


def charge_lane(reduced: bool) -> Image.Image:
    image = Image.new("RGBA", (ARENA_SIZE, ARENA_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    origin = ARENA_CENTER
    endpoint = (ARENA_CENTER[0] + round(6.5 * TILE), ARENA_CENTER[1])
    half_width = round(1.2 * TILE / 2)
    if not reduced:
        draw.rectangle((origin[0], origin[1] - half_width - 7, endpoint[0], origin[1] + half_width + 7), fill=(136, 38, 35, 55))
    draw.rectangle((origin[0], origin[1] - half_width, endpoint[0], origin[1] + half_width), fill=(136, 38, 35, 144), outline=RED_LIGHT, width=4)
    draw.line((origin[0], origin[1], endpoint[0], origin[1]), fill=BONE, width=5)
    for x in range(origin[0] + 28, endpoint[0] - 10, 36):
        draw.line((x - 9, origin[1] - 9, x, origin[1]), fill=BONE, width=3)
        draw.line((x - 9, origin[1] + 9, x, origin[1]), fill=BONE, width=3)
    # Parent warning owns the child Stop Ring opposite-gap marker.
    gap_bracket(draw, endpoint, 180, 54, 58, reduced)
    return clip_to_arena(image)


def stop_ring(reduced: bool) -> Image.Image:
    endpoint = (ARENA_CENTER[0] + round(6.5 * TILE), ARENA_CENTER[1])
    # Approved SPEC-CONFLICT-022: choose the adjacent pair whose midpoint is nearest west.
    step = 360.0 / 14.0
    candidates = [(index, ((index + 0.5) * step) % 360.0) for index in range(14)]
    start = min(candidates, key=lambda pair: (abs(((pair[1] - 180 + 180) % 360) - 180), pair[0]))[0]
    omitted = {start, (start + 1) % 14}
    image = radial_ring(14, omitted, False, reduced, endpoint, 150)
    draw = ImageDraw.Draw(image)
    gap_center = ((start + 0.5) * step) % 360.0
    gap_bracket(draw, endpoint, gap_center, 104, step * 2.15, reduced)
    return image


def phase_three_preview(reduced: bool, gap_start: int, ordinal: int) -> Image.Image:
    image = Image.new("RGBA", (ARENA_SIZE, ARENA_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    step = 20.0
    center_angle = (gap_start + 1) * step
    # Only the reserved safe gap is previewed; no implicit 800 ms hostile Ring telegraph.
    if not reduced:
        polygon = [ARENA_CENTER, point(center_angle - 28, 224), point(center_angle + 28, 224)]
        draw.polygon(polygon, fill=(91, 215, 199, 34))
    for radius in (112, 154, 196):
        gap_bracket(draw, ARENA_CENTER, center_angle, radius, 56, reduced)
    # 1/2/3 radial notches encode preview order without text or color dependency.
    for index in range(ordinal):
        angle = center_angle - 6 + index * 6
        p0 = point(angle, 70)
        p1 = point(angle, 89)
        draw.line((*p0, *p1), fill=BONE, width=4)
    return clip_to_arena(image)


def boss_marker(image: Image.Image, at: tuple[int, int]) -> None:
    """Review-only origin marker; no duplicate Caldus sprite asset is introduced."""
    draw = ImageDraw.Draw(image)
    x, y = at
    draw.ellipse((x - 17, y - 17, x + 17, y + 17), fill=(18, 19, 20, 255), outline=BRASS_LIGHT, width=3)
    draw.polygon(((x, y - 12), (x - 9, y + 9), (x + 9, y + 9)), fill=(43, 45, 46, 255), outline=BONE)
    draw.line((x - 9, y - 3, x + 9, y - 3), fill=BRASS, width=3)


def player_marker(image: Image.Image, at: tuple[int, int]) -> None:
    draw = ImageDraw.Draw(image)
    x, y = at
    draw.polygon(((x, y - 12), (x - 9, y + 11), (x + 9, y + 11)), fill=PLAYER, outline=BONE)
    draw.line((x, y - 8, x + 13, y - 15), fill=BRASS_LIGHT, width=3)


def scene(overlay: Image.Image, boss_at: tuple[int, int] = ARENA_CENTER) -> Image.Image:
    image = build_arena()
    image.alpha_composite(overlay)
    boss_marker(image, boss_at)
    player_marker(image, (round(3.9 * TILE), round(11.2 * TILE)))
    return image


def centered_text(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], value: str, face: ImageFont.ImageFont, fill: tuple[int, int, int, int]) -> None:
    bounds = draw.textbbox((0, 0), value, font=face)
    x = box[0] + (box[2] - box[0] - (bounds[2] - bounds[0])) // 2
    y = box[1] + (box[3] - box[1] - (bounds[3] - bounds[1])) // 2
    draw.text((x, y), value, font=face, fill=fill)


def fit(image: Image.Image, width: int, height: int) -> Image.Image:
    scale = min(width / image.width, height / image.height)
    size = (max(1, round(image.width * scale)), max(1, round(image.height * scale)))
    return image.resize(size, Image.Resampling.NEAREST)


def review_mock(out: Path, width: int, height: int, reduced: bool) -> None:
    canvas = Image.new("RGBA", (width, height), (11, 13, 16, 255))
    draw = ImageDraw.Draw(canvas)
    margin = max(18, width // 90)
    header = max(78, height // 8)
    footer = max(76, height // 9)
    gap = max(14, width // 100)
    body_top = margin + header
    body_bottom = height - margin - footer
    card_width = (width - margin * 2 - gap * 2) // 3

    draw.rectangle((margin, margin, width - margin, margin + header - 8), fill=(18, 20, 23, 255), outline=BRASS, width=2)
    centered_text(draw, (margin, margin + 4, width - margin, margin + header // 2), "B6 / CALDUS'S BELL COURT / COMBAT READABILITY", font(max(19, height // 35), True), BONE)
    mode = "REDUCED EFFECTS / IDENTICAL CORE GEOMETRY" if reduced else "STANDARD EFFECTS / RESERVED HOSTILE CONTRAST"
    centered_text(draw, (margin, margin + header // 2 - 2, width - margin, margin + header - 8), mode, font(max(14, height // 54), True), GAP)

    cards = [
        ("PHASE I / LEARN", "SHIELD ARC / 650 MS / FIVE OVER 60 DEGREES", shield_arc(reduced), ARENA_CENTER),
        ("PHASE II / LEAVE", "CHARGE / 1.2-TILE WIDTH / LOCK +700 MS / 6.5 TILES", charge_lane(reduced), ARENA_CENTER),
        ("PHASE III / REMEMBER", "GAP A-B-C / 600 MS EACH / NO IMPLICIT RING WARN", phase_three_preview(reduced, 10, 3), ARENA_CENTER),
    ]
    for index, (title, subtitle, overlay, boss_at) in enumerate(cards):
        x0 = margin + index * (card_width + gap)
        x1 = x0 + card_width
        draw.rectangle((x0, body_top, x1, body_bottom), fill=(18, 20, 23, 255), outline=(70, 66, 58, 255), width=2)
        centered_text(draw, (x0, body_top + 3, x1, body_top + 35), title, font(max(13, height // 58), True), BONE)
        rendered = fit(scene(overlay, boss_at), card_width - 18, body_bottom - body_top - 86)
        canvas.alpha_composite(rendered, (x0 + (card_width - rendered.width) // 2, body_top + 39))
        centered_text(draw, (x0 + 4, body_bottom - 39, x1 - 4, body_bottom - 4), subtitle, font(max(10, height // 78), True), RED_LIGHT if index < 2 else GAP)

    footer_top = height - margin - footer + 7
    draw.rectangle((margin, footer_top, width - margin, height - margin), fill=(17, 19, 22, 255), outline=BRASS, width=2)
    centered_text(draw, (margin + 6, footer_top + 3, width - margin - 6, footer_top + footer // 2), "SERVER OWNS TARGET LOCK, INDEX/GAP, WIDTH, ENDPOINT, TICKS, COLLISION, DAMAGE, AND PHASE", font(max(12, height // 61), True), BONE)
    centered_text(draw, (margin + 6, footer_top + footer // 2 - 2, width - margin - 6, height - margin - 2), "STATIC UNREGISTERED REVIEW MOCK / NOT NATIVE CAPTURE / ART CANNOT START OR RESOLVE B6", font(max(11, height // 68), True), RED_LIGHT)
    canvas.convert("RGB").save(out, optimize=True)


def contact_sheet(overlays: list[Image.Image], out: Path) -> None:
    sheet = Image.new("RGBA", (ARENA_SIZE * 3, ARENA_SIZE * 2), (0, 0, 0, 0))
    for index, overlay in enumerate(overlays):
        sheet.alpha_composite(scene(overlay), ((index % 3) * ARENA_SIZE, (index // 3) * ARENA_SIZE))
    sheet.save(out, optimize=True)


def minimum_scale_sheet(overlays: list[Image.Image], out: Path) -> None:
    size = ARENA_SIZE // 2
    sheet = Image.new("RGB", (size * 3, size * 2), (12, 14, 17))
    for index, overlay in enumerate(overlays):
        rendered = scene(overlay).resize((size, size), Image.Resampling.NEAREST).convert("RGB")
        sheet.paste(rendered, ((index % 3) * size, (index // 3) * size))
    sheet.save(out, optimize=True)


def grayscale_sheet(overlays: list[Image.Image], out: Path) -> None:
    size = ARENA_SIZE // 2
    sheet = Image.new("L", (size * 3, size * 2), 0)
    for index, overlay in enumerate(overlays):
        rendered = scene(overlay).convert("RGB").convert("L").resize((size, size), Image.Resampling.NEAREST)
        sheet.paste(rendered, ((index % 3) * size, (index // 3) * size))
    sheet.save(out, optimize=True)


def write_hashes(pack: Path) -> None:
    paths = sorted(
        path
        for path in pack.rglob("*")
        if path.is_file() and path.name != "SHA256SUMS.txt" and "__pycache__" not in path.parts
    )
    lines = [f"{sha256(path)}  {path.relative_to(pack).as_posix()}" for path in paths]
    (pack / "SHA256SUMS.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def build(root: Path) -> None:
    pack = root / PACK_RELATIVE
    for relative in ("runtime", "frames/telegraphs", "previews"):
        (pack / relative).mkdir(parents=True, exist_ok=True)

    build_arena().save(pack / "runtime/caldus-bell-court.576x576.png", optimize=True)
    for veil in (False, True):
        family = "veil-major" if veil else "physical-major"
        for reduced in (False, True):
            mode = "reduced" if reduced else "standard"
            telegraph_material(veil, reduced).save(pack / f"runtime/telegraph-{family}.{mode}.32.png", optimize=True)

    frame_specs = [
        ("01-shield-arc.standard.png", shield_arc(False)),
        ("02-shield-arc.reduced.png", shield_arc(True)),
        ("03-bell-ring.standard.png", bell_ring(False)),
        ("04-bell-ring.reduced.png", bell_ring(True)),
        ("05-charge-lane.standard.png", charge_lane(False)),
        ("06-charge-lane.reduced.png", charge_lane(True)),
        ("07-charge-stop-ring.standard.png", stop_ring(False)),
        ("08-charge-stop-ring.reduced.png", stop_ring(True)),
    ]
    preview_starts = (0, 5, 10)
    for ordinal, start in enumerate(preview_starts, start=1):
        frame_specs.append((f"{8 + ordinal:02d}-phase3-preview-{chr(64 + ordinal).lower()}.standard.png", phase_three_preview(False, start, ordinal)))
    for ordinal, start in enumerate(preview_starts, start=1):
        frame_specs.append((f"{11 + ordinal:02d}-phase3-preview-{chr(64 + ordinal).lower()}.reduced.png", phase_three_preview(True, start, ordinal)))
    for name, image in frame_specs:
        image.save(pack / "frames/telegraphs" / name, optimize=True)

    representative_standard = [shield_arc(False), bell_ring(False), charge_lane(False), stop_ring(False), phase_three_preview(False, 0, 1), phase_three_preview(False, 10, 3)]
    representative_reduced = [shield_arc(True), bell_ring(True), charge_lane(True), stop_ring(True), phase_three_preview(True, 0, 1), phase_three_preview(True, 10, 3)]
    contact_sheet(representative_standard, pack / "previews/caldus-telegraphs.arena-scale.png")
    minimum_scale_sheet(representative_standard, pack / "previews/caldus-combat.standard.50pct.png")
    minimum_scale_sheet(representative_reduced, pack / "previews/caldus-combat.reduced.50pct.png")
    grayscale_sheet(representative_reduced, pack / "previews/caldus-combat.reduced.50pct.grayscale.png")
    for width, height in ((1280, 720), (1920, 1080)):
        review_mock(pack / f"previews/caldus-combat.standard.{width}x{height}.review-mock.png", width, height, False)
        review_mock(pack / f"previews/caldus-combat.reduced.{width}x{height}.review-mock.png", width, height, True)
    write_hashes(pack)


def core_geometry(image: Image.Image) -> Image.Image:
    return image.getchannel("A").point(lambda value: 255 if value >= 128 else 0, mode="1")


def verify(root: Path) -> None:
    pack = root / PACK_RELATIVE
    expected: dict[str, str] = {}
    for line in (pack / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines():
        digest, relative = line.split("  ", 1)
        expected[relative] = digest
    for relative, digest in expected.items():
        actual = sha256(pack / relative)
        if actual != digest:
            raise SystemExit(f"hash mismatch: {relative}: expected {digest}, got {actual}")

    with Image.open(pack / "runtime/caldus-bell-court.576x576.png") as arena:
        if arena.mode != "RGBA" or arena.size != (ARENA_SIZE, ARENA_SIZE):
            raise SystemExit("arena mode or dimensions differ")

    for path in sorted((pack / "runtime").glob("telegraph-*.png")):
        with Image.open(path) as material:
            if material.mode != "RGBA" or material.size != (32, 32) or material.getchannel("A").getbbox() is None:
                raise SystemExit(f"invalid material: {path}")
            luma = material.convert("RGB").convert("L")
            values = [
                value
                for value, alpha in zip(
                    luma.get_flattened_data(), material.getchannel("A").get_flattened_data()
                )
                if alpha >= 128
            ]
            if not values or max(values) < 200 or min(values) > 100:
                raise SystemExit(f"material lost grayscale core/edge contrast: {path}")

    frames = {path.name: Image.open(path).convert("RGBA") for path in sorted((pack / "frames/telegraphs").glob("*.png"))}
    try:
        if len(frames) != 14:
            raise SystemExit(f"expected 14 telegraph frames, found {len(frames)}")
        for name, image in frames.items():
            if image.size != (ARENA_SIZE, ARENA_SIZE) or image.getchannel("A").getbbox() is None:
                raise SystemExit(f"invalid telegraph frame: {name}")
            if image.getpixel((0, 0))[3] != 0:
                raise SystemExit(f"telegraph frame corner is not transparent: {name}")
        pairs = (("01", "02"), ("03", "04"), ("05", "06"), ("07", "08"), ("09", "12"), ("10", "13"), ("11", "14"))
        pair_difference_counts = []
        for standard_prefix, reduced_prefix in pairs:
            standard = next(image for name, image in frames.items() if name.startswith(standard_prefix + "-"))
            reduced = next(image for name, image in frames.items() if name.startswith(reduced_prefix + "-"))
            if ImageChops.difference(core_geometry(standard), core_geometry(reduced)).getbbox() is not None:
                raise SystemExit(f"standard/reduced core geometry differs: {standard_prefix}/{reduced_prefix}")
            full_difference = ImageChops.difference(standard, reduced)
            difference_count = sum(
                1 for pixel in full_difference.get_flattened_data() if pixel != (0, 0, 0, 0)
            )
            if difference_count == 0:
                raise SystemExit(f"standard/reduced presentation is not visually distinct: {standard_prefix}/{reduced_prefix}")
            pair_difference_counts.append(difference_count)

        representative_prefixes = ("01", "03", "05", "07", "09", "10", "11")
        representative_masks = [
            (
                prefix,
                core_geometry(next(image for name, image in frames.items() if name.startswith(prefix + "-"))),
            )
            for prefix in representative_prefixes
        ]
        shape_difference_counts = []
        for left_index, (left_name, left_mask) in enumerate(representative_masks):
            for right_name, right_mask in representative_masks[left_index + 1 :]:
                difference = ImageChops.difference(left_mask, right_mask)
                difference_count = sum(1 for value in difference.get_flattened_data() if value)
                if difference_count == 0:
                    raise SystemExit(f"distinct mechanics share one core mask: {left_name}/{right_name}")
                shape_difference_counts.append(difference_count)
    finally:
        for image in frames.values():
            image.close()

    expected_previews = {
        "caldus-telegraphs.arena-scale.png": (1728, 1152),
        "caldus-combat.standard.50pct.png": (864, 576),
        "caldus-combat.reduced.50pct.png": (864, 576),
        "caldus-combat.reduced.50pct.grayscale.png": (864, 576),
        "caldus-combat.standard.1280x720.review-mock.png": (1280, 720),
        "caldus-combat.reduced.1280x720.review-mock.png": (1280, 720),
        "caldus-combat.standard.1920x1080.review-mock.png": (1920, 1080),
        "caldus-combat.reduced.1920x1080.review-mock.png": (1920, 1080),
    }
    for name, dimensions in expected_previews.items():
        with Image.open(pack / "previews" / name) as preview:
            if preview.size != dimensions:
                raise SystemExit(f"preview dimensions differ: {name}: {preview.size}")
    print(
        f"verified {len(expected)} SHA-256 entries, 14 alpha frames, parity, dimensions, "
        f"grayscale, visual-variant minimum {min(pair_difference_counts)} px, and "
        f"shape-distinction minimum {min(shape_difference_counts)} px"
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True, help="repository root")
    parser.add_argument("--verify", action="store_true", help="verify checked-in outputs without rebuilding")
    args = parser.parse_args()
    root = args.root.resolve()
    if args.verify:
        verify(root)
    else:
        build(root)
        verify(root)


if __name__ == "__main__":
    main()
