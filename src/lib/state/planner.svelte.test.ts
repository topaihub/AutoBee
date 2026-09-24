import { afterEach, describe, expect, it, vi } from "vitest";
import type { PlannerClient } from "../ipc/client";
import type { Crs, LifeCirclePackage, Place, ReachablePoi, RoutePlan, Settings } from "../ipc/types";
import { PlannerState } from "./planner.svelte";

function place(title: string): Place {
  return { title, location: { lat: 30.5, lng: 114.4 }, provider: "tencent" };
}

function poi(name: string): ReachablePoi {
  return { category: "公园", name, address: "某路 1 号", location: { lat: 31.21, lng: 121.41 }, walkDurationSec: 600, walkDistance: 700 };
}

function plan(): RoutePlan {
  return {
    origin: { title: "a", location: { lat: 30.5, lng: 114.4 } },
    destination: { title: "b", location: { lat: 30.55, lng: 114.27 } },
    mode: "subway",
    routes: [{ id: "r0", distance: 18800, duration: 45, price: 5, bounds: [
      { lat: 30.5, lng: 114.26 },
      { lat: 30.55, lng: 114.43 },
    ], segments: [] }],
    provider: "tencent",
    crs: "WGS84",
  };
}

function fakeClient(overrides: Partial<PlannerClient> = {}): PlannerClient {
  return {
    suggest: vi.fn(async () => [place("光谷大道地铁站")]),
    planRoute: vi.fn(async () => plan()),
    getTile: vi.fn(async () => ""),
    getMapSdkKey: vi.fn(async () => null),
    listProviders: vi.fn(async () => []),
    getLifeCircle: vi.fn(async () => ({ package: "care" as const, crs: "BD09" as const, limitMinutes: 15, pois: [] })),
    saveProvider: vi.fn(async () => []),
    deleteProvider: vi.fn(async () => []),
    testProvider: vi.fn(async () => ({ ok: true, message: "" })),
    getSettings: vi.fn(async (): Promise<Settings> => ({
      autoProvider: true,
      outCrs: "WGS84",
      allowCrossBorder: false,
      tileSource: "arcgis-street",
      defaultProviders: [],
      showLogs: false,
      longWalkMeters: 500,
      manyTransfers: 2,
    })),
    saveSettings: vi.fn(async (s) => s),
    readLogs: vi.fn(async () => []),
    ...overrides,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("PlannerState", () => {
  it("空输入不联想", () => {
    vi.useFakeTimers();
    const client = fakeClient();
    const state = new PlannerState(client);
    state.onInput("from", "");
    vi.advanceTimersByTime(300);
    expect(client.suggest).not.toHaveBeenCalled();
    expect(state.suggesting).toBeNull();
  });

  it("输入一个字符后防抖联想一次", async () => {
    vi.useFakeTimers();
    const client = fakeClient();
    const state = new PlannerState(client);
    state.onInput("from", "武");
    vi.advanceTimersByTime(300);
    await vi.runAllTimersAsync();
    expect(client.suggest).toHaveBeenCalledWith("武");
    expect(state.fromSuggestions.length).toBe(1);
  });

  it("输入防抖后联想一次", async () => {
    vi.useFakeTimers();
    const client = fakeClient();
    const state = new PlannerState(client);
    state.onInput("from", "光谷大道");
    vi.advanceTimersByTime(300);
    await vi.runAllTimersAsync();
    expect(client.suggest).toHaveBeenCalledWith("光谷大道");
    expect(state.fromSuggestions.length).toBe(1);
  });

  it("点选候选锁定坐标，规划时发坐标而不是文本", async () => {
    const client = fakeClient();
    const state = new PlannerState(client);
    const p = place("钟家村-E1口");
    state.pick("to", p);
    state.fromText = "光谷大道地铁站";
    await state.run();
    expect(client.planRoute).toHaveBeenCalledWith("光谷大道地铁站", "30.5,114.4", "subway", "time");
    expect(state.plan?.routes.length).toBe(1);
    expect(state.selectedRouteId).toBe("r0");
  });
  it("已有路线时切换方式会重新规划", async () => {
    const client = fakeClient();
    const state = new PlannerState(client);
    state.fromText = "孝陵卫地铁站";
    state.toText = "南京南站";
    await state.run();
    state.setMode("walking");
    await Promise.resolve();
    expect(client.planRoute).toHaveBeenLastCalledWith("孝陵卫地铁站", "南京南站", "walking", "time");
  });


  it("已规划后按当前坐标系请求生活圈", async () => {
    const getLifeCircle = vi.fn(async () => ({ package: "care" as const, crs: "BD09" as const, limitMinutes: 15, pois: [] }));
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    state.fromText = "孝陵卫地铁站";
    state.toText = "南京南站";
    await state.run();
    await state.loadLifeCircle();
    expect(getLifeCircle).toHaveBeenCalledWith(30.5, 114.4, "WGS84", "care");
  });

  it("生活圈自己选的地点优先于换乘起点，并按候选的 GCJ-02 送出", async () => {
    const getLifeCircle = vi.fn(async () => ({ package: "care" as const, crs: "BD09" as const, limitMinutes: 15, pois: [] }));
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    state.fromText = "孝陵卫地铁站";
    state.toText = "南京南站";
    await state.run();
    state.pick("circle", { title: "新中心", location: { lat: 31.2, lng: 121.4 }, provider: "tencent" });
    await state.loadLifeCircle();
    expect(getLifeCircle).toHaveBeenCalledWith(31.2, 121.4, "GCJ02", "care");
  });

  it("勾选多个服务包时并发查询，每包各一次", async () => {
    const getLifeCircle = vi.fn(async (lat: number, lng: number, crs: Crs, packageType: LifeCirclePackage) => ({
      package: packageType,
      crs: "BD09" as const,
      limitMinutes: 15,
      pois: [],
    }));
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    state.pick("circle", { title: "中心", location: { lat: 31.2, lng: 121.4 }, provider: "tencent" });
    state.togglePackage("family");
    await state.loadLifeCircle();
    expect(getLifeCircle.mock.calls.map((call) => call[3])).toEqual(["care", "family"]);
    expect(state.circleResults.map((result) => result.package)).toEqual(["care", "family"]);
  });

  it("单包失败不影响其余包出结果", async () => {
    const getLifeCircle = vi.fn(async (lat: number, lng: number, crs: Crs, packageType: LifeCirclePackage) => {
      if (packageType === "care") throw { code: "UPSTREAM", message: "百度地点检索失败" };
      return { package: packageType, crs: "BD09" as const, limitMinutes: 15, pois: [poi("社区公园")] };
    });
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    state.pick("circle", { title: "中心", location: { lat: 31.2, lng: 121.4 }, provider: "tencent" });
    state.togglePackage("family");
    await state.loadLifeCircle();
    expect(state.circleResults[0]).toMatchObject({ package: "care", ok: false, message: "百度地点检索失败" });
    expect(state.circleResults[1]).toMatchObject({ package: "family", ok: true });
    expect(state.reachablePois.map((p) => p.name)).toEqual(["社区公园"]);
    // 只有部分失败时不占用顶层错误位 —— 那里是「整件事没成」才用的。
    expect(state.lifeCircleError).toBe("");
  });

  it("全部包同因失败时提示上游原因", async () => {
    const getLifeCircle = vi.fn(async () => {
      throw { code: "BAIDU_KEY_REQUIRED", message: "请先设置百度地图 Web 服务 AK" };
    });
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    state.pick("circle", { title: "中心", location: { lat: 31.2, lng: 121.4 }, provider: "tencent" });
    await state.loadLifeCircle();
    expect(state.lifeCircleError).toContain("AK");
  });

  it("服务包可多选，但不允许取消到空", () => {
    const state = new PlannerState(fakeClient());
    state.togglePackage("emergency");
    expect(state.selectedPackages).toEqual(["care", "emergency"]);
    state.togglePackage("care");
    expect(state.selectedPackages).toEqual(["emergency"]);
    state.togglePackage("emergency");
    expect(state.selectedPackages).toEqual(["emergency"]);
  });

  it("取消勾选后该包的旧结果不再展示", async () => {
    const state = new PlannerState(fakeClient());
    state.pick("circle", { title: "中心", location: { lat: 31.2, lng: 121.4 }, provider: "tencent" });
    state.togglePackage("family");
    await state.loadLifeCircle();
    expect(state.circleResults).toHaveLength(2);
    state.togglePackage("family");
    expect(state.circleResults.map((result) => result.package)).toEqual(["care"]);
  });

  it("没有任何地点时不请求生活圈", async () => {
    const getLifeCircle = vi.fn(async () => ({ package: "care" as const, crs: "BD09" as const, limitMinutes: 15, pois: [] }));
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    await state.loadLifeCircle();
    expect(getLifeCircle).not.toHaveBeenCalled();
    expect(state.lifeCircleError).toContain("地点");
  });

  it("打了字但没点候选时不猜坐标", async () => {
    const getLifeCircle = vi.fn(async () => ({ package: "care" as const, crs: "BD09" as const, limitMinutes: 15, pois: [] }));
    const state = new PlannerState(fakeClient({ getLifeCircle }));
    state.fromText = "孝陵卫地铁站";
    state.toText = "南京南站";
    await state.run();
    state.onInput("circle", "随手打的字");
    await state.loadLifeCircle();
    expect(getLifeCircle).not.toHaveBeenCalled();
  });

  it("交换起终点：文本与锁定的地点一起换", () => {
    const state = new PlannerState(fakeClient());
    state.fromText = "A";
    state.toText = "B";
    state.pick("to", place("B站"));
    state.swap();
    expect(state.fromText).toBe("B站");
    expect(state.fromPlace?.title).toBe("B站");
    expect(state.toText).toBe("A");
    expect(state.toPlace).toBeNull();
  });

  it("起终点为空时不发请求，给可读的提示", async () => {
    const client = fakeClient();
    const state = new PlannerState(client);
    await state.run();
    expect(client.planRoute).not.toHaveBeenCalled();
    expect(state.error).toContain("起点");
  });

  it("后端错误对象转为可读文案", async () => {
    const client = fakeClient({
      planRoute: vi.fn(async () => {
        throw { code: "NO_ROUTE", message: "没有找到符合条件的路线", status: 404 };
      }),
    });
    const state = new PlannerState(client);
    state.fromText = "a";
    state.toText = "b";
    await state.run();
    expect(state.error).toContain("没有找到");
    expect(state.plan).toBeNull();
  });

  it("降级说明透传给界面", async () => {
    const p = plan();
    p.degraded = { tried: ["amap"], reason: "主服务不可用" };
    const client = fakeClient({ planRoute: vi.fn(async () => p) });
    const state = new PlannerState(client);
    state.fromText = "a";
    state.toText = "b";
    await state.run();
    expect(state.degradedNote).toContain("amap");
  });
});
