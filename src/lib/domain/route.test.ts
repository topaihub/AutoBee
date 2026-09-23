import { describe, expect, it } from "vitest";
import type { Route } from "../ipc/types";
import { formatDistance, formatDuration, formatPrice, formatStations } from "./format";
import { hasBus, hasSubway, lineNames, totalWalkDistance } from "./route";

function route(overrides: Partial<Route>): Route {
  return {
    id: "r0",
    distance: 18800,
    duration: 45,
    price: 5,
    bounds: [
      { lat: 30.5, lng: 114.26 },
      { lat: 30.55, lng: 114.43 },
    ],
    segments: [],
    ...overrides,
  };
}

describe("format", () => {
  it("距离：一公里内用米", () => {
    expect(formatDistance(350)).toBe("350 米");
    expect(formatDistance(999.4)).toBe("999 米");
  });

  it("距离：以上用公里，一位小数", () => {
    expect(formatDistance(18800)).toBe("18.8 公里");
    expect(formatDistance(1000)).toBe("1.0 公里");
  });

  it("时长：不足一小时只有分钟", () => {
    expect(formatDuration(45)).toBe("45 分钟");
  });

  it("时长：小时整与小时带分钟", () => {
    expect(formatDuration(60)).toBe("1 小时");
    expect(formatDuration(65)).toBe("1 小时 5 分");
  });

  it("票价：免费与整数元", () => {
    expect(formatPrice(0)).toBe("免费");
    expect(formatPrice(5)).toBe("5 元");
    expect(formatPrice(4.5)).toBe("4.5 元");
  });

  it("站数：0 站不显示", () => {
    expect(formatStations(0)).toBe("");
    expect(formatStations(11)).toBe("11 站");
  });
});

describe("route 摘要", () => {
  const sample = route({
    segments: [
      { kind: "walk", distance: 300, duration: 4, points: [] },
      {
        kind: "transit",
        distance: 15000,
        duration: 30,
        points: [],
        line: {
          vehicle: "SUBWAY",
          title: "2号线",
          stationCount: 12,
          color: "#5b6ee1",
          getOn: { title: "光谷大道", location: { lat: 30.5, lng: 114.4 } },
          getOff: { title: "中南路", location: { lat: 30.54, lng: 114.33 } },
        },
      },
      {
        kind: "transit",
        distance: 3000,
        duration: 10,
        points: [],
        line: {
          vehicle: "BUS",
          title: "703路",
          stationCount: 5,
          color: "#2ba471",
          getOn: { title: "中南路", location: { lat: 30.54, lng: 114.33 } },
          getOff: { title: "钟家村", location: { lat: 30.55, lng: 114.27 } },
        },
      },
      { kind: "walk", distance: 120, duration: 2, points: [] },
    ],
  });

  it("换乘摘要按顺序列出线路名", () => {
    expect(lineNames(sample)).toEqual(["2号线", "703路"]);
  });

  it("步行总距离是各步行段之和", () => {
    expect(totalWalkDistance(sample)).toBe(420);
  });

  it("地铁与公交的判定", () => {
    expect(hasSubway(sample)).toBe(true);
    expect(hasBus(sample)).toBe(true);
    expect(hasSubway(route({ segments: [{ kind: "walk", distance: 1, duration: 1, points: [] }] }))).toBe(false);
  });
});
