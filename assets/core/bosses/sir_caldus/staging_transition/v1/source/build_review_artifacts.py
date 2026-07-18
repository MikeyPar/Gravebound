#!/usr/bin/env python3
"""Build and verify the unregistered Sir Caldus staging-transition review pack."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageEnhance, ImageFont, ImageOps


PACK_REL = Path("assets/core/bosses/sir_caldus/staging_transition/v1")
SOURCE_STRIP = Path("source/sir-caldus-boss-lock.alpha.png")
ARENA_REL = Path("assets/core/bosses/sir_caldus/combat_presentation/v1/runtime/caldus-bell-court.576x576.png")
CALDUS_REL = Path("assets/core/bosses/sir_caldus/review/v3/frames/idle/01.png")
FRAME_NAMES = ("dormant", "arming", "sealed", "introduction")
EFFECTS = ("standard", "reduced")


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    windows = Path("C:/Windows/Fonts")
    path = windows / ("segoeuib.ttf" if bold else "segoeui.ttf")
    if path.exists():
        return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default()


def contain(image: Image.Image, size: tuple[int, int]) -> Image.Image:
    result = Image.new("RGBA", size, (0, 0, 0, 0))
    copy = image.copy()
    copy.thumbnail(size, Image.Resampling.LANCZOS)
    result.alpha_composite(copy, ((size[0] - copy.width) // 2, (size[1] - copy.height) // 2))
    return result


def reduced_effects(image: Image.Image) -> Image.Image:
    """Reduce optional amber intensity without changing alpha geometry."""
    rgba = image.convert("RGBA")
    rgb = rgba.convert("RGB")
    rgb = ImageEnhance.Color(rgb).enhance(0.72)
    rgb = ImageEnhance.Contrast(rgb).enhance(0.94)
    rgb = ImageEnhance.Brightness(rgb).enhance(0.91)
    rgb.putalpha(rgba.getchannel("A"))
    return rgb


def split_lock_frames(pack: Path) -> dict[tuple[str, str], Image.Image]:
    source = Image.open(pack / SOURCE_STRIP).convert("RGBA")
    if source.size != (2048, 768):
        raise ValueError(f"unexpected generated strip size: {source.size}")
    output: dict[tuple[str, str], Image.Image] = {}
    for index, state in enumerate(FRAME_NAMES):
        # Every generated slot is 512 px wide. The shared 512 px vertical crop preserves
        # pose scale and center across states while discarding empty chroma-key padding.
        slot = source.crop((index * 512, 128, (index + 1) * 512, 640))
        standard = slot.resize((256, 256), Image.Resampling.LANCZOS)
        variants = {"standard": standard, "reduced": reduced_effects(standard)}
        for effects, frame in variants.items():
            path = pack / "frames" / "boss_lock" / f"{index + 1:02d}-{state}.{effects}.png"
            path.parent.mkdir(parents=True, exist_ok=True)
            frame.save(path, optimize=True)
            output[(state, effects)] = frame

    for effects in EFFECTS:
        sheet = Image.new("RGBA", (1024, 256), (0, 0, 0, 0))
        for index, state in enumerate(FRAME_NAMES):
            sheet.alpha_composite(output[(state, effects)], (index * 256, 0))
        runtime = pack / "runtime" / f"boss-lock-state-sheet.{effects}.1024x256.png"
        runtime.parent.mkdir(parents=True, exist_ok=True)
        sheet.save(runtime, optimize=True)
    return output


def build_countdown_dial(effects: str) -> Image.Image:
    image = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    if effects == "standard":
        for radius, alpha in ((55, 26), (53, 42), (51, 58)):
            draw.ellipse((64 - radius, 64 - radius, 64 + radius, 64 + radius), outline=(217, 154, 43, alpha), width=3)
    draw.ellipse((16, 16, 112, 112), outline=(38, 43, 45, 235), width=8)
    draw.ellipse((22, 22, 106, 106), outline=(154, 108, 37, 255), width=3)
    draw.ellipse((29, 29, 99, 99), outline=(234, 223, 189, 235), width=2)
    # Five non-color notches communicate the authored five-second boundary without
    # embedding countdown progress, numbers, or a client-owned clock.
    import math

    for index in range(5):
        angle = math.radians(-90 + index * 72)
        cx, cy = 64 + math.cos(angle) * 48, 64 + math.sin(angle) * 48
        tx, ty = -math.sin(angle), math.cos(angle)
        points = [
            (cx + tx * 4, cy + ty * 4),
            (cx - tx * 4, cy - ty * 4),
            (64 + math.cos(angle) * 40 - tx * 2, 64 + math.sin(angle) * 40 - ty * 2),
            (64 + math.cos(angle) * 40 + tx * 2, 64 + math.sin(angle) * 40 + ty * 2),
        ]
        draw.polygon(points, fill=(234, 223, 189, 255))
    return image


def build_introduction_frame(effects: str) -> Image.Image:
    image = Image.new("RGBA", (512, 96), (8, 11, 12, 224))
    draw = ImageDraw.Draw(image)
    if effects == "standard":
        draw.rectangle((4, 4, 507, 91), outline=(217, 154, 43, 66), width=6)
    draw.rectangle((8, 8, 503, 87), outline=(154, 108, 37, 255), width=2)
    draw.line((40, 22, 472, 22), fill=(234, 223, 189, 230), width=1)
    draw.line((40, 74, 472, 74), fill=(84, 64, 35, 255), width=1)
    # Bell-clapper corner motifs are decorative UI framing, not healing/safe glyphs.
    for x in (24, 488):
        draw.ellipse((x - 5, 43, x + 5, 53), outline=(154, 108, 37, 255), width=2)
        draw.line((x, 32, x, 43), fill=(234, 223, 189, 230), width=2)
    return image


def build_runtime_materials(pack: Path) -> dict[tuple[str, str], Image.Image]:
    output: dict[tuple[str, str], Image.Image] = {}
    for effects in EFFECTS:
        dial = build_countdown_dial(effects)
        frame = build_introduction_frame(effects)
        dial.save(pack / "runtime" / f"boss-ready-countdown-dial.{effects}.128.png", optimize=True)
        frame.save(pack / "runtime" / f"caldus-introduction-frame.{effects}.512x96.png", optimize=True)
        output[("dial", effects)] = dial
        output[("intro_frame", effects)] = frame
    return output


def place_scene_marker(draw: ImageDraw.ImageDraw, xy: tuple[int, int], color: tuple[int, int, int, int]) -> None:
    x, y = xy
    draw.polygon(((x, y - 9), (x - 7, y + 7), (x + 7, y + 7)), fill=color, outline=(234, 223, 189, 255))


def scene_panel(
    root: Path,
    lock: Image.Image,
    materials: dict[tuple[str, str], Image.Image],
    state: str,
    effects: str,
    size: tuple[int, int],
) -> Image.Image:
    panel = Image.new("RGBA", size, (13, 17, 18, 255))
    arena = Image.open(root / ARENA_REL).convert("RGBA")
    arena_side = min(size[0] - 24, size[1] - 78)
    arena = arena.resize((arena_side, arena_side), Image.Resampling.NEAREST)
    arena_xy = ((size[0] - arena_side) // 2, 54)
    panel.alpha_composite(arena, arena_xy)
    draw = ImageDraw.Draw(panel)

    lock_size = max(82, arena_side // 4)
    lock_small = lock.resize((lock_size, lock_size), Image.Resampling.LANCZOS)
    lock_xy = (arena_xy[0] - lock_size // 3, arena_xy[1] + arena_side // 2 - lock_size // 2)
    panel.alpha_composite(lock_small, lock_xy)

    stage = (arena_xy[0] + int(arena_side * 2.5 / 18), arena_xy[1] + arena_side // 2)
    group_a = (arena_xy[0] + int(arena_side * 2.5 / 18), arena_xy[1] + int(arena_side * 6 / 18))
    group_b = (arena_xy[0] + int(arena_side * 2.5 / 18), arena_xy[1] + int(arena_side * 12 / 18))
    for marker in (stage, group_a, group_b):
        place_scene_marker(draw, marker, (118, 169, 164, 255))

    if state == "countdown":
        dial = materials[("dial", effects)].resize((92, 92), Image.Resampling.LANCZOS)
        panel.alpha_composite(dial, ((size[0] - 92) // 2, 64))
        draw.text((size[0] // 2, 110), "3", font=font(28, True), fill=(234, 223, 189, 255), anchor="mm")
    elif state == "introduction":
        caldus = Image.open(root / CALDUS_REL).convert("RGBA").resize((128, 128), Image.Resampling.LANCZOS)
        panel.alpha_composite(caldus, (arena_xy[0] + arena_side // 2 - 64, arena_xy[1] + arena_side // 2 - 74))
        intro = materials[("intro_frame", effects)].resize((min(size[0] - 36, 390), 72), Image.Resampling.LANCZOS)
        intro_xy = ((size[0] - intro.width) // 2, size[1] - 82)
        panel.alpha_composite(intro, intro_xy)
        draw.text((size[0] // 2, intro_xy[1] + 25), "SIR CALDUS", font=font(18, True), fill=(234, 223, 189, 255), anchor="mm")
        draw.text((size[0] // 2, intro_xy[1] + 49), "BELL-BOUND KNIGHT", font=font(12), fill=(188, 158, 99, 255), anchor="mm")
    return panel


def build_review_mock(
    root: Path,
    pack: Path,
    frames: dict[tuple[str, str], Image.Image],
    materials: dict[tuple[str, str], Image.Image],
    effects: str,
    size: tuple[int, int],
) -> Image.Image:
    canvas = Image.new("RGB", size, (13, 17, 18))
    draw = ImageDraw.Draw(canvas)
    margin = max(18, round(size[0] * 0.015))
    header_h = round(size[1] * 0.115)
    footer_h = round(size[1] * 0.10)
    gap = max(14, round(size[0] * 0.012))
    panel_w = (size[0] - margin * 2 - gap * 2) // 3
    panel_h = size[1] - margin * 2 - header_h - footer_h - gap * 2

    draw.rectangle((margin, margin, size[0] - margin, margin + header_h), outline=(154, 108, 37), width=2)
    draw.text((size[0] // 2, margin + header_h * 0.38), "B5 → B6 / SIR CALDUS BOSS-LOCK TRANSITION", font=font(max(18, size[0] // 64), True), fill=(234, 223, 189), anchor="mm")
    draw.text((size[0] // 2, margin + header_h * 0.72), f"{effects.upper()} EFFECTS / STATIC REVIEW COMPOSITION", font=font(max(12, size[0] // 100)), fill=(118, 202, 194), anchor="mm")

    labels = ("STAGING / DOOR OPEN", "5-SECOND READY COUNTDOWN", "LOCKED / 2.5-SECOND INTRODUCTION")
    scene_states = (("dormant", "staging"), ("arming", "countdown"), ("introduction", "introduction"))
    top = margin + header_h + gap
    for index, ((lock_state, scene_state), label) in enumerate(zip(scene_states, labels)):
        left = margin + index * (panel_w + gap)
        draw.rectangle((left, top, left + panel_w, top + panel_h), outline=(83, 77, 64), width=2)
        draw.text((left + panel_w // 2, top + 18), label, font=font(max(11, size[0] // 110), True), fill=(234, 223, 189), anchor="mm")
        scene = scene_panel(root, frames[(lock_state, effects)], materials, scene_state, effects, (panel_w - 12, panel_h - 36))
        canvas.paste(scene.convert("RGB"), (left + 6, top + 30))

    footer_top = top + panel_h + gap
    draw.rectangle((margin, footer_top, size[0] - margin, size[1] - margin), outline=(154, 108, 37), width=2)
    draw.text((size[0] // 2, footer_top + footer_h * 0.34), "SERVER OWNS LOAD GATE, COUNTDOWN TICKS, PARTICIPANT LOCK, DOOR COLLISION, AND PHASE START", font=font(max(10, size[0] // 112), True), fill=(234, 223, 189), anchor="mm")
    draw.text((size[0] // 2, footer_top + footer_h * 0.70), "UNREGISTERED REVIEW CANDIDATE / NOT NATIVE CAPTURE / ART CANNOT START OR RESOLVE B6", font=font(max(9, size[0] // 128)), fill=(234, 91, 72), anchor="mm")
    return canvas


def build_preview_sheet(
    frames: dict[tuple[str, str], Image.Image], materials: dict[tuple[str, str], Image.Image]
) -> Image.Image:
    canvas = Image.new("RGBA", (1024, 640), (13, 17, 18, 255))
    draw = ImageDraw.Draw(canvas)
    draw.text((512, 34), "SIR CALDUS BOSS-LOCK / ACTUAL-SCALE REVIEW", font=font(24, True), fill=(234, 223, 189, 255), anchor="mm")
    for row, effects in enumerate(EFFECTS):
        y = 74 + row * 262
        draw.text((20, y + 118), effects.upper(), font=font(16, True), fill=(118, 202, 194, 255), anchor="lm")
        for index, state in enumerate(FRAME_NAMES):
            x = 118 + index * 224
            frame = frames[(state, effects)].resize((192, 192), Image.Resampling.LANCZOS)
            canvas.alpha_composite(frame, (x, y))
            draw.text((x + 96, y + 210), state.upper(), font=font(13, True), fill=(234, 223, 189, 255), anchor="mm")
        canvas.alpha_composite(materials[("dial", effects)], (70, y + 124))
        intro = materials[("intro_frame", effects)].resize((384, 72), Image.Resampling.LANCZOS)
        canvas.alpha_composite(intro, (320, y + 226))
    return canvas


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_hashes(pack: Path) -> None:
    paths = sorted(path for path in pack.rglob("*.png") if path.is_file())
    lines = [f"{sha256(path)}  {path.relative_to(pack).as_posix()}" for path in paths]
    (pack / "SHA256SUMS.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def verify(pack: Path) -> None:
    expected = {}
    for line in (pack / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines():
        digest, rel = line.split("  ", 1)
        expected[rel] = digest
    for rel, digest in expected.items():
        path = pack / rel
        if not path.is_file() or sha256(path) != digest:
            raise SystemExit(f"hash mismatch: {rel}")

    for effects in EFFECTS:
        std_or_reduced = []
        for index, state in enumerate(FRAME_NAMES):
            path = pack / "frames" / "boss_lock" / f"{index + 1:02d}-{state}.{effects}.png"
            image = Image.open(path).convert("RGBA")
            if image.size != (256, 256) or image.getchannel("A").getbbox() is None:
                raise SystemExit(f"invalid runtime frame: {path}")
            if any(image.getpixel(point)[3] > 8 for point in ((0, 0), (255, 0), (0, 255), (255, 255))):
                raise SystemExit(f"opaque runtime corner: {path}")
            std_or_reduced.append(image)

    for index, state in enumerate(FRAME_NAMES):
        standard = Image.open(pack / "frames" / "boss_lock" / f"{index + 1:02d}-{state}.standard.png").convert("RGBA")
        reduced = Image.open(pack / "frames" / "boss_lock" / f"{index + 1:02d}-{state}.reduced.png").convert("RGBA")
        if ImageChops.difference(standard.getchannel("A"), reduced.getchannel("A")).getbbox() is not None:
            raise SystemExit(f"standard/reduced alpha drift: {state}")
        if ImageChops.difference(standard.convert("RGB"), reduced.convert("RGB")).getbbox() is None:
            raise SystemExit(f"standard/reduced visual variants are identical: {state}")

    for effects in EFFECTS:
        dial = Image.open(pack / "runtime" / f"boss-ready-countdown-dial.{effects}.128.png")
        intro = Image.open(pack / "runtime" / f"caldus-introduction-frame.{effects}.512x96.png")
        if dial.size != (128, 128) or intro.size != (512, 96):
            raise SystemExit(f"invalid presentation material dimensions: {effects}")
    for width, height in ((1280, 720), (1920, 1080)):
        for effects in EFFECTS:
            path = pack / "previews" / f"caldus-staging-transition.{effects}.{width}x{height}.review-mock.png"
            if Image.open(path).size != (width, height):
                raise SystemExit(f"invalid review mock: {path}")
    print(f"Verified {len(expected)} hashed files in {pack}")


def build(root: Path) -> Path:
    pack = root / PACK_REL
    frames = split_lock_frames(pack)
    materials = build_runtime_materials(pack)
    previews = pack / "previews"
    previews.mkdir(parents=True, exist_ok=True)
    build_preview_sheet(frames, materials).save(previews / "boss-lock-actual-scale-review.png", optimize=True)
    for effects in EFFECTS:
        for size in ((1280, 720), (1920, 1080)):
            mock = build_review_mock(root, pack, frames, materials, effects, size)
            mock.save(previews / f"caldus-staging-transition.{effects}.{size[0]}x{size[1]}.review-mock.png", optimize=True)
    write_hashes(pack)
    return pack


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    pack = root / PACK_REL
    if args.verify:
        verify(pack)
    else:
        build(root)
        verify(pack)


if __name__ == "__main__":
    main()
