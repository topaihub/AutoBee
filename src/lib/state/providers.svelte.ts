/**
 * 服务商档案 + 界面偏好的状态（设置页用）。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { MapProvider, MapProviderInput, Settings, TestResult } from "../ipc/types";
import { errorMessage } from "./planner.svelte";

export class ProvidersState {
  list = $state<MapProvider[]>([]);
  settings = $state<Settings | null>(null);
  loading = $state(false);
  error = $state("");
  saved = $state(false);

  /** 每个 slug 的测试中/结果。用 Record 而不是 Map，$state 的深层代理对它更友好。 */
  testing = $state<Record<string, boolean>>({});
  results = $state<Record<string, TestResult>>({});

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  async load(): Promise<void> {
    this.loading = true;
    this.error = "";
    try {
      this.list = await this.client.listProviders();
      this.settings = await this.client.getSettings();
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  /** 保存一条档案后整体刷新列表（后端返回的就是新列表）。 */
  async save(input: MapProviderInput): Promise<void> {
    this.error = "";
    try {
      this.list = await this.client.saveProvider(input);
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async remove(slug: string): Promise<void> {
    this.error = "";
    try {
      this.list = await this.client.deleteProvider(slug);
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  /** 真发一次请求验证 Key。结果按 slug 记，卡片就地显示。 */
  async test(slug: string): Promise<void> {
    this.testing = { ...this.testing, [slug]: true };
    this.results = { ...this.results, [slug]: { ok: false, message: "" } };
    try {
      const result = await this.client.testProvider(slug);
      this.results = { ...this.results, [slug]: result };
    } catch (e) {
      this.results = { ...this.results, [slug]: { ok: false, message: errorMessage(e) } };
    } finally {
      this.testing = { ...this.testing, [slug]: false };
    }
  }

  async saveSettings(settings: Settings): Promise<void> {
    this.error = "";
    try {
      this.settings = await this.client.saveSettings(settings);
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  #savedTimer: ReturnType<typeof setTimeout> | undefined;
  /** 「已保存」提示，2.5 秒后自己消失（微交互：静默成功，不打断）。 */
  flashSaved(): void {
    this.saved = true;
    clearTimeout(this.#savedTimer);
    this.#savedTimer = setTimeout(() => (this.saved = false), 2500);
  }
}

export const providers = new ProvidersState();
