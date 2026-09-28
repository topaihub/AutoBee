/**
 * 客户端接口：状态层只认识这个接口，不认识 Tauri。
 *
 * Vitest 用假实现注入（见 state/*.test.ts），所以状态层的测试不必起 Tauri。
 * command 名必须与 `commands.rs` 里的函数名一致，对不上时运行时报
 * 「unknown command」—— 这种错误类型检查抓不到，改 Rust 侧时两边都要搜一遍。
 */
import { invokeCmd, listenEvent } from "./tauri-client";
import type {
  ChatMessage,
  ChatStartHandle,
  ChatStreamEvent,
  Conversation,
  Crs,
  LifeCircle,
  LifeCirclePackage,
  LlmModelInput,
  LlmProvider,
  LlmProviderInput,
  LogEntry,
  MapProvider,
  MapProviderInput,
  McpServer,
  McpServerInput,
  McpToolInfo,
  ModelRef,
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
  listLlmProviders(): Promise<LlmProvider[]>;
  saveLlmProvider(input: LlmProviderInput): Promise<LlmProvider[]>;
  deleteLlmProvider(id: string): Promise<LlmProvider[]>;
  /** 真发一次 `GET {baseUrl}/models`，结果不落库。 */
  fetchLlmModels(id: string): Promise<string[]>;
  saveLlmModels(providerId: string, models: LlmModelInput[]): Promise<LlmProvider[]>;

  listConversations(): Promise<Conversation[]>;
  readConversation(id: string): Promise<ChatMessage[]>;
  /** 历史路线的完整折线快照；旧会话没有时返回 null。 */
  readConversationRoute(id: string): Promise<RoutePlan | null>;
  deleteConversation(id: string): Promise<void>;
  /**
   * 起一轮对话。**立即返回**，正文与工具进展走 `llm:chat:<streamId>` 事件。
   *
   * 返回值里带 `convId` 与已落库的用户消息：会话可能是这次新建的，而上游失败时
   * 不会有任何成功事件 —— 那时前端仍需知道往哪个会话追加，否则下一句又新建一个。
   */
  chatStart(convId: string | null, text: string, model: ModelRef): Promise<ChatStartHandle>;
  /** 取消一条流。流已结束或 id 不存在时报 `STREAM_NOT_FOUND`。 */
  chatCancel(streamId: string): Promise<void>;
  /** 订阅一条流的事件。返回退订函数，调用方**必须**在流结束后调它。 */
  onChatEvent(streamId: string, handler: (event: ChatStreamEvent) => void): Promise<() => void>;

  listMcpServers(): Promise<McpServer[]>;
  saveMcpServer(input: McpServerInput): Promise<McpServer[]>;
  deleteMcpServer(id: string): Promise<McpServer[]>;
  /** 连上去跑一次 tools/list，不落库。`id` 省略则遍历全部启用的 server。 */
  listMcpTools(id?: string): Promise<McpToolInfo[]>;
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
  listLlmProviders: () => invokeCmd("list_llm_providers"),
  saveLlmProvider: (input) => invokeCmd("save_llm_provider", { input }),
  deleteLlmProvider: (id) => invokeCmd("delete_llm_provider", { id }),
  fetchLlmModels: (id) => invokeCmd("fetch_llm_models", { id }),
  saveLlmModels: (providerId, models) => invokeCmd("save_llm_models", { providerId, models }),
  listConversations: () => invokeCmd("list_conversations"),
  readConversation: (id) => invokeCmd("read_conversation", { id }),
  readConversationRoute: (id) => invokeCmd("read_conversation_route", { id }),
  deleteConversation: (id) => invokeCmd("delete_conversation", { id }),
  chatStart: (convId, text, model) => invokeCmd("llm_chat_start", { convId, text, model }),
  chatCancel: (streamId) => invokeCmd("llm_chat_cancel", { streamId }),
  onChatEvent: (streamId, handler) => listenEvent(`llm:chat:${streamId}`, handler),
  listMcpServers: () => invokeCmd("list_mcp_servers"),
  saveMcpServer: (input) => invokeCmd("save_mcp_server", { input }),
  deleteMcpServer: (id) => invokeCmd("delete_mcp_server", { id }),
  listMcpTools: (id) => invokeCmd("list_mcp_tools", { id }),
};
