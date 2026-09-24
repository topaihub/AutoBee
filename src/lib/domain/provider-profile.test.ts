import { describe, expect, it } from "vitest";
import type { MapProvider } from "../ipc/types";
import {
  PROVIDER_DOT,
  priorityLabel,
  providerState,
  providerSummary,
  partialKeyHint,
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
    browserKeyMasked: "",
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

  it("百度只填服务端 AK 是 partial", () => {
    // 只填一把时显示「已配置」会让用户以为设置完了，而地图是空白的。
    expect(
      providerState(provider({ slug: "baidu", apiKeyMasked: "Cz2b****z26a", browserKeyMasked: "" })),
    ).toBe("partial");
  });

  it("百度只填浏览器 AK 也是 partial", () => {
    expect(
      providerState(provider({ slug: "baidu", apiKeyMasked: "", browserKeyMasked: "Cz2b****z26a" })),
    ).toBe("partial");
  });

  it("百度两把都填了才是 untested", () => {
    expect(
      providerState(
        provider({ slug: "baidu", apiKeyMasked: "Cz2b****z26a", browserKeyMasked: "AbCd****9xYz" }),
      ),
    ).toBe("untested");
  });

  it("非百度厂商填一把就够，不会被判 partial", () => {
    // browserKeyMasked 对其余厂商恒为空串，不该因此报半残。
    expect(
      providerState(provider({ slug: "tencent", apiKeyMasked: "FAKE****TEST", browserKeyMasked: "" })),
    ).toBe("untested");
  });

  it("每个状态都有对应色点", () => {
    // 漏一个的后果是那种状态的点不显示，用户看不出档案有问题。
    for (const state of ["ready", "untested", "empty", "partial"] as const) {
      expect(PROVIDER_DOT[state]).toBeTruthy();
    }
  });
});

describe("partialKeyHint", () => {
  it("缺浏览器 AK 时说清缺的是哪一把与后果", () => {
    const hint = partialKeyHint(
      provider({ slug: "baidu", apiKeyMasked: "Cz2b****z26a", browserKeyMasked: "" }),
    );
    expect(hint).toContain("浏览器");
    expect(hint).toContain("地图");
  });

  it("缺服务端 AK 时指向搜索与路线", () => {
    const hint = partialKeyHint(
      provider({ slug: "baidu", apiKeyMasked: "", browserKeyMasked: "Cz2b****z26a" }),
    );
    expect(hint).toContain("服务端");
    expect(hint).toContain("路线");
  });

  it("状态正常时不出提示", () => {
    expect(partialKeyHint(provider({ apiKeyMasked: "FAKE****TEST" }))).toBe("");
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
