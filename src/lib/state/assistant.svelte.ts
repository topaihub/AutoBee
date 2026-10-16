/**
 * 「问一问」与地图的接缝：Aether ChatPanel 自己管对话，这里只把宿主工具的
 * `tool_result.payload` 回灌到 planner，并按会话存/取路线快照。
 */
import type { ChatPort } from "@ouraihub/aether-ui/ports";
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { LifeCircle, RoutePlan } from "../ipc/types";
import { planner as defaultPlanner, type PlannerState } from "./planner.svelte";

/** 后端 plan_route / life_circle 成功时的载荷；其余工具不带。 */
type Payload = { kind: "plan"; plan: RoutePlan } | { kind: "circle"; circle: LifeCircle };

export class AssistantState {
  /** ChatPanel 当前显示的会话；null = 新对话。 */
  conversationId = $state<string | null>(null);
  /** 当前会话是否产生过路线；避免把安心换乘的旧路线误显示在聊天页。 */
  hasRoute = $state(false);

  constructor(
    private readonly client: PlannerClient = tauriPlannerClient,
    private readonly planner: Pick<PlannerState, "applyPlan" | "applyCircle"> = defaultPlanner,
  ) {}

  /** 包一层 `chat.run`：工具事件可能先于 run() resolve 到达，两种顺序都要存上快照。 */
  wrap(chat: ChatPort): void {
    const run = chat.run.bind(chat);
    chat.run = async (request, onEvent, options) => {
      let convId = request.conversationId ?? null;
      let pending: RoutePlan | null = null;
      const accepted = await run(
        request,
        (event) => {
          if (event.kind === "tool_result" && event.outcome === "ok" && event.payload) {
            const payload = event.payload as Payload;
            if (payload.kind === "plan") {
              this.planner.applyPlan(payload.plan, true);
              this.hasRoute = true;
              if (convId) void this.save(convId, payload.plan);
              else pending = payload.plan;
            } else if (payload.kind === "circle") {
              this.planner.applyCircle(payload.circle);
            }
          }
          onEvent(event);
        },
        options,
      );
      convId = accepted.conversationId;
      // ChatPanel 随后会报告这个 id；先记下，open() 就不会把刚画上的路线收起。
      this.conversationId = convId;
      if (convId && pending) void this.save(convId, pending);
      return accepted;
    };
  }

  /** ChatPanel 切换会话：有快照就重画，没有就收起地图。 */
  async open(id: string | null): Promise<void> {
    if (id === this.conversationId) return;
    this.conversationId = id;
    this.hasRoute = false;
    if (!id) return;
    const plan = await this.client.readRouteSnapshot(id).catch(() => null);
    if (plan && this.conversationId === id) {
      this.planner.applyPlan(plan, true);
      this.hasRoute = true;
    }
  }

  private async save(convId: string, plan: RoutePlan): Promise<void> {
    // ponytail: 快照存失败只影响重开会话时的地图，不打断对话。
    await this.client.saveRouteSnapshot(convId, plan).catch(() => undefined);
  }
}

/** 单例。layout 包 chat port，页面读 hasRoute。 */
export const assistant = new AssistantState();
