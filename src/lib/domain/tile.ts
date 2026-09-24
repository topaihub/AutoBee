/**
 * Web Mercator 投影与瓦片数学（EPSG:3857，wkid 102100/3857）。纬度必须钳制在 ±85.0511°
 * （投影定义域）：超出后 tan 发散、坐标变成 NaN，现象是画布上整条路线静默消失。
 * 世界像素坐标 = 投影坐标缩放到 [0, 256·2^z)。
 */

export const TILE_SIZE = 256;

export const MAX_MERCATOR_LAT = 85.0511;

/** 赤道周长的一半，即投影坐标的满幅（米）。 */
export const WEB_MERCATOR_EXTENT = 20037508.34;

export type LatLng = { lat: number; lng: number };

export type WorldPixel = { x: number; y: number };

export function project(p: LatLng, zoom: number): WorldPixel {
  const lat = Math.min(Math.max(p.lat, -MAX_MERCATOR_LAT), MAX_MERCATOR_LAT);
  const scale = (TILE_SIZE * 2 ** zoom) / 360;
  const x = (p.lng + 180) * scale;
  const sin = Math.sin((lat * Math.PI) / 180);
  const y = (0.5 - Math.log((1 + sin) / (1 - sin)) / (4 * Math.PI)) * TILE_SIZE * 2 ** zoom;
  return { x, y };
}

export function unproject(px: WorldPixel, zoom: number): LatLng {
  const scale = TILE_SIZE * 2 ** zoom;
  const lng = (px.x / scale) * 360 - 180;
  const n = Math.PI - (2 * Math.PI * px.y) / scale;
  const lat = (180 / Math.PI) * Math.atan(0.5 * (Math.exp(n) - Math.exp(-n)));
  return { lat, lng: ((lng + 540) % 360) - 180 };
}

export function lngToTileX(lng: number, z: number): number {
  return ((lng + 180) / 360) * 2 ** z;
}

export function latToTileY(lat: number, z: number): number {
  const clamped = Math.min(Math.max(lat, -MAX_MERCATOR_LAT), MAX_MERCATOR_LAT);
  const sin = Math.sin((clamped * Math.PI) / 180);
  return ((0.5 - Math.log((1 + sin) / (1 - sin)) / (4 * Math.PI)) * 2 ** z);
}

/** 让外接矩形尽量塞进视口：返回中心点与整数缩放级别。padding 为四周留白（像素）。 */
export function fitBounds(
  bounds: [LatLng, LatLng],
  width: number,
  height: number,
  padding = 48,
): { center: LatLng; zoom: number } {
  const usableW = Math.max(width - padding * 2, 64);
  const usableH = Math.max(height - padding * 2, 64);

  let zoom = 19;
  for (let z = 19; z >= 3; z--) {
    const a = project(bounds[0], z);
    const b = project(bounds[1], z);
    if (Math.abs(a.x - b.x) <= usableW && Math.abs(a.y - b.y) <= usableH) {
      zoom = z;
      break;
    }
    zoom = z;
  }

  const centerPx = {
    x: (project(bounds[0], zoom).x + project(bounds[1], zoom).x) / 2,
    y: (project(bounds[0], zoom).y + project(bounds[1], zoom).y) / 2,
  };
  return { center: unproject(centerPx, zoom), zoom };
}

export function visibleTiles(
  center: LatLng,
  zoom: number,
  width: number,
  height: number,
): Array<{ z: number; x: number; y: number; sx: number; sy: number }> {
  const origin = project(center, zoom);
  const left = origin.x - width / 2;
  const top = origin.y - height / 2;

  const minX = Math.floor(left / TILE_SIZE);
  const maxX = Math.floor((left + width) / TILE_SIZE);
  const minY = Math.floor(top / TILE_SIZE);
  const maxY = Math.floor((top + height) / TILE_SIZE);

  const max = 2 ** Math.round(zoom);
  const out: Array<{ z: number; x: number; y: number; sx: number; sy: number }> = [];
  for (let y = minY; y <= maxY; y++) {
    for (let x = minX; x <= maxX; x++) {
      // 越过经度边界的瓦片用取模回绕（世界是环形的）；纬度越界直接丢弃。
      if (y < 0 || y >= max) continue;
      const wx = ((x % max) + max) % max;
      out.push({ z: Math.round(zoom), x: wx, y, sx: x * TILE_SIZE - left, sy: y * TILE_SIZE - top });
    }
  }
  return out;
}
