"""生成应用图标（纯标准库，不装 Pillow）。

为什么要自己画：仓库里不该出现一个来路不明的 png，而 `tauri build` 又必须
有图标文件才肯打包。画的是程序自己做的事 —— 一条折线加两个端点。

产物（相对本文件两级的 src-tauri/icons/）：
  icon.png 1024 · 128x128.png · 128x128@2x.png(256) · 32x32.png · icon.ico

用法：
  python tools/make_icon.py
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

ACCENT = (91, 110, 225)  # #5b6ee1，与地铁配色的第一个颜色一致
PAPER = (255, 255, 255)

# 折线的控制点（0..1 的相对坐标），起点 → 终点。
PATH = [(0.28, 0.72), (0.40, 0.40), (0.60, 0.62), (0.74, 0.30)]


def write_png(path: Path, size: int) -> None:
    """写一张 RGBA PNG。逐像素判断颜色，够慢但只在建图标时跑一次。"""
    rows = bytearray()
    radius = 0.22  # 相对单位（0..1）。写成 size*0.22 的话 1-radius 变成负数，
                   # 角判定永远不成立，整张图都是透明 —— 那是真发生过的。
    for y in range(size):
        rows.append(0)  # filter type 0
        for x in range(size):
            rows.extend(pixel(x, y, size, radius))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFF_FFFF)
        )

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(rows), 9))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(png)


def pixel(x: int, y: int, size: int, radius: float) -> tuple[int, int, int, int]:
    u = (x + 0.5) / size
    v = (y + 0.5) / size

    # 圆角方形底：角外的像素全透明。
    cx = min(max(u, radius), 1 - radius)
    cy = min(max(v, radius), 1 - radius)
    if math.hypot(u - cx, v - cy) > radius:
        return (0, 0, 0, 0)

    # 折线：点到各段的距离小于线宽就画白。
    width = 0.085
    for (x1, y1), (x2, y2) in zip(PATH, PATH[1:]):
        if distance_to_segment(u, v, x1, y1, x2, y2) < width:
            return (*PAPER, 255)

    # 端点：起点实心、终点空心，一眼能看出方向。
    dot = 0.075
    if math.hypot(u - PATH[0][0], v - PATH[0][1]) < dot:
        return (*PAPER, 255)
    end = math.hypot(u - PATH[-1][0], v - PATH[-1][1])
    if abs(end - dot) < 0.032:
        return (*PAPER, 255)

    return (*ACCENT, 255)


def distance_to_segment(px: float, py: float, x1: float, y1: float, x2: float, y2: float) -> float:
    dx, dy = x2 - x1, y2 - y1
    length_sq = dx * dx + dy * dy
    if length_sq == 0:
        return math.hypot(px - x1, py - y1)
    t = min(max(((px - x1) * dx + (py - y1) * dy) / length_sq, 0.0), 1.0)
    return math.hypot(px - (x1 + t * dx), py - (y1 + t * dy))


def write_ico(path: Path, png_bytes: bytes, size: int) -> None:
    """ICO 里直接塞 PNG 数据 —— Windows Vista 起就支持，不必转成 BMP。"""
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", size, size, 0, 0, 1, 32, len(png_bytes), 22)
    path.write_bytes(header + entry + png_bytes)


def main() -> None:
    icons = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
    icons.mkdir(parents=True, exist_ok=True)

    for name, size in [
        ("icon.png", 1024),
        ("128x128@2x.png", 256),
        ("128x128.png", 128),
        ("Square30x30Logo.png", 30),
        ("Square44x44Logo.png", 44),
        ("Square71x71Logo.png", 71),
        ("Square89x89Logo.png", 89),
        ("Square107x107Logo.png", 107),
        ("Square142x142Logo.png", 142),
        ("Square150x150Logo.png", 150),
        ("Square284x284Logo.png", 284),
        ("Square310x310Logo.png", 310),
        ("StoreLogo.png", 50),
        ("32x32.png", 32),
        ("16x16.png", 16),
    ]:
        write_png(icons / name, size)

    write_ico(icons / "icon.ico", (icons / "128x128@2x.png").read_bytes(), 0)
    print(f"图标已生成：{icons}")


if __name__ == "__main__":
    main()
