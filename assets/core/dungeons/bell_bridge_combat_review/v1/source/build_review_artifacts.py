#!/usr/bin/env python3
"""Deterministically build the unregistered M03 B5 Mire Bridge review pack."""

from __future__ import annotations

import argparse
import hashlib
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


TILE = 32
ROOM_TILES = (23, 11)
ROOM_SIZE = (ROOM_TILES[0] * TILE, ROOM_TILES[1] * TILE)
PACK_RELATIVE = Path("assets/core/dungeons/bell_bridge_combat_review/v1")

INK = (9, 11, 14, 255)
WALL = (27, 29, 34, 255)
WALL_LIGHT = (54, 54, 58, 255)
STONE = (46, 46, 47, 255)
STONE_LIGHT = (68, 64, 59, 255)
STONE_DARK = (30, 31, 33, 255)
WATER = (15, 24, 30, 255)
WATER_LIGHT = (27, 47, 55, 255)
BRASS = (143, 104, 54, 255)
ASH = (229, 218, 195, 255)
RED = (150, 45, 39, 255)
RED_LIGHT = (217, 88, 69, 255)
VIOLET = (129, 82, 159, 255)
PLAYER = (180, 208, 195, 255)


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    windows = Path("C:/Windows/Fonts/seguisb.ttf" if bold else "C:/Windows/Fonts/segoeui.ttf")
    if windows.exists():
        return ImageFont.truetype(str(windows), size=size)
    return ImageFont.load_default()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def pixel_rect(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], fill: tuple[int, int, int, int]) -> None:
    draw.rectangle(box, fill=fill)


def build_bridge() -> Image.Image:
    """Render the exact 23x11 authored shell at 32 px per world tile."""
    image = Image.new("RGBA", ROOM_SIZE, STONE)
    draw = ImageDraw.Draw(image)

    # Exact deep-water volumes: north y=[0,3), south y=[9,11).
    draw.rectangle((0, 0, ROOM_SIZE[0] - 1, 3 * TILE - 1), fill=WATER)
    draw.rectangle((0, 9 * TILE, ROOM_SIZE[0] - 1, ROOM_SIZE[1] - 1), fill=WATER)
    for y in list(range(10, 3 * TILE, 20)) + list(range(9 * TILE + 10, ROOM_SIZE[1], 20)):
        offset = 8 if (y // 10) % 2 else 0
        for x in range(-24 + offset, ROOM_SIZE[0], 64):
            draw.line((x, y, x + 24, y), fill=WATER_LIGHT, width=2)
            draw.point((x + 28, y + 1), fill=(45, 63, 67, 255))

    # Dry bridge flagstones. Seams are irregular but never form hostile red/white language.
    for ty in range(3, 9):
        for tx in range(ROOM_TILES[0]):
            x0, y0 = tx * TILE, ty * TILE
            tone = STONE_LIGHT if (tx * 3 + ty * 5) % 7 == 0 else STONE
            draw.rectangle((x0, y0, x0 + TILE - 1, y0 + TILE - 1), fill=tone)
            draw.line((x0, y0, x0 + TILE - 1, y0), fill=STONE_DARK, width=2)
            draw.line((x0, y0, x0, y0 + TILE - 1), fill=STONE_DARK, width=2)
            if (tx + ty) % 4 == 0:
                draw.line((x0 + 8, y0 + 17, x0 + 14, y0 + 14), fill=(36, 37, 37, 255), width=1)

    # Low north/south parapets visibly separate safe floor from lethal deep water.
    for y in (3 * TILE - 8, 9 * TILE):
        draw.rectangle((0, y, ROOM_SIZE[0] - 1, y + 7), fill=WALL)
        draw.line((0, y, ROOM_SIZE[0] - 1, y), fill=WALL_LIGHT, width=2)
        for x in range(0, ROOM_SIZE[0], 64):
            draw.rectangle((x + 4, y + 2, min(x + 10, ROOM_SIZE[0] - 1), y + 5), fill=BRASS)

    # Two authored pattern-lane floor channels at x=7.5 and x=15.5 tiles.
    for center_tile in (7.5, 15.5):
        cx = round(center_tile * TILE)
        draw.rectangle((cx - 10, 3 * TILE, cx + 10, 9 * TILE - 1), fill=(33, 34, 35, 255))
        draw.line((cx - 9, 3 * TILE, cx - 9, 9 * TILE - 1), fill=(77, 69, 57, 255), width=2)
        draw.line((cx + 9, 3 * TILE, cx + 9, 9 * TILE - 1), fill=(77, 69, 57, 255), width=2)
        for y in range(3 * TILE + 8, 9 * TILE, 24):
            draw.arc((cx - 7, y, cx + 7, y + 12), 15, 165, fill=(97, 79, 53, 255), width=2)
            draw.arc((cx - 7, y, cx + 7, y + 12), 195, 345, fill=(97, 79, 53, 255), width=2)

    # West/east three-tile doorway thresholds at authored y=5.5.
    door_top = 4 * TILE
    door_bottom = 7 * TILE - 1
    for x0, x1 in ((0, 20), (ROOM_SIZE[0] - 21, ROOM_SIZE[0] - 1)):
        draw.rectangle((x0, door_top, x1, door_bottom), fill=(55, 52, 47, 255))
        for y in range(door_top + 8, door_bottom, 24):
            draw.line((x0, y, x1, y), fill=BRASS, width=2)

    # A restrained 1 px frame supports alpha-free tilemap review and prevents crop ambiguity.
    draw.rectangle((0, 0, ROOM_SIZE[0] - 1, ROOM_SIZE[1] - 1), outline=INK, width=1)
    return image


def lane_material(reduced: bool) -> Image.Image:
    """A 32px transparent repeating physical-Major lane material, not geometry authority."""
    image = Image.new("RGBA", (32, 32), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    if not reduced:
        draw.rectangle((0, 2, 31, 29), fill=(118, 30, 28, 52))
    draw.rectangle((0, 5, 31, 26), fill=(125, 36, 32, 150 if reduced else 126))
    draw.line((0, 5, 31, 5), fill=RED_LIGHT, width=3)
    draw.line((0, 26, 31, 26), fill=RED_LIGHT, width=3)
    draw.line((0, 15, 31, 15), fill=ASH, width=2 if reduced else 1)
    for x in (3, 15, 27):
        draw.line((x, 9, x + 6, 22), fill=ASH if reduced else (196, 181, 157, 230), width=2)
        draw.line((x + 6, 9, x, 22), fill=(83, 30, 28, 255), width=1)
    return image


def clipped_line(draw: ImageDraw.ImageDraw, points: tuple[tuple[int, int], tuple[int, int]], fill: tuple[int, int, int, int], width: int) -> None:
    draw.line(points, fill=fill, width=width)


def cross_overlay(diagonal: bool, reduced: bool) -> Image.Image:
    """Review-only room-scale overlay derived from the exact B5 dimensions."""
    image = Image.new("RGBA", ROOM_SIZE, (0, 0, 0, 0))
    center = (round(11.5 * TILE), round(5.5 * TILE))
    lines: list[tuple[tuple[int, int], tuple[int, int]]]
    if diagonal:
        lines = [((-160, -352), (ROOM_SIZE[0] + 160, ROOM_SIZE[1] + 352)), ((-160, ROOM_SIZE[1] + 352), (ROOM_SIZE[0] + 160, -352))]
    else:
        lines = [((0, center[1]), (ROOM_SIZE[0] - 1, center[1])), ((center[0], 3 * TILE), (center[0], 9 * TILE - 1))]

    # Render on a temporary layer, then clip to exact dry bridge walkable band.
    raw = Image.new("RGBA", ROOM_SIZE, (0, 0, 0, 0))
    draw = ImageDraw.Draw(raw)
    if not reduced:
        for line in lines:
            clipped_line(draw, line, (126, 31, 29, 52), 44)
    for line in lines:
        clipped_line(draw, line, RED_LIGHT, 32)
        clipped_line(draw, line, (113, 31, 29, 230), 24)
        clipped_line(draw, line, ASH, 4 if reduced else 3)

    # Non-color chain notches communicate lane orientation in grayscale/reduced effects.
    if diagonal:
        for offset in range(-300, ROOM_SIZE[0] + 300, 42):
            for sign in (-1, 1):
                y = center[1] + sign * (offset - center[0])
                draw.line((offset - 5, y - 8, offset + 5, y + 8), fill=ASH, width=2)
    else:
        for x in range(16, ROOM_SIZE[0], 42):
            draw.line((x - 5, center[1] - 10, x + 5, center[1] + 10), fill=ASH, width=2)
        for y in range(3 * TILE + 14, 9 * TILE, 42):
            draw.line((center[0] - 10, y - 5, center[0] + 10, y + 5), fill=ASH, width=2)

    mask = Image.new("L", ROOM_SIZE, 0)
    ImageDraw.Draw(mask).rectangle((0, 3 * TILE, ROOM_SIZE[0] - 1, 9 * TILE - 1), fill=255)
    raw.putalpha(Image.composite(raw.getchannel("A"), Image.new("L", ROOM_SIZE, 0), mask))
    image.alpha_composite(raw)
    return image


def enemy_sprite(root: Path, name: str) -> Image.Image:
    path = root / "assets/core/enemies/core_bell_encounter_trio/v1/runtime" / name
    return Image.open(path).convert("RGBA")


def draw_spawn_warning(draw: ImageDraw.ImageDraw, x: int, y: int, reduced: bool) -> None:
    radius = 15
    draw.ellipse((x - radius, y - radius, x + radius, y + radius), outline=ASH, width=2)
    for angle in range(0, 360, 90):
        rad = math.radians(angle)
        x0, y0 = x + int(8 * math.cos(rad)), y + int(8 * math.sin(rad))
        x1, y1 = x + int(13 * math.cos(rad)), y + int(13 * math.sin(rad))
        draw.line((x0, y0, x1, y1), fill=RED_LIGHT, width=3)
    if not reduced:
        draw.ellipse((x - 20, y - 20, x + 20, y + 20), outline=(160, 45, 39, 100), width=2)


def combat_scene(root: Path, reduced: bool, diagonal: bool) -> Image.Image:
    scene = build_bridge()
    scene.alpha_composite(cross_overlay(diagonal, reduced))
    draw = ImageDraw.Draw(scene)

    fodder_points = [(4, 4), (8, 7), (12, 4), (16, 7), (19, 4), (20, 7)]
    pilgrim = enemy_sprite(root, "drowned-pilgrim.48.png")
    sentry = enemy_sprite(root, "chain-sentry.48.png")
    for tx, ty in fodder_points:
        x, y = tx * TILE, ty * TILE
        draw_spawn_warning(draw, x, y, reduced)
        scene.alpha_composite(pilgrim, (x - 24, y - 42))
    sx, sy = round(11.5 * TILE), round(5.5 * TILE)
    scene.alpha_composite(sentry, (sx - 24, sy - 38))

    # Review-only local-player marker, deliberately shape-first and non-hostile.
    px, py = round(5.5 * TILE), round(5.5 * TILE)
    draw.polygon(((px, py - 15), (px - 10, py + 12), (px + 10, py + 12)), fill=PLAYER, outline=INK)
    draw.line((px, py - 10, px + 15, py - 17), fill=(192, 148, 79, 255), width=3)
    return scene


def fit_room(scene: Image.Image, max_w: int, max_h: int) -> Image.Image:
    scale = min(max_w / scene.width, max_h / scene.height)
    size = (max(1, round(scene.width * scale)), max(1, round(scene.height * scale)))
    return scene.resize(size, Image.Resampling.NEAREST)


def centered_text(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], value: str, face: ImageFont.ImageFont, fill: tuple[int, int, int, int]) -> None:
    bounds = draw.textbbox((0, 0), value, font=face)
    x = box[0] + (box[2] - box[0] - (bounds[2] - bounds[0])) // 2
    y = box[1] + (box[3] - box[1] - (bounds[3] - bounds[1])) // 2
    draw.text((x, y), value, font=face, fill=fill)


def review_mock(root: Path, out: Path, width: int, height: int, reduced: bool) -> None:
    canvas = Image.new("RGBA", (width, height), (12, 14, 17, 255))
    draw = ImageDraw.Draw(canvas)
    margin = max(20, width // 80)
    header_h = max(72, height // 9)
    footer_h = max(72, height // 9)
    body_top = margin + header_h
    body_bottom = height - margin - footer_h
    gap = max(16, width // 80)
    card_w = (width - 2 * margin - gap) // 2

    draw.rectangle((margin, margin, width - margin, margin + header_h - 8), fill=(20, 22, 26, 255), outline=BRASS, width=2)
    centered_text(draw, (margin, margin, width - margin, margin + header_h // 2), "B5 / MIRE BRIDGE / CHAIN SENTRY READABILITY", font(max(18, height // 34), True), ASH)
    centered_text(draw, (margin, margin + header_h // 2 - 4, width - margin, margin + header_h - 8), "STANDARD EFFECTS" if not reduced else "REDUCED EFFECTS / SAME MECHANICAL BOUNDS", font(max(14, height // 52), True), RED_LIGHT)

    for index, diagonal in enumerate((False, True)):
        x0 = margin + index * (card_w + gap)
        x1 = x0 + card_w
        draw.rectangle((x0, body_top, x1, body_bottom), fill=(20, 22, 25, 255), outline=(72, 68, 61, 255), width=2)
        label = "CAST 1 / CARDINAL AXES" if not diagonal else "CAST 2 / DIAGONAL AXES"
        centered_text(draw, (x0, body_top + 4, x1, body_top + 42), label, font(max(14, height // 52), True), ASH)
        scene = fit_room(combat_scene(root, reduced, diagonal), card_w - 24, body_bottom - body_top - 92)
        paste_x = x0 + (card_w - scene.width) // 2
        paste_y = body_top + 48 + (body_bottom - body_top - 82 - scene.height) // 2
        canvas.alpha_composite(scene, (paste_x, paste_y))
        centered_text(draw, (x0, body_bottom - 36, x1, body_bottom - 4), "900 ms / PHYSICAL MAJOR / WHITE CORE + RED EDGE + CHAIN NOTCHES", font(max(11, height // 70), False), (202, 194, 180, 255))

    footer_top = height - margin - footer_h + 8
    draw.rectangle((margin, footer_top, width - margin, height - margin), fill=(18, 20, 23, 255), outline=BRASS, width=2)
    centered_text(draw, (margin + 8, footer_top + 4, width - margin - 8, footer_top + footer_h // 2), "SERVER OWNS ATTACK AXES, 0.9-TILE WIDTH, ROOM CLIPPING, TIMING, DAMAGE, AND COLLISION", font(max(13, height // 56), True), ASH)
    centered_text(draw, (margin + 8, footer_top + footer_h // 2 - 2, width - margin - 8, height - margin - 2), "STATIC REVIEW MOCK / NOT NATIVE CAPTURE / ART CANNOT START OR RESOLVE B5", font(max(12, height // 62), True), RED_LIGHT)
    canvas.convert("RGB").save(out, optimize=True)


def contact_sheet(frames: list[Image.Image], out: Path) -> None:
    sheet = Image.new("RGBA", (ROOM_SIZE[0] * 2, ROOM_SIZE[1] * 2), (0, 0, 0, 0))
    for index, frame in enumerate(frames):
        sheet.alpha_composite(frame, ((index % 2) * ROOM_SIZE[0], (index // 2) * ROOM_SIZE[1]))
    sheet.save(out, optimize=True)


def minimum_scale_sheet(root: Path, out: Path) -> None:
    scenes = [combat_scene(root, reduced, diagonal).resize((368, 176), Image.Resampling.NEAREST) for reduced in (False, True) for diagonal in (False, True)]
    sheet = Image.new("RGB", (736, 352), (16, 18, 21))
    for index, scene in enumerate(scenes):
        sheet.paste(scene.convert("RGB"), ((index % 2) * 368, (index // 2) * 176))
    sheet.save(out, optimize=True)


def write_hashes(pack: Path) -> None:
    paths = sorted(
        path
        for path in pack.rglob("*")
        if path.is_file() and path.name != "SHA256SUMS.txt"
        and "__pycache__" not in path.parts
    )
    lines = [f"{sha256(path)}  {path.relative_to(pack).as_posix()}" for path in paths]
    (pack / "SHA256SUMS.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def build(root: Path) -> None:
    pack = root / PACK_RELATIVE
    for relative in ("runtime", "frames/cross_lanes", "previews"):
        (pack / relative).mkdir(parents=True, exist_ok=True)

    bridge = build_bridge()
    bridge.save(pack / "runtime/mire-bridge.736x352.png", optimize=True)
    lane_material(False).save(pack / "runtime/chain-lane-pattern.standard.32.png", optimize=True)
    lane_material(True).save(pack / "runtime/chain-lane-pattern.reduced.32.png", optimize=True)

    frames = []
    for name, diagonal, reduced in (
        ("01-cardinal.standard.png", False, False),
        ("02-diagonal.standard.png", True, False),
        ("03-cardinal.reduced.png", False, True),
        ("04-diagonal.reduced.png", True, True),
    ):
        frame = cross_overlay(diagonal, reduced)
        frame.save(pack / "frames/cross_lanes" / name, optimize=True)
        frames.append(frame)
    contact_sheet(frames, pack / "previews/chain-sentry-cross-lanes.room-scale.png")
    minimum_scale_sheet(root, pack / "previews/bridge-combat.50pct.png")

    for width, height in ((1280, 720), (1920, 1080)):
        for reduced in (False, True):
            mode = "reduced" if reduced else "standard"
            review_mock(root, pack / f"previews/mire-bridge.{mode}.{width}x{height}.review-mock.png", width, height, reduced)
    write_hashes(pack)


def verify(root: Path) -> None:
    pack = root / PACK_RELATIVE
    expected = {}
    for line in (pack / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines():
        digest, relative = line.split("  ", 1)
        expected[relative] = digest
    for relative, digest in expected.items():
        actual = sha256(pack / relative)
        if actual != digest:
            raise SystemExit(f"hash mismatch: {relative}: expected {digest}, got {actual}")

    required_rgba = [pack / "runtime/mire-bridge.736x352.png"]
    required_rgba += sorted((pack / "runtime").glob("chain-lane-pattern.*.png"))
    required_rgba += sorted((pack / "frames/cross_lanes").glob("*.png"))
    for path in required_rgba:
        with Image.open(path) as image:
            if image.mode != "RGBA":
                raise SystemExit(f"expected RGBA: {path}")
    with Image.open(pack / "runtime/mire-bridge.736x352.png") as room:
        if room.size != ROOM_SIZE:
            raise SystemExit(f"room dimensions differ: {room.size}")
    expected_frame_bounds = {
        "01-cardinal.standard.png": (0, 96, 736, 288),
        "02-diagonal.standard.png": (226, 96, 511, 288),
        "03-cardinal.reduced.png": (0, 96, 736, 288),
        "04-diagonal.reduced.png": (235, 96, 502, 288),
    }
    for path in sorted((pack / "frames/cross_lanes").glob("*.png")):
        with Image.open(path) as frame:
            if frame.size != ROOM_SIZE or frame.getbbox() is None:
                raise SystemExit(f"invalid lane frame: {path}")
            if frame.getpixel((0, 0))[3] != 0:
                raise SystemExit(f"lane frame corner is not transparent: {path}")
            if frame.getchannel("A").getbbox() != expected_frame_bounds[path.name]:
                raise SystemExit(f"lane frame alpha bounds differ: {path}")

    expected_material_bounds = {
        "chain-lane-pattern.standard.32.png": (0, 2, 32, 30),
        "chain-lane-pattern.reduced.32.png": (0, 4, 32, 28),
    }
    for name, bounds in expected_material_bounds.items():
        with Image.open(pack / "runtime" / name) as material:
            if material.size != (32, 32) or material.getchannel("A").getbbox() != bounds:
                raise SystemExit(f"lane material dimensions/alpha bounds differ: {name}")
            grayscale = material.convert("RGB").convert("L")
            opaque_luma = [
                value
                for value, alpha in zip(
                    grayscale.get_flattened_data(), material.getchannel("A").get_flattened_data()
                )
                if alpha >= 150
            ]
            if not opaque_luma or max(opaque_luma) < 200 or min(opaque_luma) > 100:
                raise SystemExit(f"lane material lost grayscale core/edge contrast: {name}")
    for width, height in ((1280, 720), (1920, 1080)):
        for mode in ("standard", "reduced"):
            path = pack / f"previews/mire-bridge.{mode}.{width}x{height}.review-mock.png"
            with Image.open(path) as preview:
                if preview.size != (width, height):
                    raise SystemExit(f"preview dimensions differ: {path}")
    print(f"verified {len(expected)} SHA-256 entries and all dimensions/alpha gates")


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
