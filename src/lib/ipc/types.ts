/**
 * IPC 的 DTO —— 与 `src-tauri/core/src/models.rs`、`credentials/mod.rs` 保持一致。
 *
 * 跨语言的 IPC 本来就没有编译期检查，两边靠这份手抄对齐。**改动 Rust 结构时
 * 必须同步这里**，否则现象是运行时字段全是 undefined 而类型检查全绿。
 * （ai-installer 同样手抄，注释见它的 ipc/types.ts。）
 */

export type LatLng = { lat: number; lng: number };

export type Crs = "WGS84" | "GCJ02" | "BD09" | "CGCS2000";

export type ProviderId = "tencent" | "amap" | "baidu" | "tianditu" | "here" | "google";

export type TravelMode = "subway" | "bus" | "walking";
export type SortBy = "time" | "distance";
export type Vehicle = "SUBWAY" | "BUS";
export type SegmentKind = "walk" | "transit";

export type LifeCirclePackage = "care" | "family" | "emergency";

export type ReachablePoi = {
  category: string;
  name: string;
  address: string;
  location: LatLng;
  walkDurationSec: number;
  walkDistance: number;
};

export type LifeCircle = {
  package: LifeCirclePackage;
  crs: Crs;
  limitMinutes: number;
  pois: ReachablePoi[];
};

export type Place = {
  title: string;
  address?: string;
  location: LatLng;
  adcode?: string;
  provider: ProviderId;
  id?: string;
};

export type Stop = {
  title: string;
  location: LatLng;
  exit?: string;
};

export type TransitLine = {
  vehicle: Vehicle;
  title: string;
  stationCount: number;
  /** 服务端定好的颜色（地铁按序轮流、公交固定绿色），前端不再计算 */
  color: string;
  getOn: Stop;
  getOff: Stop;
  destination?: string;
  firstTime?: string;
  lastTime?: string;
};

export type Segment = {
  kind: SegmentKind;
  /** 米 */
  distance: number;
  /** 分钟 */
  duration: number;
  points: LatLng[];
  instruction?: string[];
  line?: TransitLine;
};

export type Bounds = [LatLng, LatLng];

export type Route = {
  id: string;
  /** 米 */
  distance: number;
  /** 分钟 */
  duration: number;
  /** 元 */
  price: number;
  bounds: Bounds;
  segments: Segment[];
};

export type Endpoint = {
  title: string;
  location: LatLng;
};

export type Degraded = {
  tried: ProviderId[];
  reason?: string;
};

export type RoutePlan = {
  origin: Endpoint;
  destination: Endpoint;
  mode: TravelMode;
  routes: Route[];
  provider: ProviderId;
  /** 输出坐标系。桌面版底图是 OSM/Carto 瓦片，默认 WGS84 */
  crs: Crs;
  degraded?: Degraded;
};

/** Rust 侧 `AppError` 的序列化形状 */
export type AppError = {
  code: string;
  message: string;
  status: number;
};

export type MapProvider = {
  slug: ProviderId;
  name: string;
  isPreset: boolean;
  enabled: boolean;
  priority?: number;
  defaultPriority: number;
  coverage: string;
  coordSys: string;
  capabilities: string;
  /** 掩码后的服务端 Key，空串表示还没填 */
  apiKeyMasked: string;
  /** 掩码后的浏览器端 Key，空串表示还没填。只有百度用得上 */
  browserKeyMasked: string;
  implemented: boolean;
  /** 官方申请 API Key 的控制台链接 */
  keyApplyUrl: string;
};

export type MapProviderInput = {
  slug: ProviderId;
  name: string;
  enabled: boolean;
  priority?: number | null;
  /** 新密钥明文。null = 没动过（后端保留库里那份） */
  apiKey?: string | null;
  /** 新的浏览器端密钥明文。null = 没动过 */
  browserKey?: string | null;
};

export type Settings = {
  autoProvider: boolean;
  outCrs: Crs;
  allowCrossBorder: boolean;
  tileSource: string;
  defaultProviders: ProviderId[];
  showLogs: boolean;
  /** 「含较长步行」提醒阈值，米。见 §4.2:阈值不许写死在 UI。 */
  longWalkMeters: number;
  /** 「换乘较多」提醒阈值，次。 */
  manyTransfers: number;
};

export type LogLevel = "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE";

/** 后端日志的一行。与 `src-tauri/src/logs.rs` 的 `LogEntry` 对应。 */
export type LogEntry = {
  seq: number;
  time: string;
  /** 不合格式的行（panic 回溯等）后端按 INFO 给出，整行在 message 里。 */
  level: LogLevel | string;
  target: string;
  message: string;
};

export type TestResult = {
  ok: boolean;
  message: string;
};

/**
 * 大模型服务入口的协议。
 *
 * 「中转商」不是单独的类型：它就是 baseUrl 指向代理、协议选 `openai-chat`
 * 的普通入口。与 `credentials/mod.rs` 的 `LlmProtocol` 对应（serde kebab-case）。
 */
export type LlmProtocol = "anthropic-messages" | "openai-responses" | "openai-chat";

/** 用户从某个入口挑中的一个模型。 */
export type LlmModel = {
  model: string;
  /** 显示名。缺省则显示 model */
  alias?: string | null;
  /** 该模型是否支持 tool calling */
  toolsOk: boolean;
};

export type LlmProvider = {
  id: string;
  name: string;
  baseUrl: string;
  protocol: LlmProtocol;
  enabled: boolean;
  /** 掩码后的 Key，空串表示还没填 */
  apiKeyMasked: string;
  models: LlmModel[];
};

export type LlmProviderInput = {
  /** null = 新建 */
  id?: string | null;
  name: string;
  baseUrl: string;
  protocol: LlmProtocol;
  enabled: boolean;
  /** 新密钥明文。null 或空串 = 没动过（后端保留库里那份） */
  apiKey?: string | null;
};

export type LlmModelInput = {
  model: string;
  alias?: string | null;
  toolsOk?: boolean;
};


/** 用户导入的离线 Skill。其指引只能扩展对话策略，不能覆盖导航系统规则。 */
export type Skill = {
  id: string;
  packageId: string;
  name: string;
  version: string;
  description: string;
  /** 已校验 UTF-8 的 SKILL.md，供设置页查看；不作为 HTML 渲染。 */
  instructions: string;
  enabled: boolean;
  mcpServerIds: string[];
};
export type ChatRole = "system" | "user" | "assistant" | "tool";

/** 一次对话。`updatedAt` 降序排列，两个时间戳都是 unix 秒。 */
export type Conversation = {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
};

/**
 * 一条消息。`toolCalls` 是原始 JSON 串而不是解析后的结构：界面只折叠显示它，
 * 解析它等于在前端再实现一遍 OpenAI 的 tool call 形状，而那个形状由后端负责。
 */
export type ChatMessage = {
  id: string;
  convId: string;
  role: ChatRole;
  content: string;
  toolCalls?: string | null;
  toolName?: string | null;
  model?: string | null;
  createdAt: number;
};

/** 指定用哪个模型。两段都要：同一个模型标识可能挂在多个服务商下。 */
export type ModelRef = {
  providerId: string;
  model: string;
};

/**
 * 流式事件的载荷，事件名 `llm:chat:<streamId>`。
 *
 * `summary` 是给人看的一句话（如「找到 3 条路线」），**不是原始 JSON**，
 * 前端不该去解析它。
 *
 * `toolEnd` 的 `plan` / `circle` 是 §6.4.2 的回灌载荷，**未经裁剪**:发回模型的
 * JSON 不含折线点（省上下文），这两个字段带的是原始结构，地图要折线才画得出线。
 * 只在对应工具**成功**时出现 —— 失败、截断、以及 `search_place` 都没有。
 */
export type ChatStreamEvent =
  | { kind: "delta"; text: string }
  | { kind: "toolStart"; tool: string; args: string }
  | {
      kind: "toolEnd";
      tool: string;
      ok: boolean;
      summary: string;
      plan?: RoutePlan;
      circle?: LifeCircle;
    }
  | { kind: "done"; messages: ChatMessage[] }
  | { kind: "error"; code: string; message: string };

/**
 * `chatStart` 的立即返回值。
 *
 * `convId` 必须带回来：首轮是后端新建的，前端不知道该往哪个会话追加。
 * `userMessage` 是已落库的那条用户消息，用来替换乐观气泡（拿到真实 id 与时间）。
 *
 * 这两个字段**不随成功事件下发**：上游失败时压根没有成功事件，而用户那句话
 * 已经落库了。少了它们，下一句会再新建一个会话，库里于是攒下一串孤立会话。
 */
export type ChatStartHandle = {
  streamId: string;
  convId: string;
  userMessage: ChatMessage;
};

/** 一个 MCP server。`toolCount` 是上次成功 `tools/list` 拿到的条数，null = 还没连上过。 */
export type McpServer = {
  id: string;
  name: string;
  url: string;
  enabled: boolean;
  toolCount?: number | null;
};

export type McpServerInput = {
  /** null = 新建 */
  id?: string | null;
  name: string;
  url: string;
  enabled: boolean;
};

/** `list_mcp_tools` 的一项。`server` 是 server 的 name，用于界面分组。 */
export type McpToolInfo = {
  /** 带前缀的完整工具名，形如 `mcp_baidu_<tool>` */
  name: string;
  server: string;
  description: string;
};
