import { describe, expect, it } from "vitest";
import type { Route } from "../ipc/types";
import { burdenReason, routeBurden, sortedRoutes } from "./accessibility";

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
});
