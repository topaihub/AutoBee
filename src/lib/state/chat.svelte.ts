/**
 * 对话页的状态。组件只读这里的字段、只调这里的方法，IPC 一律经 `PlannerClient`。
 *
 * 一轮对话是**流式**的：`chatStart` 立刻返回一个 `streamId`，正文与工具进展
 * 随 `llm:chat:<streamId>` 事件陆续到达。`streaming` 里累加的是还没落库的正文，
 * `done` 事件到了才换成库里那几条。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { ChatMessage, ChatStreamEvent, Conversation, ModelRef, RoutePlan } from "../ipc/types";
import { errorMessage, planner } from "./planner.svelte";

/**
 * 界面上的一条消息。
 *
 * 比 `ChatMessage` 多一个 `failed`：发送失败的用户气泡**留在列表里**并可重发。
 * 静默删掉会让用户以为自己没发出去，于是重打一遍。
 */
export type ChatItem = ChatMessage & {
  failed?: boolean;
};

/** 本地临时 id。后端落库那条有真实 id，这个只用来在列表里定位乐观插入的气泡。 */
function localId(): string {
  return `local-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * 工具名 → 界面上的一句人话。
 *
 * 具名动作而不是转圈图标（§8.2 末段）：工具调用可能停顿几秒，用户看不出
 * 程序在做什么还是卡死了。名字对不上时退回一句通用的，不显示原始标识符 ——
 * `mcp_xxx_yyy` 这种内部名字对用户没有意义。
 */
const TOOL_LABELS: Record<string, string> = {
  plan_route: "正在规划路线…",
  search_place: "正在查找地点…",
  life_circle: "正在看周边设施…",
};

export class ChatState {
  list = $state<Conversation[]>([]);
  /** null = 新会话，还没落库。首轮 `send` 之后换成后端给的 `convId`。 */
  currentId = $state<string | null>(null);
  messages = $state<ChatItem[]>([]);
  /** 当前对话是否产生过路线工具结果；避免把安心换乘的旧路线误显示在聊天页。 */
  hasRouteResult = $state(false);

  /** 一轮对话在飞。为真时不许再发 —— 同一个会话并发两轮，历史顺序会错乱。 */
  sending = $state(false);
  /**
   * 正在流入的回复正文。空串 = 还没有正文（可能正在调工具）。
   *
   * 与 `messages` 分开：这段还没落库，没有真实 id。混进 `messages` 就得为它
   * 编一个临时 id，`done` 之后再摘掉，多一次容易出错的对账。
   */
  streaming = $state("");
  /**
   * 工具调用期间的具名提示，如「正在查询路线…」。空串 = 没有工具在跑。
   *
   * 必须是具名动作而不是转圈图标（§8.2 末段）：工具调用可能停顿几秒，
   * 用户看不出程序在做什么还是卡死了。
   */
  toolHint = $state("");
  /** 会话列表 / 历史正在读。与 `sending` 分开：两者能同时为真。 */
  loading = $state(false);
  error = $state("");

  /** 选中的模型。没选中时 `send` 直接提示，不发请求。 */
  model = $state<ModelRef | null>(null);

  /** 当前流的 id，给「停止」用。null = 没有流在跑。 */
  #streamId: string | null = null;
  /**
   * 本次运行期间收到的完整路线，按会话隔离。
   * SQLite 只存裁剪后的 tool 摘要，所以应用重启后不能伪造路线地图。
   */
  #routePlans = new Map<string, RoutePlan>();
  /** 当前流的退订函数。流结束或被取消时必须调，否则监听器越积越多。 */
  #unlisten: (() => void) | null = null;

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  async loadConversations(): Promise<void> {
    this.loading = true;
    this.error = "";
    try {
      this.list = await this.client.listConversations();
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  /** 打开一个会话。先切 id 再读历史，读的过程中标题栏就已经是对的了。 */
  async openConversation(id: string): Promise<void> {
    this.currentId = id;
    this.messages = [];
    const cachedPlan = this.#routePlans.get(id);
    this.hasRouteResult = cachedPlan !== undefined;
    if (cachedPlan) planner.applyPlan(cachedPlan, true);
    this.loading = true;
    this.error = "";
    try {
      this.messages = await this.client.readConversation(id);
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  newConversation(): void {
    this.currentId = null;
    this.messages = [];
    this.hasRouteResult = false;
  }

  async deleteConversation(id: string): Promise<void> {
    this.error = "";
    try {
      await this.client.deleteConversation(id);
    } catch (e) {
      this.error = errorMessage(e);
      return;
    }
    this.list = this.list.filter((item) => item.id !== id);
    this.#routePlans.delete(id);
    if (this.currentId === id) this.newConversation();
  }

  /**
   * 发一轮。**在第一个事件到达之前就返回** —— 剩下的靠事件推进。
   *
   * 用户气泡先插进列表再发请求：等后端回来才显示自己刚打的字，界面看着像卡住了。
   */
  async send(text: string): Promise<void> {
    const content = text.trim();
    if (content.length === 0) return;
    if (this.sending) return;
    const model = this.model;
    if (!model) {
      this.error = "请先在设置里配置并选择一个模型。";
      return;
    }

    this.sending = true;
    this.error = "";
    this.streaming = "";
    this.toolHint = "";
    const convId = this.currentId;
    const optimistic = this.#pushOptimistic(content);

    try {
      const handle = await this.client.chatStart(convId, content, model);
      // 订阅要在拿到 id 之后、且**不能等第一个事件**：这里 await 的是「监听器装好了」。
      this.#streamId = handle.streamId;
      this.#unlisten = await this.client.onChatEvent(handle.streamId, (event) => {
        this.#onEvent(event);
      });
      // 乐观那条换成后端落库的（真实 id 与 createdAt）。两条都留着会出现重复气泡。
      this.messages = this.messages.map((item) =>
        item.id === optimistic.id ? handle.userMessage : item,
      );
      // 首轮的会话是后端新建的，前端不知道 id，只能从返回值里取。
      this.currentId = handle.convId;
      // 新会话要在侧栏出现，已有会话的 updatedAt 也变了（列表按它排序）。
      await this.loadConversations();
    } catch (e) {
      // 起流本身失败（空输入、没 Key、SQL 失败）：不会有任何事件到来，这里收尾。
      this.error = errorMessage(e);
      this.messages = this.messages.map((item) =>
        item.id === optimistic.id ? { ...item, failed: true } : item,
      );
      this.#finish();
    }
  }

  /**
   * 点「停止」。
   *
   * 不等后端确认就把本地状态收掉：用户按了停止就该立刻能打下一句。后端那边
   * `CANCELLED` 事件随后会到，那时流已经不在了，`#onEvent` 直接忽略。
   */
  async stop(): Promise<void> {
    const streamId = this.#streamId;
    if (!streamId) return;
    // 先摘本地：`#finish` 会退订，之后来的事件一概收不到。
    this.#keepStreamedText();
    this.#finish();
    try {
      await this.client.chatCancel(streamId);
    } catch {
      // 流可能刚好自己结束了（`STREAM_NOT_FOUND`）。用户要的效果已经达到，不报错。
    }
  }

  /** 重发失败的那条：把它连同后面的残留一起摘掉，再当成新消息发一次。 */
  async retry(id: string): Promise<void> {
    if (this.sending) return;
    const at = this.messages.findIndex((item) => item.id === id);
    if (at < 0) return;
    const text = this.messages[at]?.content ?? "";
    this.messages = this.messages.slice(0, at);
    await this.send(text);
  }

  /** 当前会话的标题，给标题栏用。新会话在侧栏还没有条目。 */
  get currentTitle(): string {
    return this.list.find((item) => item.id === this.currentId)?.title ?? "新对话";
  }

  /**
   * 乐观插入用户气泡。
   *
   * `convId` 用当前的（新会话是空串）：这条马上会被后端那条替换掉，
   * 界面也不读它，真正要紧的是 `id` 能在替换时定位回来。
   */
  #pushOptimistic(content: string): ChatItem {
    const item: ChatItem = {
      id: localId(),
      convId: this.currentId ?? "",
      role: "user",
      content,
      createdAt: Math.floor(Date.now() / 1000),
    };
    this.messages = [...this.messages, item];
    return item;
  }

  /**
   * 处理一条流式事件。
   *
   * 流已经收掉时（用户点过停止）一概忽略：那之后到达的 `CANCELLED` 与残余 `delta`
   * 都属于上一轮，追加进去会让界面上出现用户已经叫停的内容。
   */
  #onEvent(event: ChatStreamEvent): void {
    if (!this.#streamId) return;
    switch (event.kind) {
      case "delta":
        // 追加而不是替换：后端发的是增量。
        this.streaming += event.text;
        // 有正文了说明工具那步过去了。
        this.toolHint = "";
        break;
      case "toolStart":
        this.toolHint = TOOL_LABELS[event.tool] ?? "正在查询…";
        break;
      case "toolEnd":
        this.toolHint = "";
        // 回灌地图（§6.4.2）：算得出路线却画不出来，等于没解决问题。
        // 走 applyPlan / applyCircle 而不是直接赋值 —— 那两条路径还管选中项、
        if (event.plan) {
          planner.applyPlan(event.plan, true);
          if (this.currentId) this.#routePlans.set(this.currentId, event.plan);
          this.hasRouteResult = true;
        }
        if (event.circle) planner.applyCircle(event.circle);
        break;
      case "done":
        // 落库的那几条替换掉累加出来的临时正文：它们有真实 id，能做列表 key。
        this.messages = [...this.messages, ...event.messages];
        this.streaming = "";
        this.#finish();
        break;
      case "error":
        // 已经流出来的正文留着 —— 那部分是模型真说过的话，抹掉等于骗用户。
        this.#keepStreamedText();
        this.error = event.message;
        this.#finish();
        break;
    }
  }

  /**
   * 把已经流出来的正文钉成一条消息。
   *
   * 用在中途失败与用户取消：这两种情况下后端不会发 `done`，正文没有落库的 id。
   * 给一个本地 id 而不是丢掉：模型已经说出来的话在界面上突然消失，比留着更费解。
   */
  #keepStreamedText(): void {
    const text = this.streaming;
    if (text.length === 0) return;
    this.messages = [
      ...this.messages,
      {
        id: localId(),
        convId: this.currentId ?? "",
        role: "assistant",
        content: text,
        createdAt: Math.floor(Date.now() / 1000),
      },
    ];
    this.streaming = "";
  }

  /** 一轮结束（成功、失败、取消都走这里）：退订并清掉流式状态。 */
  #finish(): void {
    this.#unlisten?.();
    this.#unlisten = null;
    this.#streamId = null;
    this.sending = false;
    this.streaming = "";
    this.toolHint = "";
  }
}

/** 单例。组件从 $lib/state 引这个。 */
export const chat = new ChatState();
