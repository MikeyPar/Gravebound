"""Build deterministic Caldus resolution-state sheets and static review mocks."""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


INK = (8, 10, 11, 255)
PANEL = (19, 21, 22, 246)
STONE = (40, 38, 37, 255)
BRASS = (178, 138, 68, 255)
ASH = (222, 216, 199, 255)
MUTED = (133, 129, 120, 255)
EMBER = (157, 47, 42, 255)


def repository_root(start: Path) -> Path:
    for candidate in (start, *start.parents):
        if (candidate / "Gravebound_Production_GDD_v1_Canonical.md").is_file():
            return candidate
    raise ValueError("repository root not found")


def font(repo: Path, size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    name = "AlegreyaSans-Bold.ttf" if bold else "AlegreyaSans-Regular.ttf"
    path = repo / "assets" / "fonts" / "alegreya_sans" / name
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


def contact_sheet(frames: list[Image.Image], size: int, out: Path) -> None:
    gap = max(8, size // 8)
    image = checkerboard(size * len(frames) + gap * (len(frames) - 1), size, max(8, size // 8))
    for index, frame in enumerate(frames):
        sprite = frame.resize((size, size), Image.Resampling.NEAREST)
        image.alpha_composite(sprite, (index * (size + gap), 0))
    image.save(out, optimize=True)


def centered(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], label: str, face, fill) -> None:
    x0, y0, x1, y1 = box
    bounds = draw.textbbox((0, 0), label, font=face)
    x = x0 + ((x1 - x0) - (bounds[2] - bounds[0])) // 2
    y = y0 + ((y1 - y0) - (bounds[3] - bounds[1])) // 2 - bounds[1]
    draw.text((x, y), label, font=face, fill=fill)


def stone_backdrop(width: int, height: int, reduced: bool) -> Image.Image:
    image = Image.new("RGBA", (width, height), INK)
    draw = ImageDraw.Draw(image, "RGBA")
    course = max(26, height // 28)
    block = max(70, width // 18)
    for y in range(0, height, course):
        stagger = (y // course) & 1
        for x in range(-block, width + block, block):
            x0 = x + stagger * block // 2
            tone = 24 + ((x0 * 11 + y * 5) % 13)
            draw.rectangle(
                (x0, y, x0 + block - 4, y + course - 3),
                fill=(tone, tone - 2, tone - 3, 255),
                outline=(54, 50, 46, 150),
                width=max(1, width // 960),
            )
    draw.rectangle((0, 0, width, height), fill=(4, 6, 7, 148 if reduced else 110))
    return image


def panel_scene(
    image: Image.Image,
    box: tuple[int, int, int, int],
    badge: Image.Image,
    caldus: Image.Image,
    exit_sprite: Image.Image,
    pending_frame: Image.Image,
    committed: bool,
    reduced: bool,
    repo: Path,
) -> None:
    draw = ImageDraw.Draw(image, "RGBA")
    x0, y0, x1, y1 = box
    accent = EMBER if committed else MUTED
    draw.rectangle(box, fill=PANEL, outline=accent, width=max(1, image.width // 960))

    title = "REWARD COMMITTED / AT RISK" if committed else "BOSS DEFEATED / REWARD UNRESOLVED"
    subtitle = (
        "STABLE EXIT MAY APPEAR; ITEMS ARE NOT SECURED UNTIL EXTRACTION"
        if committed
        else "INERT CALDUS PERSISTS; EXIT REMAINS HIDDEN"
    )
    centered(draw, (x0, y0 + 10, x1, y0 + 52), title, font(repo, max(18, image.height // 45), True), ASH)
    centered(draw, (x0, y0 + 49, x1, y0 + 80), subtitle, font(repo, max(12, image.height // 70), True), accent)

    badge_size = min(96, max(64, (x1 - x0) // 7))
    image.alpha_composite(badge.resize((badge_size, badge_size), Image.Resampling.NEAREST), (x0 + 22, y0 + 18))

    floor_y = y1 - max(56, image.height // 12)
    draw.ellipse(
        (x0 + 24, floor_y - 22, x1 - 24, floor_y + 22),
        outline=(100, 91, 78, 70),
        width=max(1, image.width // 960),
    )
    caldus_size = min(192, max(130, (y1 - y0) // 3))
    caldus_x = x0 + (x1 - x0) // 2 - caldus_size // 2
    caldus_y = floor_y - caldus_size
    image.alpha_composite(caldus.resize((caldus_size, caldus_size), Image.Resampling.NEAREST), (caldus_x, caldus_y))

    if committed:
        exit_size = min(170, max(120, (y1 - y0) // 3))
        exit_x = x1 - exit_size - 36
        exit_y = floor_y - exit_size
        if not reduced:
            draw.ellipse(
                (exit_x + exit_size // 3, exit_y + exit_size // 5, exit_x + 2 * exit_size // 3, floor_y),
                fill=(228, 222, 205, 28),
            )
        image.alpha_composite(exit_sprite.resize((exit_size, exit_size), Image.Resampling.NEAREST), (exit_x, exit_y))
        slot_size = max(48, min(64, (x1 - x0) // 10))
        slot_x = x0 + 34
        slot_y = floor_y - slot_size
        image.alpha_composite(pending_frame.resize((slot_size, slot_size), Image.Resampling.NEAREST), (slot_x, slot_y))
        centered(
            draw,
            (slot_x - 10, slot_y + slot_size + 2, slot_x + slot_size + 10, slot_y + slot_size + 25),
            "AT RISK",
            font(repo, max(11, image.height // 82), True),
            EMBER,
        )
    else:
        draw.line((x1 - 150, floor_y - 70, x1 - 62, floor_y - 70), fill=(100, 95, 87, 110), width=2)
        centered(
            draw,
            (x1 - 168, floor_y - 104, x1 - 44, floor_y - 77),
            "EXIT HIDDEN",
            font(repo, max(11, image.height // 82), True),
            MUTED,
        )


def review_mock(root: Path, width: int, height: int, reduced: bool, out: Path) -> None:
    repo = repository_root(root)
    frames = [Image.open(root / "frames" / "state" / f"{index:02d}.png").convert("RGBA") for index in (1, 2)]
    caldus = Image.open(repo / "assets" / "core" / "bosses" / "sir_caldus" / "review" / "v6" / "frames" / "defeat" / "04.png").convert("RGBA")
    exit_sprite = Image.open(repo / "assets" / "core" / "dungeons" / "bell_fixed_route_landmarks" / "v1" / "runtime" / "bell-post-reward-exit.192.png").convert("RGBA")
    pending = Image.open(repo / "assets" / "core" / "ui" / "pending_loot_risk" / "v1" / "runtime" / "pending-slot-risk-frame.64.png").convert("RGBA")

    image = stone_backdrop(width, height, reduced)
    draw = ImageDraw.Draw(image, "RGBA")
    margin = max(18, width // 80)
    header_h = max(76, height // 10)
    footer_h = max(58, height // 14)
    draw.rectangle((margin, margin, width - margin, header_h), fill=(8, 10, 11, 244), outline=BRASS, width=1)
    draw.text((margin * 2, margin + 8), "B6 CALDUS RESOLUTION  /  STATIC ART REVIEW", font=font(repo, max(20, height // 38), True), fill=ASH)
    mode = "REDUCED EFFECTS" if reduced else "STANDARD EFFECTS"
    mode_face = font(repo, max(15, height // 56), True)
    mode_bounds = draw.textbbox((0, 0), mode, font=mode_face)
    draw.text((width - margin * 2 - (mode_bounds[2] - mode_bounds[0]), margin + 12), mode, font=mode_face, fill=BRASS)

    body_top = header_h + margin
    body_bottom = height - footer_h - margin * 2
    gap = margin
    panel_w = (width - margin * 2 - gap) // 2
    panel_scene(image, (margin, body_top, margin + panel_w, body_bottom), frames[0], caldus, exit_sprite, pending, False, reduced, repo)
    panel_scene(image, (margin + panel_w + gap, body_top, width - margin, body_bottom), frames[1], caldus, exit_sprite, pending, True, reduced, repo)

    footer_top = height - footer_h - margin
    draw.rectangle((margin, footer_top, width - margin, height - margin), fill=(8, 10, 11, 244), outline=BRASS, width=1)
    centered(
        draw,
        (margin, footer_top, width - margin, height - margin),
        "STORED REWARD AUTHORITY SELECTS STATE  /  ART CANNOT GRANT, PLACE, SECURE, EXTRACT, OR ADVANCE",
        font(repo, max(13, height // 59), True),
        ASH,
    )
    watermark = "STATIC REVIEW MOCK  /  NOT NATIVE CAPTURE"
    water_face = font(repo, max(12, height // 72), True)
    water_bounds = draw.textbbox((0, 0), watermark, font=water_face)
    water_w = water_bounds[2] - water_bounds[0]
    draw.rectangle((width - margin - water_w - 18, body_bottom - 30, width - margin, body_bottom), fill=(73, 24, 22, 232), outline=EMBER)
    draw.text((width - margin - water_w - 9, body_bottom - 25), watermark, font=water_face, fill=(241, 184, 171, 255))
    image.convert("RGB").save(out, optimize=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    frames = [Image.open(root / "frames" / "state" / f"{index:02d}.png").convert("RGBA") for index in (1, 2)]
    previews = root / "previews"
    previews.mkdir(parents=True, exist_ok=True)
    contact_sheet(frames, 96, previews / "caldus-resolution-states.96px.png")
    contact_sheet(frames, 48, previews / "caldus-resolution-states.48px.png")
    for width, height in ((1280, 720), (1920, 1080)):
        for reduced in (False, True):
            mode = "reduced" if reduced else "standard"
            review_mock(root, width, height, reduced, previews / f"caldus-resolution-states.{mode}.{width}x{height}.review-mock.png")


if __name__ == "__main__":
    main()
