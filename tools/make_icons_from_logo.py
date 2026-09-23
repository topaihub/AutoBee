# -*- coding: utf-8 -*-
"""Generate the full Tauri icon set from the user's AutoBee logo.

Steps:
1. Load AutoBee.png and trim near-white borders.
2. Flood-fill transparent from image edges (only removes background white,
   keeps white highlights inside the bee).
3. Pad content to a square with ~8% margin.
4. Emit every icon referenced by tauri.conf.json / NSIS installer.
"""
from __future__ import annotations

from collections import deque
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "AutoBee.png"
OUT = ROOT / "src-tauri" / "icons"

WHITE_THRESH = 245  # >= means "background-ish white"


def trim_white(img: Image.Image) -> Image.Image:
    """Crop away uniform white margins around the artwork."""
    rgb = img.convert("RGB")
    w, h = rgb.size
    px = rgb.load()

    def row_is_white(y: int) -> bool:
        return all(px[x, y][0] >= WHITE_THRESH and px[x, y][1] >= WHITE_THRESH
                   and px[x, y][2] >= WHITE_THRESH for x in range(0, w, 2))

    def col_is_white(x: int) -> bool:
        return all(px[x, y][0] >= WHITE_THRESH and px[x, y][1] >= WHITE_THRESH
                   and px[x, y][2] >= WHITE_THRESH for y in range(0, h, 2))

    top, bottom = 0, h - 1
    left, right = 0, w - 1
    while top < bottom and row_is_white(top):
        top += 1
    while bottom > top and row_is_white(bottom):
        bottom -= 1
    while left < right and col_is_white(left):
        left += 1
    while right > left and col_is_white(right):
        right -= 1
    return img.crop((left, top, right + 1, bottom + 1))


def make_transparent_bg(img: Image.Image) -> Image.Image:
    """Flood-fill near-white pixels connected to the border -> alpha 0."""
    rgba = img.convert("RGBA")
    w, h = rgba.size
    px = rgba.load()
    seen = bytearray(w * h)
    q: deque[tuple[int, int]] = deque()

    def is_whiteish(x: int, y: int) -> bool:
        r, g, b, _ = px[x, y]
        return r >= WHITE_THRESH and g >= WHITE_THRESH and b >= WHITE_THRESH

    for x in range(w):
        for y in (0, h - 1):
            if is_whiteish(x, y) and not seen[y * w + x]:
                seen[y * w + x] = 1
                q.append((x, y))
    for y in range(h):
        for x in (0, w - 1):
            if is_whiteish(x, y) and not seen[y * w + x]:
                seen[y * w + x] = 1
                q.append((x, y))
    while q:
        x, y = q.popleft()
        px[x, y] = (255, 255, 255, 0)
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < w and 0 <= ny < h and not seen[ny * w + nx] and is_whiteish(nx, ny):
                seen[ny * w + nx] = 1
                q.append((nx, ny))
    return rgba


def square_pad(img: Image.Image, margin: float = 0.08) -> Image.Image:
    """Center the artwork on a transparent square canvas with margin."""
    w, h = img.size
    side = int(max(w, h) / (1 - 2 * margin))
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.paste(img, ((side - w) // 2, (side - h) // 2), img)
    return canvas


def build_master() -> Image.Image:
    src = Image.open(SRC)
    art = make_transparent_bg(trim_white(src))
    return square_pad(art)


def save(master: Image.Image, name: str, size: int) -> None:
    master.resize((size, size), Image.LANCZOS).save(OUT / name)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    master = build_master()
    master.resize((1024, 1024), Image.LANCZOS).save(OUT / "icon.png")

    save(master, "128x128.png", 128)
    save(master, "128x128@2x.png", 256)
    save(master, "32x32.png", 32)
    save(master, "16x16.png", 16)

    # Windows Store / NSIS square logos
    for name, size in (
        ("Square30x30Logo.png", 30), ("Square44x44Logo.png", 44),
        ("Square71x71Logo.png", 71), ("Square89x89Logo.png", 89),
        ("Square107x107Logo.png", 107), ("Square142x142Logo.png", 142),
        ("Square150x150Logo.png", 150), ("Square284x284Logo.png", 284),
        ("Square310x310Logo.png", 310), ("StoreLogo.png", 50),
    ):
        save(master, name, size)

    # Multi-size .ico — save with the largest frame as base + append_images
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    frames = [master.resize((s, s), Image.LANCZOS) for s in ico_sizes]
    frames[-1].save(OUT / "icon.ico", format="ICO",
                    append_images=frames[:-1])

    print("icons written to", OUT)


if __name__ == "__main__":
    main()
