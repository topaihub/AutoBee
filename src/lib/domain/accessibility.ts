import type { Route } from "../ipc/types";

export type TravelProfile = "standard" | "luggage" | "slow" | "fewTransfers" | "lowWalk";

export type RouteBurden = {
  duration: number;
  walkDistance: number;
  longestWalk: number;
  transfers: number;
};

/** 出行画像。名字只承诺现有数据支撑的东西（§6.3）：`slow` 不叫「老人 / 行动缓慢」，因为路线数据里没有电梯、台阶、坡道、路缘。 */
export const PROFILE_OPTIONS: Array<{ value: TravelProfile; label: string }> = [
  { value: "standard", label: "普通出行" },
  { value: "luggage", label: "行李 / 婴儿车" },
  { value: "slow", label: "少走长段" },
  { value: "fewTransfers", label: "少换乘" },
  { value: "lowWalk", label: "低步行" },
];

// §6.2 的「少换乘」「低步行」要依次升序，不是加权求和 —— 无论权重多大，第二项的差值总能翻盘第一项。
// 链尾补 `duration` 保证全序，否则前面各项相等时次序随输入漂移。
const PROFILE_ORDER: Partial<Record<TravelProfile, Array<keyof RouteBurden>>> = {
  fewTransfers: ["transfers", "walkDistance", "duration"],
  lowWalk: ["walkDistance", "longestWalk", "transfers", "duration"],
};

export type DecisionLabel = { text: string; tone: "positive" | "warning" };

/** §4.2 的两个提醒阈值（默认取原文保守值）。作为参数传入是因为 §4.2 要求阈值可配置。 */
export type LabelThresholds = { longWalkMeters: number; manyTransfers: number };

export const DEFAULT_LABEL_THRESHOLDS: LabelThresholds = { longWalkMeters: 500, manyTransfers: 2 };

/**
 * §4.2 的六个决策标签。前三个是候选集内的相对判断，故入口收 `Route[]`。原文「低步行」的「总步行**或**
 * 最长单段最低」与「相对标签只出现在一张卡片上」冲突，故取单一胜者：总步行为主，最长单段做 tie-break。
 * 「直达优先」是绝对条件，用 `transfers === 0` 判定 —— 同一条线被切成多段时分段数会大于 1。
 */
export function decisionLabels(
  routes: Route[],
  thresholds: LabelThresholds = DEFAULT_LABEL_THRESHOLDS,
): Record<string, DecisionLabel[]> {
  const ranked = routes.map((route) => ({ id: route.id, burden: routeBurden(route) }));
  const best = (pick: (item: (typeof ranked)[number]) => number[]): Set<string> => {
    if (ranked.length === 0) return new Set();
    const keyed = ranked.map((item) => ({ id: item.id, key: pick(item) }));
    const cmp = (a: number[], b: number[]): number => {
      for (let i = 0; i < a.length; i += 1) {
        if (a[i] !== b[i]) return a[i] - b[i];
      }
      return 0;
    };
    const min = keyed.reduce((acc, item) => (cmp(item.key, acc) < 0 ? item.key : acc), keyed[0].key);
    return new Set(keyed.filter((item) => cmp(item.key, min) === 0).map((item) => item.id));
  };

  const lowWalk = best((item) => [item.burden.walkDistance, item.burden.longestWalk]);
  // 「少换乘」不加 tie-break：换乘数相同的候选本就该一起拿到这个标签。
  const fewTransfers = best((item) => [item.burden.transfers]);
  const fastest = best((item) => [item.burden.duration, item.burden.walkDistance]);

  return Object.fromEntries(
    ranked.map(({ id, burden }) => {
      const labels: DecisionLabel[] = [];
      if (lowWalk.has(id)) labels.push({ text: "步行负担最低", tone: "positive" });
      if (fewTransfers.has(id)) labels.push({ text: "换乘次数最少", tone: "positive" });
      if (burden.transfers === 0) labels.push({ text: "路线更直接", tone: "positive" });
      if (fastest.has(id)) labels.push({ text: "预计最快到达", tone: "positive" });
      if (burden.longestWalk > thresholds.longWalkMeters) {
        labels.push({ text: "含较长步行，请留意体力与行李", tone: "warning" });
      }
      if (burden.transfers > thresholds.manyTransfers) {
        labels.push({ text: "换乘较多，建议预留时间", tone: "warning" });
      }
      return [id, labels];
    }),
  );
}

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

// 越小越好。`fewTransfers` / `lowWalk` 的排序走 `PROFILE_ORDER` 不经过这里，但两者仍要有明确权重：
// 这个函数是导出的，静默落到 standard 会让调用方拿到与排序不一致的分数。
export function burdenScore(burden: RouteBurden, profile: TravelProfile): number {
  const weights = profile === "luggage"
    ? { walk: 1.7, longest: 1.5, transfers: 1.2, duration: 0.1 }
    : profile === "slow"
      ? { walk: 1.4, longest: 1.8, transfers: 1.6, duration: 0.2 }
      : profile === "fewTransfers"
        ? { walk: 0.8, longest: 0.5, transfers: 3, duration: 0.2 }
        : profile === "lowWalk"
          ? { walk: 2.5, longest: 1.5, transfers: 0.6, duration: 0.1 }
          : { walk: 1, longest: 1, transfers: 1, duration: 0.3 };
  return burden.duration * weights.duration
    + burden.walkDistance * weights.walk
    + burden.longestWalk * weights.longest
    + burden.transfers * 500 * weights.transfers;
}

export function sortedRoutes(routes: Route[], profile: TravelProfile): Route[] {
  const keys = PROFILE_ORDER[profile];
  const ranked = routes.map((route) => ({ route, burden: routeBurden(route) }));
  if (keys) {
    return ranked
      .sort((a, b) => {
        for (const key of keys) {
          const diff = a.burden[key] - b.burden[key];
          if (diff !== 0) return diff;
        }
        return 0;
      })
      .map(({ route }) => route);
  }
  return ranked
    .map(({ route, burden }) => ({ route, score: burdenScore(burden, profile) }))
    .sort((a, b) => a.score - b.score || a.route.duration - b.route.duration)
    .map(({ route }) => route);
}

export function burdenReason(
  route: Route,
  profile: TravelProfile,
  thresholds: LabelThresholds = DEFAULT_LABEL_THRESHOLDS,
): string {
  const burden = routeBurden(route);
  // 解释文案必须落在该画像的排序依据上，否则文案与排序脱节。
  if (profile === "fewTransfers") {
    return `换乘 ${burden.transfers} 次，总步行 ${Math.round(burden.walkDistance)} 米`;
  }
  if (profile === "lowWalk") {
    return `总步行 ${Math.round(burden.walkDistance)} 米，最长单段 ${Math.round(burden.longestWalk)} 米`;
  }
  if (burden.longestWalk >= thresholds.longWalkMeters) {
    return `最长步行 ${Math.round(burden.longestWalk)} 米，建议预留体力`;
  }
  if (burden.transfers > 1) return `${burden.transfers} 次换乘，${profile === "standard" ? "时间可能更优" : "请确认换乘通道"}`;
  if (burden.walkDistance === 0) return "无步行段数据";
  return `总步行 ${Math.round(burden.walkDistance)} 米，换乘 ${burden.transfers} 次`;
}
