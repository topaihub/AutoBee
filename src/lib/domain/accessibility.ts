import type { Route } from "../ipc/types";

/** AccessMap-inspired user profiles, using only facts present in current route data. */
export type TravelProfile = "standard" | "luggage" | "slow" | "fewTransfers" | "lowWalk";

export type RouteBurden = {
  duration: number;
  walkDistance: number;
  longestWalk: number;
  transfers: number;
};

/**
 * 出行画像。名字只许承诺现有数据能支撑的东西 —— §6.3。
 *
 * `slow` **不叫「老人 / 行动缓慢」**:那个名字暗示应用懂行动能力，而这里能看到的
 * 只有步行距离、最长单段与换乘次数（见 [`RouteBurden`]），电梯、台阶、坡道、
 * 路缘一个字段都没有。按那个名字选路的人会以为结果考虑过台阶，实际没有。
 * 接入设施数据前，名字只说它真正在做的事：压低单段步行。
 */
export const PROFILE_OPTIONS: Array<{ value: TravelProfile; label: string }> = [
  { value: "standard", label: "普通出行" },
  { value: "luggage", label: "行李 / 婴儿车" },
  { value: "slow", label: "少走长段" },
  { value: "fewTransfers", label: "少换乘" },
  { value: "lowWalk", label: "低步行" },
];

/**
 * §6.2 的「少换乘」「低步行」要的是**依次升序**，不是加权求和。
 *
 * 加权和做不出字典序：无论权重多大，第二项的差值总能翻盘第一项。所以这两个
 * 画像走独立的比较器链，而不是往 `burdenScore` 里塞更极端的权重。
 * 每条链末尾都补 `duration`，保证全序 —— 前面各项全相等时仍有确定次序，
 * 否则排序结果会随输入顺序漂移。
 */
const PROFILE_ORDER: Partial<Record<TravelProfile, Array<keyof RouteBurden>>> = {
  // §6.2：transfers、walk_m、duration_min 依次升序。
  fewTransfers: ["transfers", "walkDistance", "duration"],
  // §6.2：walk_m、max_walk_m、transfers 依次升序。
  lowWalk: ["walkDistance", "longestWalk", "transfers", "duration"],
};

/** 一个决策标签。`warning` 用于提醒类，`positive` 用于优势类。 */
export type DecisionLabel = { text: string; tone: "positive" | "warning" };

/**
 * §4.2 的两个提醒阈值。默认值按原文取保守值（500 米 / 2 次换乘）。
 *
 * 作为参数传入而不是读全局，是因为 §4.2 末尾要求阈值可配置；调用方决定从哪里取。
 */
export type LabelThresholds = { longWalkMeters: number; manyTransfers: number };

export const DEFAULT_LABEL_THRESHOLDS: LabelThresholds = { longWalkMeters: 500, manyTransfers: 2 };

/**
 * §4.2 的六个决策标签。
 *
 * 前三个是**候选集内的相对判断**,必须看到整组路线才算得出,所以入口收 `Route[]`
 * 而不是单条 —— 卡片自己算不出「在候选中最低」。
 *
 * 原文「低步行」的触发条件写的是「总步行**或**最长单段步行在候选中最低」。
 * 这个「或」与判据「相对标签只出现在一张卡片上」冲突：两项的最低值常常落在
 * 不同路线上，那就会贴出两个「步行负担最低」。这里按判据取单一胜者 ——
 * 总步行为主，最长单段做 tie-break。真同分时才会有多张卡同时命中。
 *
 * 「直达优先」按原文是绝对条件（无公共交通换乘），不是相对比较，所以允许多张卡
 * 同时命中 —— 判据点名要求唯一的只有「低步行」与「时间最短」。
 * 用 `transfers === 0` 判定，而不是数乘车分段数：同一条线被切成多段时分段数会
 * 大于 1，但那仍是全程单线，标签不该丢。
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
  // 「少换乘」不加 tie-break：原文的触发条件就是「乘车线路变更次数最低」，
  // 换乘数相同的候选本就该一起拿到这个标签。判据点名要求唯一的只有
  // 「低步行」与「时间最短」。
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

/**
 * Lower is better. Weights deliberately favor walking/transfer burden by profile.
 *
 * `fewTransfers` / `lowWalk` 的排序走 `PROFILE_ORDER` 的字典序，不经过这里。
 * 但这个函数是导出的（解释文案与卡片都可能拿它做「分数差」），所以两者仍要有
 * 明确权重，不能静默落到 standard —— 那会让调用方拿到与排序不一致的分数。
 */
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
    // 字典序：逐项比，第一个不等的项决定次序。
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
  // 这两个画像是按自己那一项排出来的，解释就得落在那一项上，否则文案与
  // 排序依据脱节（判据要求「切画像文案可见变化」）。
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
