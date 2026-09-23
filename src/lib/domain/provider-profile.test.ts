import { describe, expect, it } from "vitest";
import type { MapProvider } from "../ipc/types";
import {
  PROVIDER_DOT,
  priorityLabel,
  providerState,
  providerSummary,
  sortProviders,
  testDisabledReason,
} from "./provider-profile";

function provider(overrides: Partial<MapProvider>): MapProvider {
  return {
    slug: "tencent",
    name: "腾讯位置服务",
    isPreset: true,
    enabled: true,
    defaultPriority: 1,
    coverage: "CN",
    coordSys: "GCJ02",
    capabilities: "公交 · 地铁 · 步行",
    apiKeyMasked: "",
    implemented: true,
    keyApplyUrl: "https://lbs.qq.com/dev/console/application/mine",
    ...overrides,
  };
}

describe("providerState", () => {
  it("没填 Key 是 empty", () => {
    expect(providerState(provider({ apiKeyMasked: "" }))).toBe("empty");
  });

  it("填了 Key 是 untested（能用与否要测过才知道）", () => {
    expect(providerState(provider({ apiKeyMasked: "FAKE********TEST" }))).toBe("untested");
  });

  it("三个状态各有一个色点，不重复", () => {
    const dots = Object.values(PROVIDER_DOT);
    expect(new Set(dots).size).toBe(3);
  });
});

describe("testDisabledReason", () => {
  it("没填 Key 时给出原因", () => {
    expect(testDisabledReason(provider({ apiKeyMasked: "" }))).not.toBe("");
  });

  it("填了 Key 就能测", () => {
    expect(testDisabledReason(provider({ apiKeyMasked: "FAKE********TEST" }))).toBe("");
  });

  it("没实现的即使填了 Key 也测不了", () => {
    expect(testDisabledReason(provider({ apiKeyMasked: "abc", implemented: false }))).not.toBe("");
  });
});

describe("priorityLabel", () => {
  it("默认优先级不加「已调整」", () => {
    const p = provider({ defaultPriority: 1, priority: undefined });
    expect(priorityLabel(p)).toBe("优先级 1");
  });

  it("用户改过的显示两个值", () => {
    const p = provider({ defaultPriority: 1, priority: 5 });
    expect(priorityLabel(p)).toBe("优先级 5（默认 1）");
  });
});

describe("sortProviders", () => {
  it("预置在前，其余按 slug", () => {
    const list = [
      provider({ slug: "google", isPreset: false }),
      provider({ slug: "here", isPreset: true }),
      provider({ slug: "amap", isPreset: true }),
    ];
    expect(sortProviders(list).map((p) => p.slug)).toEqual(["amap", "here", "google"]);
  });

  it("不改入参", () => {
    const list = [provider({ slug: "baidu" }), provider({ slug: "amap" })];
    sortProviders(list);
    expect(list.map((p) => p.slug)).toEqual(["baidu", "amap"]);
  });
});

describe("providerSummary", () => {
  it("拼出可读的一行", () => {
    expect(providerSummary(provider({}))).toBe("CN · GCJ02 · 公交 · 地铁 · 步行");
  });
});
