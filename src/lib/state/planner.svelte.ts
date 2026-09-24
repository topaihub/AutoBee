/**
 * 规划页的状态。组件只收 props 和回调，IPC 由这里发起（eslint 会拦组件）。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type {
  Crs,
  LatLng,
  LifeCircle,
  LifeCirclePackage,
  Place,
  ReachablePoi,
  Route,
  RoutePlan,
  SortBy,
  TravelMode,
} from "../ipc/types";
import { type TravelProfile, sortedRoutes } from "../domain/accessibility";

/** 输入框在「哪个字段」上联想。`circle` 是生活圈自己的地点输入。 */
export type Field = "from" | "to" | "circle";

/** 一个服务包的查询结果。失败也留在列表里 —— 用户要知道是哪一包没出来。 */
export type CircleResult =
  | { package: LifeCirclePackage; ok: true; circle: LifeCircle }
  | { package: LifeCirclePackage; ok: false; message: string };

/**
 * 防抖定时器句柄。浏览器给 number、Node 给 Timeout，两个环境都要编过
 * （vitest 跑在 node 下），所以按平台声明取而不是写死 number。
 */
type TimerHandle = ReturnType<typeof setTimeout>;

export class PlannerState {
  /** 输入框文本。用户手打的与候选点选的都要同步回这里。 */
  fromText = $state("");
  toText = $state("");

  /** 点选锁定的地点。有它就直接按坐标规划，不再走文本解析。 */
  fromPlace = $state<Place | null>(null);
  toPlace = $state<Place | null>(null);

  mode = $state<TravelMode>("subway");
  sort = $state<SortBy>("time");

  /** AccessMap-inspired burden profile for ranking the current route candidates. */
  profile = $state<TravelProfile>("standard");

  plan = $state<RoutePlan | null>(null);
  selectedRouteId = $state<string | null>(null);

  loading = $state(false);
  error = $state("");
  /** 后端降级说明（主服务商失败时换了一家），有值就展示。 */
  degradedNote = $state("");

  /**
   * 生活圈自己的地点。默认继承安心换乘的起点，用户也可以单独输入另一个地点。
   *
   * 独立字段而不是复用 `fromText`:两个功能的地点经常不是同一个
   * ——「从家出发去公司」与「看看医院附近有什么」。
   */
  circleText = $state("");
  circlePlace = $state<Place | null>(null);
  circleSuggestions = $state<Place[]>([]);
  /** 用户是否动过生活圈的输入框。没动过就跟着安心换乘的起点走。 */
  circleTouched = $state(false);

  /** 勾选的服务包，可多选。顺序即展示顺序。 */
  selectedPackages = $state<LifeCirclePackage[]>(["care"]);
  /** 每个已勾选包的结果，成功与失败都在里面。 */
  circleResults = $state<CircleResult[]>([]);
  lifeCircleLoading = $state(false);
  lifeCircleError = $state("");

  fromSuggestions = $state<Place[]>([]);
  toSuggestions = $state<Place[]>([]);
  /** 正在联想的字段；下拉的显隐看它。 */
  suggesting = $state<Field | null>(null);

  #timers: Record<Field, TimerHandle | undefined> = {
    from: undefined,
    to: undefined,
    circle: undefined,
  };

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  /**
   * 取百度 JS API GL 的 AK。
   *
   * 放在 store 而不是组件里,是因为组件不许直接发 IPC(eslint 的
   * `no-restricted-imports` 会拦)。这层转发看着是纯代理,但它是组件触达
   * `PlannerClient` 的唯一通道,也是单测注入假 client 的接缝 —— 删掉它
   * 地图组件就只能自己 import `tauriPlannerClient`,那条约定就破了。
   *
   * 没配 AK 时返回 `null` 而不是抛 —— 那不是故障,是「还没填」,
   * 由地图组件显示「去设置里填」而不是进错误态。
   */
  async mapSdkKey(): Promise<string | null> {
    return this.client.getMapSdkKey();
  }

  /** 输入变化 → 防抖联想。空文本清空候选。 */
  onInput(field: Field, text: string): void {
    this.#setText(field, text);
    this.#setPlace(field, null);
    if (field === "circle") this.circleTouched = true;
    clearTimeout(this.#timers[field]);
    if (text.trim().length === 0) {
      this.#clearSuggestions(field);
      return;
    }
    this.#timers[field] = setTimeout(() => void this.#suggest(field, text), 250);
  }

  async #suggest(field: Field, text: string): Promise<void> {
    this.suggesting = field;
    try {
      const hits = await this.client.suggest(text.trim());
      this.#setSuggestions(field, hits);
    } catch {
      this.#clearSuggestions(field);
    } finally {
      this.suggesting = null;
    }
  }

  /** 从下拉候选点选：锁定坐标，规划时直接用，跳过文本解析。 */
  pick(field: Field, place: Place): void {
    this.#setPlace(field, place);
    this.#setText(field, place.title);
    this.#clearSuggestions(field);
    if (field === "circle") this.circleTouched = true;
  }

  /** 交换起终点（文本与锁定的地点一起换）。 */
  swap(): void {
    const t = this.fromText;
    const p = this.fromPlace;
    this.fromText = this.toText;
    this.fromPlace = this.toPlace;
    this.toText = t;
    this.toPlace = p;
    this.#clearSuggestions("from");
    this.#clearSuggestions("to");
  }

  /** 切换方式后，已有结果立即按新方式重算。 */
  setMode(mode: TravelMode): void {
    if (this.mode === mode) return;
    this.mode = mode;
    if (this.plan && !this.loading) void this.run();
  }

  /** Changes how existing route candidates are ranked without repeating the API call. */
  setProfile(profile: TravelProfile): void {
    if (this.profile === profile) return;
    this.profile = profile;
    this.selectedRouteId = this.routes[0]?.id ?? null;
  }

  get routes(): Route[] {
    return this.plan ? sortedRoutes(this.plan.routes, this.profile) : [];
  }

  /** 规划。起终点为空或太短时给出可读的错误，不发请求。 */
  async run(): Promise<void> {
    const from = this.#endpointText("from");
    const to = this.#endpointText("to");
    if (!from || !to) {
      this.error = "请先填好起点和终点。";
      return;
    }

    this.loading = true;
    this.error = "";
    this.degradedNote = "";
    try {
      this.applyPlan(await this.client.planRoute(from, to, this.mode, this.sort));
    } catch (e) {
      this.plan = null;
      this.selectedRouteId = null;
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  /**
   * 把一份算好的方案写进状态。`run()` 与大模型工具调用共用这条路径 ——
   * 直接赋值 `planner.plan` 会绕过选中项与降级文案，地图能画但卡片选不中。
   */
  applyPlan(plan: RoutePlan): void {
    this.plan = plan;
    this.selectedRouteId = this.routes[0]?.id ?? null;
    this.error = "";
    this.degradedNote = plan.degraded
      ? `${plan.degraded.tried.join("、")} 暂不可用，本次结果来自备用服务商。`
      : "";
  }

  /** 选中某个方案；卡片点击与地图上的联动都走这里。 */
  selectRoute(id: string): void {
    this.selectedRouteId = id;
  }

  /**
   * 生活圈中心：用户单独输入过就用那个，否则继承安心换乘的起点。
   *
   * 坐标系必须跟着坐标一起带 —— `suggest` 返回的候选是 provider 原生的
   * GCJ-02，而 `plan.origin` 已经转成用户配置的 `out_crs`。两者混用会
   * 让中心点偏移几百米，POI 也就跟着错。
   */
  get circleOrigin(): { location: LatLng; crs: Crs; label: string } | null {
    if (this.circlePlace) {
      return { location: this.circlePlace.location, crs: "GCJ02", label: this.circlePlace.title };
    }
    // 用户打了字但没点候选：拿不到坐标，交给调用方提示点选。
    if (this.circleTouched && this.circleText.trim().length > 0) return null;
    if (this.plan) {
      return { location: this.plan.origin.location, crs: this.plan.crs, label: this.plan.origin.title };
    }
    if (this.fromPlace) {
      return { location: this.fromPlace.location, crs: "GCJ02", label: this.fromPlace.title };
    }
    return null;
  }

  /** 勾选/取消一个服务包。至少留一个 —— 全空的界面没有可操作的下一步。 */
  togglePackage(packageType: LifeCirclePackage): void {
    const has = this.selectedPackages.includes(packageType);
    if (has && this.selectedPackages.length === 1) return;
    this.selectedPackages = has
      ? this.selectedPackages.filter((item) => item !== packageType)
      : [...this.selectedPackages, packageType];
    // 勾选集变了，旧结果里多出来或少掉的那几包不该继续展示。
    this.circleResults = this.circleResults.filter((result) => this.selectedPackages.includes(result.package));
  }

  /**
   * 查询勾选的全部服务包。并发发起 —— 每包一次百度请求，串行会让
   * 三包等到三倍时间。单包失败只记在那一包上，其余照常出结果。
   */
  async loadLifeCircle(): Promise<void> {
    const origin = this.circleOrigin;
    if (!origin) {
      this.lifeCircleError = "请从候选里点选一个地点，或先规划一次路线。";
      return;
    }
    if (this.selectedPackages.length === 0) {
      this.lifeCircleError = "请至少勾选一个服务包。";
      return;
    }
    this.lifeCircleLoading = true;
    this.lifeCircleError = "";
    const wanted = [...this.selectedPackages];
    try {
      this.circleResults = await Promise.all(
        wanted.map(async (packageType): Promise<CircleResult> => {
          try {
            const circle = await this.client.getLifeCircle(origin.location.lat, origin.location.lng, origin.crs, packageType);
            return { package: packageType, ok: true, circle };
          } catch (error) {
            return { package: packageType, ok: false, message: errorMessage(error) };
          }
        }),
      );
      // 每一包都失败,且原因相同,那不是「这包没数据」而是 AK/配额问题。
      const failures = this.circleResults.filter((result) => !result.ok);
      if (failures.length === this.circleResults.length && failures[0] && !failures[0].ok) {
        this.lifeCircleError = failures[0].message;
      }
    } finally {
      this.lifeCircleLoading = false;
    }
  }

  /** 全部成功结果里的 POI，按步行时长升序。地图打点与「最近的服务」都用它。 */
  get reachablePois(): ReachablePoi[] {
    return this.circleResults
      .flatMap((result) => (result.ok ? result.circle.pois : []))
      .sort((a, b) => a.walkDurationSec - b.walkDurationSec);
  }

  #endpointText(field: Field): string {
    const place = this.#placeOf(field);
    return place ? coordText(place.location) : this.#textOf(field).trim();
  }

  #textOf(field: Field): string {
    if (field === "from") return this.fromText;
    if (field === "to") return this.toText;
    return this.circleText;
  }

  #setText(field: Field, text: string): void {
    if (field === "from") this.fromText = text;
    else if (field === "to") this.toText = text;
    else this.circleText = text;
  }

  #placeOf(field: Field): Place | null {
    if (field === "from") return this.fromPlace;
    if (field === "to") return this.toPlace;
    return this.circlePlace;
  }

  #setPlace(field: Field, place: Place | null): void {
    if (field === "from") this.fromPlace = place;
    else if (field === "to") this.toPlace = place;
    else this.circlePlace = place;
  }

  #setSuggestions(field: Field, hits: Place[]): void {
    if (field === "from") this.fromSuggestions = hits;
    else if (field === "to") this.toSuggestions = hits;
    else this.circleSuggestions = hits;
  }

  #clearSuggestions(field: Field): void {
    this.#setSuggestions(field, []);
  }
}

/** 坐标文本，后端直接按坐标处理，跳过解析。 */
function coordText(l: { lat: number; lng: number }): string {
  return `${l.lat},${l.lng}`;
}

/** 后端的错误是对象 `{ code, message }`；浏览器自身的错误是 Error。 */
export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}

/** 单例。组件从 $lib/state 引这个。 */
export const planner = new PlannerState();
