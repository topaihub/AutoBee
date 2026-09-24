import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Route } from "../ipc/types";
import { DEFAULT_LABEL_THRESHOLDS, PROFILE_OPTIONS, burdenReason, decisionLabels, routeBurden, sortedRoutes } from "./accessibility";
import baiduNormalized from "./__fixtures__/baidu-transit-normalized.json";
import directFixture from "./__fixtures__/baidu-transit-direct.json";
import manyTransfersFixture from "./__fixtures__/baidu-transit-many-transfers.json";

function route(id: string, duration: number, walks: number[], lines: string[] = []): Route {
  return {
    id,
    distance: 1000,
    duration,
    price: 0,
    bounds: [{ lat: 30.5, lng: 114.4 }, { lat: 30.6, lng: 114.5 }],
    segments: [
      ...walks.map((distance) => ({ kind: "walk" as const, distance, duration: 1, points: [] })),
      ...lines.map((title) => ({
        kind: "transit" as const,
        distance: 1000,
        duration: 10,
        points: [],
        line: {
          vehicle: "SUBWAY" as const,
          title,
          stationCount: 1,
          color: "#000",
          getOn: { title: "A", location: { lat: 30.5, lng: 114.4 } },
          getOff: { title: "B", location: { lat: 30.6, lng: 114.5 } },
        },
      })),
    ],
  };
}

describe("accessibility burden", () => {
  it("counts walking distance, longest walk, and line changes", () => {
    expect(routeBurden(route("a", 20, [200, 600], ["1号线", "2号线"]))).toMatchObject({
      duration: 20,
      walkDistance: 800,
      longestWalk: 600,
      transfers: 1,
    });
  });

  it("moves lower-walk route ahead for luggage profile", () => {
    const routes = [route("fast", 20, [900]), route("easy", 30, [100])];
    expect(sortedRoutes(routes, "luggage").map((item) => item.id)).toEqual(["easy", "fast"]);
  });

  it("warns about a long walking segment", () => {
    expect(burdenReason(route("a", 20, [600]), "slow")).toContain("最长步行 600 米");
  });

  it("少换乘画像下换乘少者胜，即使它步行远得多", () => {
    // 这一条是字典序与加权和的分水岭:换乘与步行给出相反的结论。
    // 加权和里 2600 米的步行差足以翻盘一次换乘，字典序不会。
    const routes = [
      route("walkLess", 40, [100], ["1号线", "2号线", "4号线"]), // 换乘 2，步行 100
      route("transferLess", 50, [2700], ["1号线"]),              // 换乘 0，步行 2700
    ];
    expect(sortedRoutes(routes, "fewTransfers").map((item) => item.id)).toEqual(["transferLess", "walkLess"]);
  });

  it("低步行画像下步行少者胜，即使它换乘更多", () => {
    const routes = [
      route("walkLess", 40, [100], ["1号线", "2号线", "4号线"]),
      route("transferLess", 50, [2700], ["1号线"]),
    ];
    expect(sortedRoutes(routes, "lowWalk").map((item) => item.id)).toEqual(["walkLess", "transferLess"]);
  });

  it("第一项打平时由后续项决定，不靠输入顺序", () => {
    // 两条换乘同为 1，次序必须由第二项总步行决定。
    // 故意把步行多的放前面:若比较器在第一项就 return，稳定排序会原样保留
    // 错误次序，而这正是「丢掉 tie-break」这个 bug 的样子。
    const routes = [
      route("moreWalk", 40, [900], ["1号线", "2号线"]),
      route("lessWalk", 60, [200], ["1号线", "2号线"]),
    ];
    expect(routeBurden(routes[0]).transfers).toBe(routeBurden(routes[1]).transfers);
    expect(sortedRoutes(routes, "fewTransfers").map((item) => item.id)).toEqual(["lessWalk", "moreWalk"]);
  });
});

/**
 * 真实百度响应过 `BaiduProvider` 归一化后的样本（天安门东 → 国家图书馆，
 * `tactics_incity=5`）。上面那些用的是构造数据，构造数据证明不了 adapter
 * 把 `accessibility.ts` 真正读的四个字段填对了 —— 字段填错时评分会静默变成
 * 0 或常数，而构造数据永远填得对。
 *
 * 生成方式：抓真实响应 → 过 `baidu.rs` 的 `transit()` → 序列化。
 * `points` 已清空（不参与评分），其余字段逐字保留。
 *
 * JSON 导入会把 `kind` 宽化成 string、`bounds` 宽化成数组（不是二元组），
 * 直转 `Route[]` 会被 TS 拒。但 `as unknown as Route[]` 会把真实的形状错配
 * 一起藏掉，那正好废掉这个 fixture 的意义 —— 所以显式重建成 Route 形状，
 * 顺带在运行时校验 `kind` 只有 walk / transit 两种取值。
 */
const realRoutes: Route[] = baiduNormalized.map((raw) => {
  const [southWest, northEast] = raw.bounds;
  expect(southWest).toBeDefined();
  expect(northEast).toBeDefined();
  return {
    ...raw,
    bounds: [southWest, northEast],
    segments: raw.segments.map((segment) => {
      expect(segment.kind === "walk" || segment.kind === "transit").toBe(true);
      return segment as Route["segments"][number];
    }),
  } satisfies Route;
});

describe("百度真实数据上的负担指标", () => {
  const routes = realRoutes;

  it("adapter 填对了评分要读的四个字段", () => {
    // 三段步行 + 两段地铁。若 kind 填错，walks 会是空数组、总步行变 0。
    expect(routeBurden(routes[0])).toMatchObject({
      duration: 68,
      walkDistance: 3009,
      longestWalk: 1345,
      transfers: 1,
    });
  });

  it("三条候选的步行负担确实不同，不是常数", () => {
    // 字段没填对最典型的表现是三条全一样，那时排序变成无意义的稳定排序。
    expect(routes.map((route) => routeBurden(route).walkDistance)).toEqual([3009, 3107, 3215]);
  });

  it("换乘次数按线路名变化计数，同线不算换乘", () => {
    const titles = routes[0].segments.flatMap((segment) => segment.line?.title ?? []);
    expect(titles).toEqual(["地铁1号线八通线", "地铁9号线"]);
    expect(routeBurden(routes[0]).transfers).toBe(1);
  });

  it("行李画像把步行最少的排到最前", () => {
    expect(sortedRoutes(routes, "luggage").map((route) => route.id)).toEqual(["r0", "r1", "r2"]);
  });

  it("同一组输入连排两次结果完全一致", () => {
    const first = sortedRoutes(routes, "slow").map((route) => route.id);
    const second = sortedRoutes(routes, "slow").map((route) => route.id);
    expect(first).toEqual(second);
  });

  it("少换乘画像按 transfers → walk → duration 依次升序", () => {
    // 三条换乘次数都是 1，所以次序由第二项总步行决定：3009 < 3107 < 3215。
    expect(sortedRoutes(routes, "fewTransfers").map((route) => route.id)).toEqual(["r0", "r1", "r2"]);
  });

  it("低步行画像按 walk → maxWalk → transfers 依次升序", () => {
    expect(sortedRoutes(routes, "lowWalk").map((route) => route.id)).toEqual(["r0", "r1", "r2"]);
  });

  it("五个画像的解释文案各自落在自己的排序依据上", () => {
    // 判据要求切画像文案可见变化。两个新画像不能复用「请确认换乘通道」，
    // 那句与它们的排序依据无关。
    expect(burdenReason(routes[0], "fewTransfers")).toBe("换乘 1 次，总步行 3009 米");
    expect(burdenReason(routes[0], "lowWalk")).toBe("总步行 3009 米，最长单段 1345 米");
    expect(burdenReason(routes[0], "standard")).toContain("最长步行 1345 米");
  });

  it("真实数据的最长步行超过阈值时给出提醒", () => {
    expect(burdenReason(routes[0], "luggage")).toContain("最长步行 1345 米");
  });

  it("调高阈值后同一条路线不再给长步行提醒", () => {
    // §4.2 要求阈值可配置。真实那条最长单段 1345 米：阈值 500 时提醒，
    // 调到 2000 就该改口，否则说明函数在拿写死的 500 比。
    const reason = burdenReason(routes[0], "luggage", { longWalkMeters: 2000, manyTransfers: 2 });
    expect(reason).not.toContain("建议预留体力");
  });
});

describe("阈值默认值前后端同源", () => {
  it("前端默认值与 Rust 侧的常量一致", () => {
    // 分叉的后果是静默的：设置页显示 500、后端存 800，改哪一边都看不出来。
    // 照 `out_crs` 那处的做法（三处默认值不许分叉）从源码里读真值比。
    const rust = readFileSync("src-tauri/shell/src/credentials/mod.rs", "utf8");
    const walk = /DEFAULT_LONG_WALK_METERS: u32 = (\d+)/.exec(rust);
    const transfers = /DEFAULT_MANY_TRANSFERS: u32 = (\d+)/.exec(rust);
    expect(Number(walk?.[1])).toBe(DEFAULT_LABEL_THRESHOLDS.longWalkMeters);
    expect(Number(transfers?.[1])).toBe(DEFAULT_LABEL_THRESHOLDS.manyTransfers);
  });
});

describe("§6.3 诚信边界", () => {
  /** 界面上出现这些词就等于承诺了没有数据支撑的事实。 */
  const FORBIDDEN = ["全程无障碍", "无障碍通行", "轮椅可通行", "有电梯", "无台阶", "保证无障碍"];

  it("面向用户的文案不承诺未接入的设施事实", () => {
    // 出现承诺性说法的后果不是措辞问题:推轮椅的人会照它走。
    // 只看给用户的文案 —— 注释里解释「数据里没有电梯」不算承诺。
    const files = [
      "src/lib/components/app/RouteCard.svelte",
      "src/routes/+page.svelte",
      "src/lib/domain/accessibility.ts",
    ];
    for (const file of files) {
      const text = readFileSync(file, "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .replace(/(^|[^:])\/\/.*$/gm, "$1")
        .replace(/<!--[\s\S]*?-->/g, "");
      for (const word of FORBIDDEN) {
        expect(text, `${file} 出现承诺性说法「${word}」`).not.toContain(word);
      }
    }
  });

  it("画像名字不声称理解行动能力", () => {
    // `slow` 排序只用得到步行距离与换乘次数，名字不能暗示它看过台阶或电梯。
    const labels = PROFILE_OPTIONS.map((item) => item.label).join(" ");
    for (const word of ["老人", "行动缓慢", "无障碍"]) {
      expect(labels, `画像名里有「${word}」但没有对应数据`).not.toContain(word);
    }
  });

  it("六个标签都能在真实百度数据上出现", () => {
    // 判据要求每个标签在真实场景里出现过一次 —— 定义了但永不触发的标签
    // 等于没有，而单测里手搓的路线证明不了百度真会返回那种形状。
    // 直达那份来自王府井→东单（纯公交、0 换乘），多换乘那份来自回龙观→亦庄。
    const seen = new Set<string>();
    for (const raw of [directFixture, manyTransfersFixture]) {
      const routes = raw as unknown as Route[];
      for (const labels of Object.values(decisionLabels(routes))) {
        for (const label of labels) seen.add(label.text);
      }
    }
    expect([...seen].sort()).toEqual(
      [
        "含较长步行，请留意体力与行李",
        "换乘较多，建议预留时间",
        "换乘次数最少",
        "步行负担最低",
        "路线更直接",
        "预计最快到达",
      ].sort(),
    );
  });
});

describe("§4.2 决策标签", () => {
  const texts = (route: Route, all: Route[]): string[] =>
    decisionLabels(all)[route.id]?.map((label) => label.text) ?? [];

  it("相对标签在候选集里唯一，不会同时贴给两条路线", () => {
    const routes = [
      route("slowShortWalk", 60, [100], ["1号线", "2号线"]),
      route("fastLongWalk", 20, [900], ["1号线", "2号线"]),
    ];
    expect(texts(routes[0], routes)).toContain("步行负担最低");
    expect(texts(routes[1], routes)).not.toContain("步行负担最低");
    expect(texts(routes[1], routes)).toContain("预计最快到达");
    expect(texts(routes[0], routes)).not.toContain("预计最快到达");
  });

  it("同分时相对标签才允许同时出现", () => {
    const routes = [route("a", 30, [300], ["1号线"]), route("b", 30, [300], ["4号线"])];
    expect(texts(routes[0], routes)).toContain("步行负担最低");
    expect(texts(routes[1], routes)).toContain("步行负担最低");
  });

  it("换乘数打平时三条都拿到少换乘标签，不靠步行压成唯一", () => {
    // 真实百度候选就是这个形状：三条换乘全是 1。给它加 tie-break 会让标签
    // 只剩一条，与原文「乘车线路变更次数最低」不符。
    const routes = [
      route("a", 30, [100], ["1号线", "2号线"]),
      route("b", 40, [500], ["1号线", "4号线"]),
      route("c", 50, [900], ["1号线", "9号线"]),
    ];
    for (const item of routes) expect(routeBurden(item).transfers).toBe(1);
    for (const item of routes) expect(texts(item, routes)).toContain("换乘次数最少");
  });

  it("单条线路拿到直达标签，换乘的拿不到", () => {
    const routes = [route("direct", 30, [200], ["1号线"]), route("transfer", 30, [200], ["1号线", "2号线"])];
    expect(texts(routes[0], routes)).toContain("路线更直接");
    expect(texts(routes[1], routes)).not.toContain("路线更直接");
  });

  it("同一条线被切成多段仍算直达", () => {
    // 按乘车分段数判定会在这里丢标签：两段但同线，换乘次数仍是 0。
    const routes = [route("split", 30, [200], ["1号线", "1号线"])];
    expect(routeBurden(routes[0]).transfers).toBe(0);
    expect(texts(routes[0], routes)).toContain("路线更直接");
  });

  it("两个提醒标签按阈值触发，等于阈值时不触发", () => {
    const atThreshold = [route("at", 30, [500], ["1号线", "2号线", "4号线"])];
    expect(texts(atThreshold[0], atThreshold)).not.toContain("含较长步行，请留意体力与行李");
    expect(texts(atThreshold[0], atThreshold)).not.toContain("换乘较多，建议预留时间");

    const over = [route("over", 30, [501], ["1号线", "2号线", "4号线", "9号线"])];
    expect(texts(over[0], over)).toContain("含较长步行，请留意体力与行李");
    expect(texts(over[0], over)).toContain("换乘较多，建议预留时间");
  });

  it("阈值可由调用方覆盖，不写死在实现里", () => {
    const routes = [route("a", 30, [300], ["1号线", "2号线"])];
    const strict = decisionLabels(routes, { longWalkMeters: 200, manyTransfers: 0 });
    expect(strict[routes[0].id]?.map((label) => label.text)).toEqual(
      expect.arrayContaining(["含较长步行，请留意体力与行李", "换乘较多，建议预留时间"]),
    );
  });

  it("空候选集返回空表而不抛错", () => {
    expect(decisionLabels([])).toEqual({});
  });

  it("真实百度候选上，最快与最低步行落在同一条且只贴一次", () => {
    const labels = decisionLabels(realRoutes);
    const holders = realRoutes.filter((item) => labels[item.id]?.some((label) => label.text === "步行负担最低"));
    // r0 的总步行 3009 是三条里最低的，duration 68 也是最低的。
    expect(holders.map((item) => item.id)).toEqual(["r0"]);
    expect(labels["r0"]?.map((label) => label.text)).toContain("预计最快到达");
    // 三条换乘都是 1，「换乘次数最少」按原文该三条都有。
    for (const item of realRoutes) {
      expect(labels[item.id]?.map((label) => label.text)).toContain("换乘次数最少");
    }
    // 1 次换乘未超过默认阈值 2，所以没有「换乘较多」。
    expect(labels["r0"]?.map((label) => label.text)).not.toContain("换乘较多，建议预留时间");
  });
});
