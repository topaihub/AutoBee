import { describe, expect, it, vi } from "vitest";
import type { ChatPort, RunEvent, RunRequest } from "@ouraihub/aether-ui/ports";
import type { PlannerClient } from "../ipc/client";
import type { RoutePlan } from "../ipc/types";
import { AssistantState } from "./assistant.svelte";

const plan = { provider: "baidu" } as unknown as RoutePlan;
const planEvent: RunEvent = { kind: "tool_result", callId: "t1", outcome: "ok", summary: "", payload: { kind: "plan", plan } };
const accepted = { runId: "r1", conversationId: "c1", turnId: "u1" };

function setup(emit: (onEvent: (e: RunEvent) => void) => Promise<typeof accepted>) {
  const saveRouteSnapshot = vi.fn(async () => undefined);
  const planner = { applyPlan: vi.fn(), applyCircle: vi.fn() };
  const state = new AssistantState({ saveRouteSnapshot } as unknown as PlannerClient, planner);
  const chat = { run: (_req, onEvent) => emit(onEvent), approve: vi.fn() } as ChatPort;
  state.wrap(chat);
  return { state, chat, planner, saveRouteSnapshot };
}

describe("AssistantState.wrap", () => {
  it("事件先于 run() 返回：拿到会话 id 后补存快照", async () => {
    const { state, chat, planner, saveRouteSnapshot } = setup(async (onEvent) => {
      onEvent(planEvent);
      return accepted;
    });
    await chat.run({ conversationId: null } as RunRequest, () => {});
    expect(planner.applyPlan).toHaveBeenCalledWith(plan, true);
    expect(saveRouteSnapshot).toHaveBeenCalledWith("c1", plan);
    expect(state.hasRoute).toBe(true);
  });

  it("事件在 run() 返回后到达：直接存快照", async () => {
    let later: (e: RunEvent) => void = () => {};
    const { chat, saveRouteSnapshot } = setup(async (onEvent) => {
      later = onEvent;
      return accepted;
    });
    await chat.run({ conversationId: null } as RunRequest, () => {});
    expect(saveRouteSnapshot).not.toHaveBeenCalled();
    later(planEvent);
    expect(saveRouteSnapshot).toHaveBeenCalledOnce();
    expect(saveRouteSnapshot).toHaveBeenCalledWith("c1", plan);
  });
});
