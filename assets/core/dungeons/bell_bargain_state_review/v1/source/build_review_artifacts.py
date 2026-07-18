"""Build deterministic Bell Bargain state review sheets and static presentation mocks."""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


INK = (11, 13, 14, 255)
PANEL = (22, 23, 24, 244)
STONE = (34, 32, 32, 255)
BRASS = (177, 137, 64, 255)
ASH = (220, 214, 196, 255)
VIOLET = (130, 78, 164, 255)
MUTED = (125, 122, 116, 255)
RED = (142, 50, 45, 255)


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    root = Path(__file__).resolve().parents[5]
    name = "AlegreyaSans-Bold.ttf" if bold else "AlegreyaSans-Regular.ttf"
    path = root / "fonts" / "alegreya_sans" / name
    try:
        return ImageFont.truetype(path, size=size)
    except OSError:
        return ImageFont.load_default()


def checkerboard(width: int, height: int, cell: int) -> Image.Image:
    image = Image.new("RGBA", (width, height), (29, 31, 33, 255))
    draw = ImageDraw.Draw(image)
    colors = ((35, 38, 40, 255), (50, 53, 55, 255))
    for y in range(0, height, cell):
        for x in range(0, width, cell):
            draw.rectangle(
                (x, y, min(x + cell - 1, width), min(y + cell - 1, height)),
                fill=colors[((x // cell) + (y // cell)) & 1],
            )
    return image


def contact_sheet(frames: list[Image.Image], scale: int, out: Path) -> None:
    gap = 12 * scale
    frame_size = 192 * scale
    image = checkerboard(frame_size * 3 + gap * 2, frame_size, 16 * scale)
    for index, frame in enumerate(frames):
        resized = frame.resize((frame_size, frame_size), Image.Resampling.NEAREST)
        image.alpha_composite(resized, (index * (frame_size + gap), 0))
    image.save(out)


def alpha_bbox(image: Image.Image, threshold: int = 8) -> tuple[int, int, int, int] | None:
    return image.getchannel("A").point(lambda value: 255 if value > threshold else 0).getbbox()


def normalize_generated_scale(root: Path) -> None:
    """Match generated state height to the locked open-state seed."""
    seed = Image.open(root / "frames" / "state" / "01.png").convert("RGBA")
    seed_bbox = alpha_bbox(seed)
    if seed_bbox is None:
        raise ValueError("locked open-state seed has no alpha content")
    target_height = seed_bbox[3] - seed_bbox[1]
    for index in (2, 3):
        path = root / "frames" / "state" / f"{index:02d}.png"
        frame = Image.open(path).convert("RGBA")
        bbox = alpha_bbox(frame)
        if bbox is None:
            raise ValueError(f"generated state {index:02d} has no alpha content")
        crop = frame.crop(bbox)
        width = max(1, round(crop.width * target_height / crop.height))
        resized = crop.resize((width, target_height), Image.Resampling.NEAREST)
        canvas = Image.new("RGBA", (192, 192), (0, 0, 0, 0))
        canvas.alpha_composite(resized, ((192 - width) // 2, 192 - target_height))
        canvas.save(path)


def stone_backdrop(width: int, height: int, reduced: bool) -> Image.Image:
    image = Image.new("RGBA", (width, height), INK)
    draw = ImageDraw.Draw(image, "RGBA")
    draw.rectangle((0, 0, width, height), fill=(9, 11, 12, 255))
    for y in range(0, height, max(28, height // 30)):
        offset = (y // max(28, height // 30)) % 2
        for x in range(-80, width + 80, max(72, width // 18)):
            x0 = x + offset * 34
            tone = 25 + ((x0 * 13 + y * 7) % 13)
            draw.rectangle(
                (x0, y, x0 + max(68, width // 19), y + max(25, height // 33)),
                fill=(tone, tone - 2, tone - 2, 255),
                outline=(53, 48, 44, 150),
                width=max(1, width // 960),
            )
    draw.rectangle((0, 0, width, height), fill=(5, 7, 8, 125 if reduced else 95))
    return image


def centered(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], text: str, face, fill) -> None:
    x0, y0, x1, y1 = box
    bounds = draw.textbbox((0, 0), text, font=face)
    x = x0 + ((x1 - x0) - (bounds[2] - bounds[0])) // 2
    y = y0 + ((y1 - y0) - (bounds[3] - bounds[1])) // 2 - bounds[1]
    draw.text((x, y), text, font=face, fill=fill)


def review_mock(
    frames: list[Image.Image], width: int, height: int, reduced: bool, out: Path
) -> None:
    image = stone_backdrop(width, height, reduced)
    draw = ImageDraw.Draw(image, "RGBA")
    margin = max(18, width // 80)
    header_h = max(70, height // 10)
    footer_h = max(50, height // 14)
    draw.rectangle((margin, margin, width - margin, header_h), fill=(9, 11, 12, 242), outline=BRASS, width=1)
    draw.text(
        (margin * 2, margin + 7),
        "B4 VEIL BARGAIN  /  STATIC ART REVIEW",
        font=font(max(20, height // 38), True),
        fill=ASH,
    )
    mode = "REDUCED EFFECTS" if reduced else "STANDARD EFFECTS"
    mode_face = font(max(15, height // 55), True)
    mode_box = draw.textbbox((0, 0), mode, font=mode_face)
    draw.text((width - margin * 2 - (mode_box[2] - mode_box[0]), margin + 11), mode, font=mode_face, fill=BRASS)
    draw.text(
        (margin * 2, header_h - max(25, height // 35)),
        "OPEN is unresolved. SELECTED and REFUSED appear only from stored authoritative projection.",
        font=font(max(13, height // 62)),
        fill=MUTED,
    )

    body_top = header_h + margin
    body_bottom = height - footer_h - margin * 2
    gap = margin
    card_w = (width - margin * 2 - gap * 2) // 3
    labels = (
        ("OPEN", "UNRESOLVED", VIOLET),
        ("SELECTED", "COMMITTED", BRASS),
        ("REFUSED", "COMMITTED", ASH),
    )
    sprite_size = min(int(card_w * 0.66), int((body_bottom - body_top) * 0.62))
    for index, (title, subtitle, accent) in enumerate(labels):
        x0 = margin + index * (card_w + gap)
        x1 = x0 + card_w
        draw.rectangle((x0, body_top, x1, body_bottom), fill=PANEL, outline=accent, width=max(1, width // 960))
        centered(draw, (x0, body_top + 10, x1, body_top + max(55, height // 13)), title, font(max(22, height // 34), True), ASH)
        centered(draw, (x0, body_top + max(54, height // 13), x1, body_top + max(88, height // 9)), subtitle, font(max(14, height // 58), True), accent)
        sprite_x = x0 + (card_w - sprite_size) // 2
        sprite_y = body_top + max(90, height // 8)
        if not reduced and index < 2:
            # Standard-effects context adds only sparse ambient motes. They are
            # deliberately detached from the sprite so they cannot read as an
            # interaction radius, safe-zone ring, or authoritative state cue.
            mote_color = (137, 82, 165, 105)
            mote_positions = (
                (sprite_x + sprite_size * 22 // 100, sprite_y + sprite_size * 31 // 100),
                (sprite_x + sprite_size * 78 // 100, sprite_y + sprite_size * 36 // 100),
                (sprite_x + sprite_size * 16 // 100, sprite_y + sprite_size * 58 // 100),
                (sprite_x + sprite_size * 84 // 100, sprite_y + sprite_size * 63 // 100),
            )
            mote = max(2, sprite_size // 80)
            for px, py in mote_positions:
                draw.polygon(((px, py - mote), (px + mote, py), (px, py + mote), (px - mote, py)), fill=mote_color)
        sprite = frames[index].resize((sprite_size, sprite_size), Image.Resampling.NEAREST)
        image.alpha_composite(sprite, (sprite_x, sprite_y))
        cue_y = min(body_bottom - max(35, height // 20), sprite_y + sprite_size + max(7, height // 110))
        if index == 0:
            points = [(x0 + card_w // 2 - 22, cue_y + 18), (x0 + card_w // 2, cue_y), (x0 + card_w // 2 + 22, cue_y + 18)]
            for px, py in points:
                draw.polygon(((px, py - 5), (px + 5, py), (px, py + 5), (px - 5, py)), fill=VIOLET)
        elif index == 1:
            draw.ellipse((x0 + card_w // 2 - 15, cue_y - 2, x0 + card_w // 2 + 15, cue_y + 28), outline=BRASS, width=max(2, width // 640))
            draw.ellipse((x0 + card_w // 2 - 4, cue_y + 9, x0 + card_w // 2 + 4, cue_y + 17), fill=VIOLET)
        else:
            draw.arc((x0 + card_w // 2 - 16, cue_y - 2, x0 + card_w // 2 + 16, cue_y + 28), 25, 155, fill=ASH, width=max(2, width // 640))
            draw.arc((x0 + card_w // 2 - 16, cue_y - 2, x0 + card_w // 2 + 16, cue_y + 28), 205, 335, fill=ASH, width=max(2, width // 640))

    footer_top = height - footer_h - margin
    draw.rectangle((margin, footer_top, width - margin, height - margin), fill=(9, 11, 12, 242), outline=BRASS, width=1)
    centered(
        draw,
        (margin, footer_top, width - margin, height - margin),
        "SERVER PROJECTION OWNS STATE  /  ART CANNOT SELECT, APPLY, OR ADVANCE B4 -> B5",
        font(max(14, height // 56), True),
        ASH,
    )
    watermark = "STATIC REVIEW MOCK  /  NOT NATIVE CAPTURE"
    water_face = font(max(12, height // 70), True)
    box = draw.textbbox((0, 0), watermark, font=water_face)
    draw.rectangle((width - margin - (box[2] - box[0]) - 18, body_bottom - 30, width - margin, body_bottom), fill=(73, 24, 22, 230), outline=RED)
    draw.text((width - margin - (box[2] - box[0]) - 9, body_bottom - 25), watermark, font=water_face, fill=(240, 180, 170, 255))
    image.convert("RGB").save(out, optimize=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root
    normalize_generated_scale(root)
    frames = [Image.open(root / "frames" / "state" / f"{i:02d}.png").convert("RGBA") for i in range(1, 4)]
    previews = root / "previews"
    previews.mkdir(parents=True, exist_ok=True)
    contact_sheet(frames, 1, previews / "bell-bargain-states.192px.png")
    half = [frame.resize((96, 96), Image.Resampling.NEAREST) for frame in frames]
    image = checkerboard(96 * 3 + 12 * 2, 96, 8)
    for index, frame in enumerate(half):
        image.alpha_composite(frame, (index * 108, 0))
    image.save(previews / "bell-bargain-states.96px.png")
    for width, height in ((1280, 720), (1920, 1080)):
        for reduced in (False, True):
            mode = "reduced" if reduced else "standard"
            review_mock(frames, width, height, reduced, previews / f"bell-bargain-states.{mode}.{width}x{height}.review-mock.png")


if __name__ == "__main__":
    main()
