import { describe, expect, it } from "vitest";
import {
  MAX_MERCATOR_LAT,
  fitBounds,
  latToTileY,
  lngToTileX,
  project,
  unproject,
  visibleTiles,
} from "./tile";

describe("Web Mercator 投影", () => {
  it("本初子午线与赤道的交点是原点", () => {
    const p = project({ lat: 0, lng: 0 }, 0);
    expect(p.x).toBeCloseTo(128, 5);
    expect(p.y).toBeCloseTo(128, 5);
  });

  it("武汉的坐标在合理范围内", () => {
    // 参考资料：x = lng·20037508.34/180 → 武汉 lng≈114.4 → x≈12878136
    const p = project({ lat: 30.5, lng: 114.4 }, 0);
    const metersX = (p.x / 256 - 0.5) * 2 * 20037508.34;
    expect(metersX).toBeCloseTo(114.4 * (20037508.34 / 180), 0);
  });

  it("纬度超出 85.0511 被钳制而不是产生 NaN", () => {
    const p = project({ lat: 89.9, lng: 0 }, 4);
    expect(Number.isFinite(p.y)).toBe(true);
    // 钳到边界后与 MAX_MERCATOR_LAT 的投影相同
    const edge = project({ lat: MAX_MERCATOR_LAT, lng: 0 }, 4);
    expect(p.y).toBeCloseTo(edge.y, 5);
  });

  it("project 与 unproject 互逆", () => {
    const p = { lat: 30.506531, lng: 114.423819 };
    for (const z of [0, 5, 12, 19]) {
      const back = unproject(project(p, z), z);
      expect(back.lat).toBeCloseTo(p.lat, 6);
      expect(back.lng).toBeCloseTo(p.lng, 6);
    }
  });
});

describe("瓦片编号", () => {
  it("武汉 z=12 落在常见瓦片上", () => {
    // 标准 XYZ 编号（与后端 tile_url 的 OSM/Carto 模板一致）：
    // x = floor((lng+180)/360 · 2^z)，y = floor((0.5 - ln((1+sin)/(1-sin))/(4π)) · 2^z)
    const x = lngToTileX(114.423819, 12);
    const y = latToTileY(30.506531, 12);
    expect(Math.floor(x)).toBe(3349);
    expect(Math.floor(y)).toBe(1683);
  });

  it("z=0 时全球只有一块瓦片", () => {
    expect(Math.floor(lngToTileX(0, 0))).toBe(0);
    expect(Math.floor(latToTileY(0, 0))).toBe(0);
  });
});

describe("fitBounds", () => {
  it("跨半城的路线给出能看清的缩放级别", () => {
    const bounds: [{ lat: number; lng: number }, { lat: number; lng: number }] = [
      { lat: 30.5065, lng: 114.4238 },
      { lat: 30.5493, lng: 114.2665 },
    ];
    const { center, zoom } = fitBounds(bounds, 900, 600);
    expect(zoom).toBeGreaterThanOrEqual(3);
    expect(zoom).toBeLessThanOrEqual(19);
    expect(center.lng).toBeCloseTo((114.4238 + 114.2665) / 2, 2);
  });

  it("很小的路线不会放得超出上限", () => {
    const bounds: [{ lat: number; lng: number }, { lat: number; lng: number }] = [
      { lat: 30.5065, lng: 114.4238 },
      { lat: 30.5068, lng: 114.4241 },
    ];
    const { zoom } = fitBounds(bounds, 900, 600);
    expect(zoom).toBeLessThanOrEqual(19);
  });
});

describe("visibleTiles", () => {
  it("900x600 视口在 z=12 覆盖约 4x3 块", () => {
    const tiles = visibleTiles({ lat: 30.5, lng: 114.4 }, 12, 900, 600);
    const xs = new Set(tiles.map((t) => t.x));
    const ys = new Set(tiles.map((t) => t.y));
    // 900/256≈3.5、600/256≈2.34，视口覆盖的行列数随中心点对齐落在 4~5 / 3~4 之间。
    expect(xs.size).toBeGreaterThanOrEqual(4);
    expect(xs.size).toBeLessThanOrEqual(5);
    expect(ys.size).toBeGreaterThanOrEqual(3);
    expect(ys.size).toBeLessThanOrEqual(4);
    expect(tiles.length).toBe(xs.size * ys.size);
  });

  it("越过经度边界时取模回绕", () => {
    // 本初子午线往西一点，会用到 x=-1 → 应回绕到最大列。
    const tiles = visibleTiles({ lat: 0, lng: -0.001 }, 4, 900, 600);
    const max = 16;
    for (const t of tiles) {
      expect(t.x).toBeGreaterThanOrEqual(0);
      expect(t.x).toBeLessThan(max);
      expect(t.y).toBeGreaterThanOrEqual(0);
    }
  });

  it("纬度越界的瓦片被丢弃", () => {
    const tiles = visibleTiles({ lat: 89, lng: 0 }, 8, 900, 600);
    expect(tiles.length).toBeGreaterThan(0);
    for (const t of tiles) {
      expect(t.y).toBeLessThan(256);
      expect(t.y).toBeGreaterThanOrEqual(0);
    }
  });
});
