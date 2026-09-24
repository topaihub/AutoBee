import { describe, expect, it, vi } from "vitest";
import type { PlannerClient } from "../ipc/client";
import type { LlmProvider } from "../ipc/types";
import { LlmState } from "./llm.svelte";

function provider(id: string, models: string[] = []): LlmProvider {
  return {
    id,
    name: "某中转",
    baseUrl: "https://api.example.com/v1",
    protocol: "openai-chat",
    enabled: true,
    apiKeyMasked: "sk-0********cdef",
    models: models.map((model) => ({ model, toolsOk: true })),
  };
}

function fakeClient(overrides: Partial<PlannerClient> = {}): PlannerClient {
  return {
    listLlmProviders: vi.fn(async () => [provider("p1")]),
    saveLlmProvider: vi.fn(async () => [provider("p1")]),
    deleteLlmProvider: vi.fn(async () => []),
    fetchLlmModels: vi.fn(async () => ["gpt-4o"]),
    saveLlmModels: vi.fn(async () => [provider("p1", ["gpt-4o"])]),
    ...overrides,
  } as unknown as PlannerClient;
}

describe("LlmState", () => {
  it("拉取失败时不把上一次的候选留在界面上冒充结果", async () => {
    // 失败与「返回空列表」是两件事：混在一起的话用户看到一个空多选框，
    // 而真正的原因（Key 不对、base_url 少了 /v1）一个字都看不到。
    const state = new LlmState(
      fakeClient({
        fetchLlmModels: vi.fn(async () => {
          throw { code: "LLM_NO_MODELS", message: "这个入口返回了空的模型列表", status: 502 };
        }),
      }),
    );

    await state.fetchModels("p1");

    expect(state.candidates["p1"]).toBeUndefined();
    expect(state.fetchError["p1"]).toContain("空的模型列表");
    expect(state.fetching["p1"]).toBe(false);
  });

  it("重新拉取会清掉上一次的错误", async () => {
    const state = new LlmState(fakeClient());
    state.fetchError = { p1: "上一次失败了" };

    await state.fetchModels("p1");

    expect(state.fetchError["p1"]).toBe("");
    expect(state.candidates["p1"]).toEqual(["gpt-4o"]);
  });

  it("删掉入口时连它的候选一起丢掉", async () => {
    // 留着的话下一个新建的入口会看到上一家的模型列表。
    const state = new LlmState(fakeClient());
    state.candidates = { p1: ["gpt-4o"], p2: ["claude-3"] };

    await state.remove("p1");

    expect(state.candidates).toEqual({ p2: ["claude-3"] });
  });

  it("后端错误对象转成可读文案而不是 [object Object]", async () => {
    const state = new LlmState(
      fakeClient({
        saveLlmProvider: vi.fn(async () => {
          throw { code: "BAD_INPUT", message: "base_url 不能为空", status: 400 };
        }),
      }),
    );

    await state.save({ name: "x", baseUrl: "", protocol: "openai-chat", enabled: true });

    expect(state.error).toContain("base_url");
  });
});
