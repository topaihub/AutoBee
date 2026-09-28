import { describe, expect, it, vi } from "vitest";
import type { PlannerClient } from "../ipc/client";
import type {
  ChatMessage,
  ChatStreamEvent,
  Conversation,
  ReachablePoi,
  RoutePlan,
  Settings,
} from "../ipc/types";
import { ChatState } from "./chat.svelte";
// 单例：`chat.svelte.ts` 的 toolEnd 分支就是往这个实例上回灌的。
import { planner } from "./planner.svelte";

/** 带真实折线的方案。点数是被断言的量 —— 少了就说明裁剪版被当成载荷发了。 */
function chatPlan(): RoutePlan {
  return {
    origin: { title: "光谷广场", location: { lat: 30.5, lng: 114.4 } },
    destination: { title: "汉口火车站", location: { lat: 30.62, lng: 114.26 } },
    mode: "subway",
    routes: [
      {
        id: "r0",
        distance: 18800,
        duration: 45,
        price: 5,
        bounds: [
          { lat: 30.5, lng: 114.26 },
          { lat: 30.62, lng: 114.4 },
        ],
        segments: [
          {
            kind: "transit",
            distance: 18800,
            duration: 45,
            points: [
              { lat: 30.5, lng: 114.4 },
              { lat: 30.56, lng: 114.33 },
              { lat: 30.62, lng: 114.26 },
            ],
            instruction: [],
          },
        ],
      },
    ],
    provider: "baidu",
    crs: "BD09",
  };
}

function chatPoi(name: string): ReachablePoi {
  return {
    category: "医院",
    name,
    address: "解放大道 1095 号",
    location: { lat: 30.58, lng: 114.28 },
    walkDurationSec: 540,
    walkDistance: 620,
  };
}

function conversation(id: string, title = "标题", updatedAt = 100): Conversation {
  return { id, title, createdAt: updatedAt, updatedAt };
}

function message(id: string, convId: string, role: ChatMessage["role"], content: string): ChatMessage {
  return { id, convId, role, content, createdAt: 200 };
}

/**
 * 事件驱动的假后端。
 *
 * `emit` 把一条事件同步推给状态层，测试因此能精确控制「流到哪一步了」——
 * 用定时器等事件到达会让这组测试变成偶发失败的那一类。
 */
function streamHarness(
  convId = "c1",
  userText = "你好",
  overrides: Partial<PlannerClient> = {},
) {
  let handler: ((event: ChatStreamEvent) => void) | null = null;
  const unlisten = vi.fn();
  const client = fakeClient({
    ...overrides,
    chatStart: vi.fn(async () => ({
      streamId: "s1",
      convId,
      userMessage: message(`${convId}-u`, convId, "user", userText),
    })),
    onChatEvent: vi.fn(async (_id: string, h: (event: ChatStreamEvent) => void) => {
      handler = h;
      return unlisten;
    }),
  });
  return {
    client,
    unlisten,
    emit(event: ChatStreamEvent): void {
      if (!handler) throw new Error("还没订阅，`send` 可能失败了");
      handler(event);
    },
    get subscribed(): boolean {
      return handler !== null;
    },
  };
}

function fakeClient(overrides: Partial<PlannerClient> = {}): PlannerClient {
  return {
    suggest: vi.fn(async () => []),
    planRoute: vi.fn(async () => {
      throw new Error("本组测试不算路");
    }),
    getTile: vi.fn(async () => ""),
    getMapSdkKey: vi.fn(async () => null),
    getLifeCircle: vi.fn(async () => ({ package: "care" as const, crs: "BD09" as const, limitMinutes: 15, pois: [] })),
    listProviders: vi.fn(async () => []),
    saveProvider: vi.fn(async () => []),
    deleteProvider: vi.fn(async () => []),
    testProvider: vi.fn(async () => ({ ok: true, message: "" })),
    getSettings: vi.fn(async (): Promise<Settings> => ({
      autoProvider: true,
      outCrs: "WGS84",
      allowCrossBorder: false,
      tileSource: "arcgis-street",
      defaultProviders: [],
      showLogs: false,
      longWalkMeters: 500,
      manyTransfers: 2,
    })),
    saveSettings: vi.fn(async (s) => s),
    readLogs: vi.fn(async () => []),
    listLlmProviders: vi.fn(async () => []),
    saveLlmProvider: vi.fn(async () => []),
    deleteLlmProvider: vi.fn(async () => []),
    fetchLlmModels: vi.fn(async () => []),
    saveLlmModels: vi.fn(async () => []),
    listConversations: vi.fn(async () => []),
    readConversation: vi.fn(async () => []),
    deleteConversation: vi.fn(async () => undefined),
    chatStart: vi.fn(async () => ({
      streamId: "s1",
      convId: "c1",
      userMessage: message("c1-u", "c1", "user", "你好"),
    })),
    chatCancel: vi.fn(async () => undefined),
    onChatEvent: vi.fn(async () => () => {}),
    listMcpServers: vi.fn(async () => []),
    saveMcpServer: vi.fn(async () => []),
    deleteMcpServer: vi.fn(async () => []),
    listMcpTools: vi.fn(async () => []),
    ...overrides,
  };
}

/** 选好模型的 state。没有模型时 `send` 会直接拒发，那是另一条用例。 */
function ready(client: PlannerClient): ChatState {
  const state = new ChatState(client);
  state.model = { providerId: "p1", model: "gpt-x" };
  return state;
}

describe("ChatState", () => {
  it("send 先插入用户气泡再发请求", () => {
    // 永不 settle：模拟请求在飞，此刻界面上必须已经有自己刚打的字。
    const pending = Promise.withResolvers<never>();
    const client = fakeClient({ chatStart: vi.fn(() => pending.promise) });
    const state = ready(client);

    void state.send("从光谷去汉口");

    expect(state.messages.map((m) => [m.role, m.content])).toEqual([["user", "从光谷去汉口"]]);
    expect(state.sending).toBe(true);
  });

  it("起流失败时那条留在列表里并标成失败", async () => {
    const client = fakeClient({
      chatStart: vi.fn(async () => {
        throw { code: "LLM_KEY_REQUIRED", message: "还没填 API Key" };
      }),
    });
    const state = ready(client);

    await state.send("在吗");

    expect(state.messages.length).toBe(1);
    expect(state.messages[0]?.content).toBe("在吗");
    expect(state.messages[0]?.failed).toBe(true);
    expect(state.error).toBe("还没填 API Key");
    expect(state.sending).toBe(false);
  });

  it("sending 期间的第二次 send 不发请求", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);

    await state.send("第一句");
    await state.send("第二句");

    expect(harness.client.chatStart).toHaveBeenCalledTimes(1);
    expect(state.messages.map((m) => m.content)).toEqual(["你好"]);
  });

  it("首轮 send 后当前会话 id 来自后端", async () => {
    const harness = streamHarness("conv-new", "你好");
    harness.client.listConversations = vi.fn(async () => [conversation("conv-new", "你好")]);
    const state = ready(harness.client);
    expect(state.currentId).toBeNull();

    await state.send("你好");

    expect(state.currentId).toBe("conv-new");
    expect(harness.client.chatStart).toHaveBeenCalledWith(null, "你好", {
      providerId: "p1",
      model: "gpt-x",
    });
    expect(state.currentTitle).toBe("你好");
  });

  it("落库的用户消息替换乐观气泡，不留重复", async () => {
    const harness = streamHarness("c1", "从光谷去汉口");
    const state = ready(harness.client);

    await state.send("从光谷去汉口");

    // 乐观那条的 id 是本地生成的，被换成后端的 `c1-u`。
    expect(state.messages.map((m) => m.id)).toEqual(["c1-u"]);
  });

  it("done 事件把落库的几条追加到列表末尾", async () => {
    const harness = streamHarness("c1", "从光谷去汉口");
    const state = ready(harness.client);
    await state.send("从光谷去汉口");

    harness.emit({ kind: "delta", text: "查" });
    harness.emit({
      kind: "done",
      messages: [
        message("m2", "c1", "tool", '{"routes":[]}'),
        message("m3", "c1", "assistant", "查到了"),
      ],
    });

    expect(state.messages.map((m) => m.id)).toEqual(["c1-u", "m2", "m3"]);
    // 累加出来的临时正文必须清掉，否则它与落库那条同时显示，正文出现两遍。
    expect(state.streaming).toBe("");
    expect(state.sending).toBe(false);
  });

  it("续聊时把已有会话 id 发给后端，历史留在前面", async () => {
    const harness = streamHarness("c1", "下一句");
    harness.client.readConversation = vi.fn(async () => [
      message("old", "c1", "user", "上一句"),
    ]);
    const state = ready(harness.client);
    await state.openConversation("c1");

    await state.send("下一句");
    harness.emit({ kind: "done", messages: [message("m10", "c1", "assistant", "答")] });

    expect(harness.client.chatStart).toHaveBeenCalledWith("c1", "下一句", {
      providerId: "p1",
      model: "gpt-x",
    });
    expect(state.messages.map((m) => m.id)).toEqual(["old", "c1-u", "m10"]);
  });

  it("没选模型时不发请求", async () => {
    const client = fakeClient();
    const state = new ChatState(client);

    await state.send("你好");

    expect(client.chatStart).not.toHaveBeenCalled();
    expect(state.error.length).toBeGreaterThan(0);
    expect(state.messages).toEqual([]);
  });

  it("空白文本不发请求也不插气泡", async () => {
    const client = fakeClient();
    const state = ready(client);

    await state.send("   \n ");

    expect(client.chatStart).not.toHaveBeenCalled();
    expect(state.messages).toEqual([]);
  });

  it("重发把失败那条换成后端落库的结果", async () => {
    const handlers: ((event: ChatStreamEvent) => void)[] = [];
    let fail = true;
    const client = fakeClient({
      chatStart: vi.fn(async () => {
        if (fail) {
          fail = false;
          throw new Error("网络断了");
        }
        return {
          streamId: "s2",
          convId: "c1",
          userMessage: message("c1-u", "c1", "user", "重试这句"),
        };
      }),
      onChatEvent: vi.fn(async (_id: string, h: (event: ChatStreamEvent) => void) => {
        handlers.push(h);
        return vi.fn();
      }),
    });
    const state = ready(client);
    await state.send("重试这句");
    const failedId = state.messages[0]?.id ?? "";

    await state.retry(failedId);
    handlers.at(-1)?.({ kind: "done", messages: [message("m2", "c1", "assistant", "这次成了")] });

    expect(client.chatStart).toHaveBeenCalledTimes(2);
    expect(state.messages.map((m) => [m.role, m.content])).toEqual([
      ["user", "重试这句"],
      ["assistant", "这次成了"],
    ]);
    expect(state.messages.some((m) => m.failed)).toBe(false);
  });

  it("sending 期间的重发不发请求", async () => {
    const pending = Promise.withResolvers<never>();
    const client = fakeClient({ chatStart: vi.fn(() => pending.promise) });
    const state = ready(client);
    void state.send("第一句");
    const id = state.messages[0]?.id ?? "";

    await state.retry(id);

    expect(client.chatStart).toHaveBeenCalledTimes(1);
  });

  it("打开会话时读历史，失败则给可读提示", async () => {
    const client = fakeClient({
      readConversation: vi.fn(async () => {
        throw { code: "DB", message: "库读不了" };
      }),
    });
    const state = new ChatState(client);

    await state.openConversation("c1");

    expect(state.currentId).toBe("c1");
    expect(state.messages).toEqual([]);
    expect(state.error).toBe("库读不了");
    expect(state.loading).toBe(false);
  });

  it("删除当前会话后清空消息与当前 id", async () => {
    const client = fakeClient({
      listConversations: vi.fn(async () => [conversation("c1"), conversation("c2")]),
      readConversation: vi.fn(async () => [message("m1", "c1", "user", "喂")]),
    });
    const state = new ChatState(client);
    await state.loadConversations();
    await state.openConversation("c1");

    await state.deleteConversation("c1");

    expect(client.deleteConversation).toHaveBeenCalledWith("c1");
    expect(state.currentId).toBeNull();
    expect(state.messages).toEqual([]);
    expect(state.list.map((c) => c.id)).toEqual(["c2"]);
  });

  it("删除失败时列表不动", async () => {
    const client = fakeClient({
      listConversations: vi.fn(async () => [conversation("c1")]),
      deleteConversation: vi.fn(async () => {
        throw new Error("删不掉");
      }),
    });
    const state = new ChatState(client);
    await state.loadConversations();

    await state.deleteConversation("c1");

    expect(state.list.map((c) => c.id)).toEqual(["c1"]);
    expect(state.error).toBe("删不掉");
  });

  it("新对话清掉当前会话与消息，但不建库记录", async () => {
    const client = fakeClient({ readConversation: vi.fn(async () => [message("m1", "c1", "user", "喂")]) });
    const state = new ChatState(client);
    await state.openConversation("c1");

    state.newConversation();

    expect(state.currentId).toBeNull();
    expect(state.messages).toEqual([]);
    expect(state.currentTitle).toBe("新对话");
  });

  it("delta 逐块累加而不是相互覆盖", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("你好");

    harness.emit({ kind: "delta", text: "建议" });
    harness.emit({ kind: "delta", text: "走 2 号线" });

    expect(state.streaming).toBe("建议走 2 号线");
  });

  it("工具调用期间显示具名动作，拿到正文后收掉", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("从光谷去汉口");

    harness.emit({ kind: "toolStart", tool: "plan_route", args: "{}" });
    expect(state.toolHint).toBe("正在规划路线…");

    harness.emit({ kind: "delta", text: "查到了" });
    expect(state.toolHint).toBe("");
  });

  it("没登记的工具名退回通用文案，不泄露内部标识符", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("你好");

    harness.emit({ kind: "toolStart", tool: "mcp_baidu_weather", args: "{}" });

    expect(state.toolHint).toBe("正在查询…");
    expect(state.toolHint).not.toContain("mcp_");
  });

  it("toolEnd 带 plan 时回灌进 planner 并选中首个方案", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("从光谷去汉口");

    // 单例是跨用例共享的：不清干净,残留的旧 plan 会让这条用例在回灌坏掉时依然绿。
    planner.plan = null;
    planner.selectedRouteId = null;

    harness.emit({ kind: "toolEnd", tool: "plan_route", ok: true, summary: "找到 1 条路线", plan: chatPlan() });

    const restoredPlan = planner.plan as RoutePlan | null;
    expect(restoredPlan).not.toBeNull();
    if (!restoredPlan) throw new Error("toolEnd 没有回灌路线");
    expect(restoredPlan.routes[0]?.segments[0]?.points).toHaveLength(3);
    expect(planner.selectedRouteId).toBe("r0");
    expect(planner.fromText).toBe("光谷广场");
    expect(planner.toText).toBe("汉口火车站");
    expect(state.hasRouteResult).toBe(true);
  });

  it("切换历史再切回时恢复该会话的路线地图", async () => {
    const harness = streamHarness("a1", "从光谷去汉口", {
      readConversation: vi.fn(async (id: string) => [message(`${id}-m`, id, "user", "历史消息")]),
    });
    const state = ready(harness.client);
    await state.send("从光谷去汉口");
    harness.emit({ kind: "toolEnd", tool: "plan_route", ok: true, summary: "找到路线", plan: chatPlan() });

    await state.openConversation("b1");
    expect(state.hasRouteResult).toBe(false);
    await state.openConversation("a1");

    expect(state.hasRouteResult).toBe(true);
    expect(planner.plan?.routes[0]?.segments[0]?.points).toHaveLength(3);
    expect(planner.fromText).toBe("光谷广场");
    expect(planner.toText).toBe("汉口火车站");
  });

  it("toolEnd 带 circle 时回灌 circleResults", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("附近有什么医院");

    harness.emit({
      kind: "toolEnd",
      tool: "life_circle",
      ok: true,
      summary: "找到 1 个地点",
      circle: { package: "care", crs: "BD09", limitMinutes: 15, pois: [chatPoi("同济医院")] },
    });

    expect(planner.reachablePois.map((p) => p.name)).toContain("同济医院");
  });

  it("search_place 的 toolEnd 不动地图", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("光谷在哪");

    planner.plan = null;
    planner.circleResults = [];
    harness.emit({ kind: "toolEnd", tool: "search_place", ok: true, summary: "找到 3 个地点" });

    // 候选点是中间步骤，画一堆点只会干扰（§6.4.2）。
    expect(planner.plan).toBeNull();
    expect(planner.circleResults).toEqual([]);
  });

  it("工具失败时不动地图，免得把上一轮的路线当成这次的结果", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("从火星去汉口");

    planner.plan = null;
    harness.emit({ kind: "toolEnd", tool: "plan_route", ok: false, summary: "没找到起点" });

    expect(planner.plan).toBeNull();
  });

  it("中途失败时已流出的正文留在列表里", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("你好");

    harness.emit({ kind: "delta", text: "建议走 2 号线，然后" });
    harness.emit({ kind: "error", code: "UPSTREAM", message: "上游 502" });

    // 模型已经说出来的话不能凭空消失 —— 用户看见过它。
    expect(state.messages.map((m) => [m.role, m.content])).toEqual([
      ["user", "你好"],
      ["assistant", "建议走 2 号线，然后"],
    ]);
    expect(state.error).toBe("上游 502");
    expect(state.sending).toBe(false);
    expect(state.streaming).toBe("");
  });

  it("停止后立刻能再发一句", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("第一句");
    harness.emit({ kind: "delta", text: "正在说" });

    await state.stop();

    expect(state.sending).toBe(false);
    await state.send("第二句");
    expect(harness.client.chatStart).toHaveBeenCalledTimes(2);
    expect(harness.client.chatCancel).toHaveBeenCalledWith("s1");
  });

  it("停止后到达的事件一概不改界面", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("你好");
    harness.emit({ kind: "delta", text: "说了一半" });
    await state.stop();
    const after = state.messages.map((m) => m.content);

    // 后端那边 `CANCELLED` 与残余 delta 仍可能在路上。
    harness.emit({ kind: "delta", text: "不该出现" });
    harness.emit({ kind: "error", code: "CANCELLED", message: "已取消本次回答。" });

    expect(state.messages.map((m) => m.content)).toEqual(after);
    expect(state.streaming).toBe("");
    expect(state.error).toBe("");
  });

  it("流结束后退订，监听器不越积越多", async () => {
    const harness = streamHarness();
    const state = ready(harness.client);
    await state.send("你好");

    harness.emit({ kind: "done", messages: [message("m2", "c1", "assistant", "好")] });

    expect(harness.unlisten).toHaveBeenCalledTimes(1);
  });
});
