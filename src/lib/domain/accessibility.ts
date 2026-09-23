import type { Route } from "../ipc/types";

/** AccessMap-inspired user profiles, using only facts present in current route data. */
export type TravelProfile = "standard" | "luggage" | "slow";

export type RouteBurden = {
  duration: number;
  walkDistance: number;
  longestWalk: number;
  transfers: number;
};

export const PROFILE_OPTIONS: Array<{ value: TravelProfile; label: string }> = [
  { value: "standard", label: "普通出行" },
  { value: "luggage", label: "行李 / 婴儿车" },
  { value: "slow", label: "老人 / 行动缓慢" },
];

/**
 * Extracts observable burden only. Missing elevator/curb/slope data is not guessed.
 */
export function routeBurden(route: Route): RouteBurden {
  const walks = route.segments.filter((segment) => segment.kind === "walk");
  const transit = route.segments.filter((segment) => segment.kind === "transit");
  const lines = transit.map((segment) => segment.line?.title).filter(Boolean);
  const transfers = lines.reduce((count, line, index) => count + Number(index > 0 && line !== lines[index - 1]), 0);
  return {
    duration: route.duration,
    walkDistance: walks.reduce((sum, segment) => sum + segment.distance, 0),
    longestWalk: walks.reduce((max, segment) => Math.max(max, segment.distance), 0),
    transfers,
  };
}

/** Lower is better. Weights deliberately favor walking/transfer burden by profile. */
export function burdenScore(burden: RouteBurden, profile: TravelProfile): number {
  const weights = profile === "luggage"
    ? { walk: 1.7, longest: 1.5, transfers: 1.2, duration: 0.1 }
    : profile === "slow"
      ? { walk: 1.4, longest: 1.8, transfers: 1.6, duration: 0.2 }
      : { walk: 1, longest: 1, transfers: 1, duration: 0.3 };
  return burden.duration * weights.duration
    + burden.walkDistance * weights.walk
    + burden.longestWalk * weights.longest
    + burden.transfers * 500 * weights.transfers;
}

export function sortedRoutes(routes: Route[], profile: TravelProfile): Route[] {
  return routes
    .map((route) => {
      const burden = routeBurden(route);
      return { route, score: burdenScore(burden, profile) };
    })
    .sort((a, b) => a.score - b.score || a.route.duration - b.route.duration)
    .map(({ route }) => route);
}

export function burdenReason(route: Route, profile: TravelProfile): string {
  const burden = routeBurden(route);
  if (burden.longestWalk >= 500) return `最长步行 ${Math.round(burden.longestWalk)} 米，建议预留体力`;
  if (burden.transfers > 1) return `${burden.transfers} 次换乘，${profile === "standard" ? "时间可能更优" : "请确认换乘通道"}`;
  if (burden.walkDistance === 0) return "无步行段数据";
  return `总步行 ${Math.round(burden.walkDistance)} 米，换乘 ${burden.transfers} 次`;
}
