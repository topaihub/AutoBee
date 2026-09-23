/**
 * 客户端接口：状态层只认识这个接口，不认识 Tauri。
 *
 * Vitest 用假实现注入（见 state/*.test.ts），所以状态层的测试不必起 Tauri。
 * command 名必须与 `commands.rs` 里的函数名一致，对不上时运行时报
 * 「unknown command」—— 这种错误类型检查抓不到，改 Rust 侧时两边都要搜一遍。
 */
import { invokeCmd } from "./tauri-client";
import type {
  Crs,
  LifeCircle,
  LifeCirclePackage,
  LogEntry,
  MapProvider,
  MapProviderInput,
  Place,
  RoutePlan,
  Settings,
  SortBy,
  TestResult,
  TravelMode,
} from "./types";

export interface PlannerClient {
  suggest(keyword: string): Promise<Place[]>;
  planRoute(from: string, to: string, mode: TravelMode, sort: SortBy): Promise<RoutePlan>;
  /** 返回 base64 编码的地图图片 */
  getTile(z: number, x: number, y: number): Promise<string>;
  getMapSdkKey(): Promise<string | null>;
  getLifeCircle(lat: number, lng: number, crs: Crs, packageType: LifeCirclePackage): Promise<LifeCircle>;
  listProviders(): Promise<MapProvider[]>;
  saveProvider(input: MapProviderInput): Promise<MapProvider[]>;
  deleteProvider(slug: string): Promise<MapProvider[]>;
  testProvider(slug: string): Promise<TestResult>;
  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  /** 回读后端日志尾部，旧的在前。 */
  readLogs(limit: number): Promise<LogEntry[]>;
}

/** 真实实现：转发给 Tauri。 */
export const tauriPlannerClient: PlannerClient = {
  suggest: (keyword) => invokeCmd("suggest", { keyword }),
  planRoute: (from, to, mode, sort) => invokeCmd("plan_route", { from, to, mode, sort }),
  getTile: (z, x, y) => invokeCmd("get_tile", { z, x, y }),
  getMapSdkKey: () => invokeCmd("get_map_sdk_key"),
  getLifeCircle: (lat, lng, crs, packageType) => invokeCmd("get_life_circle", { lat, lng, crs, package: packageType }),
  listProviders: () => invokeCmd("list_map_providers"),
  saveProvider: (input) => invokeCmd("save_map_provider", { input }),
  deleteProvider: (slug) => invokeCmd("delete_map_provider", { slug }),
  testProvider: (slug) => invokeCmd("test_map_provider", { slug }),
  getSettings: () => invokeCmd("get_settings"),
  saveSettings: (settings) => invokeCmd("save_settings", { settings }),
  readLogs: (limit) => invokeCmd("read_logs", { limit }),
};
