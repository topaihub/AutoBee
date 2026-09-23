import type { Route, Segment } from "../ipc/types";

/** 步行段的画布颜色（底图上的灰）。与 Rust 侧 WALK_COLOR 一致。 */
export const WALK_COLOR = "#9aa0a6";

/**
 * 一个方案的「换乘摘要」，如 `["2号线", "4号线"]`。
 * 连续多条同名线路不合并 —— 那是厂商给的就是两段（罕见），如实展示。
 */
export function lineNames(route: Route): string[] {
  const names: string[] = [];
  for (const segment of route.segments) {
    if (segment.line && segment.line.title) names.push(segment.line.title);
  }
  return names;
}

/** 方案里纯步行段的总距离（米）。列表卡片上显示「步行 x 米」。 */
export function totalWalkDistance(route: Route): number {
  return route.segments
    .filter((s) => s.kind === "walk")
    .reduce((sum, s) => sum + s.distance, 0);
}

/** 是否包含地铁段。列表卡片上给个「地铁」小标。 */
export function hasSubway(route: Route): boolean {
  return route.segments.some((s) => s.line?.vehicle === "SUBWAY");
}

/** 是否包含公交段。 */
export function hasBus(route: Route): boolean {
  return route.segments.some((s) => s.line?.vehicle === "BUS");
}

/** 画布上要画的全部点：按段着色，调用方逐段描线。 */
export function drawableSegments(segments: Segment[]): Array<{ color: string; kind: Segment["kind"] }> {
  return segments.map((s) => ({
    color: s.line?.color ?? WALK_COLOR,
    kind: s.kind,
  }));
}
