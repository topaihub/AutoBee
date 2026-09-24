/**
 * 大模型服务入口的状态（设置页用）。
 *
 * 与 `providers.svelte.ts` 同一个形状：后端每个写操作都返回新列表，
 * 前端不自己拼接 —— 拼接会在「保存失败但界面已经改了」时骗到用户。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { LlmModelInput, LlmProvider, LlmProviderInput } from "../ipc/types";
import { errorMessage } from "./planner.svelte";

export class LlmState {
  list = $state<LlmProvider[]>([]);
  loading = $state(false);
  error = $state("");
  saved = $state(false);

  /** 每个入口的「获取模型列表」在飞状态与结果。按 id 记，卡片就地显示。 */
  fetching = $state<Record<string, boolean>>({});
  /** 拉回来的候选模型名。不落库，所以只活在这里。 */
  candidates = $state<Record<string, string[]>>({});
  /** 拉取失败的原因。与 candidates 分开：空列表和失败是两件事。 */
  fetchError = $state<Record<string, string>>({});

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  async load(): Promise<void> {
    this.loading = true;
    this.error = "";
    try {
      this.list = await this.client.listLlmProviders();
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  async save(input: LlmProviderInput): Promise<void> {
    this.error = "";
    try {
      this.list = await this.client.saveLlmProvider(input);
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async remove(id: string): Promise<void> {
    this.error = "";
    try {
      this.list = await this.client.deleteLlmProvider(id);
      // 候选列表跟着删掉：留着的话下次新建的入口会看到上一家的模型。
      const rest = { ...this.candidates };
      delete rest[id];
      this.candidates = rest;
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  /** 真发一次 `GET {baseUrl}/models`。结果只进内存供当次挑选。 */
  async fetchModels(id: string): Promise<void> {
    this.fetching = { ...this.fetching, [id]: true };
    this.fetchError = { ...this.fetchError, [id]: "" };
    try {
      this.candidates = { ...this.candidates, [id]: await this.client.fetchLlmModels(id) };
    } catch (e) {
      this.fetchError = { ...this.fetchError, [id]: errorMessage(e) };
    } finally {
      this.fetching = { ...this.fetching, [id]: false };
    }
  }

  /** 覆盖某个入口挑中的模型。取消勾选的会真的消失。 */
  async saveModels(providerId: string, models: LlmModelInput[]): Promise<void> {
    this.error = "";
    try {
      this.list = await this.client.saveLlmModels(providerId, models);
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

export const llm = new LlmState();
