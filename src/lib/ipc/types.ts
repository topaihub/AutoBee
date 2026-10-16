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

export type TestResult = {
  ok: boolean;
  message: string;
};
