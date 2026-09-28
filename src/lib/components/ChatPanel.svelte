<script lang="ts">
  /**
   * 对话视图：左边会话列表，右边消息流，底下输入框。
   *
   * 状态在 `chat.svelte.ts`，这里只负责呈现与键鼠/滚动这类纯界面行为。
   * 模型下拉取自设置里配好的入口（`llm.svelte.ts`），没配就只显示一句提示 ——
   * 给一个空下拉框会让用户以为程序坏了。
   */
  import { chat } from "$lib/state/chat.svelte";
  import { llm } from "$lib/state/llm.svelte";
  import { cn } from "$lib/utils/cn";
  import Button from "$lib/components/ui/button/Button.svelte";
  import Select from "$lib/components/ui/select/Select.svelte";
  import ChatBubble from "$lib/components/ChatBubble.svelte";

  let draft = $state("");
  let scroller = $state<HTMLDivElement | null>(null);
  /**
   * 是否贴在底部。只有贴底时才自动滚 —— 用户往上翻历史的时候把他拽回来，
   * 是聊天界面最招人烦的一种「体贴」。
   */
  let atBottom = $state(true);
  /**
   * 贴底判定的容差（像素）。取 48 ≈ 一行气泡的高度：小数缩放下
   * `scrollHeight - scrollTop - clientHeight` 常有零点几像素的残差，
   * 严格判 0 会让自动滚动时不时失效；给到一整行又会把「刚翻上去一点」
   * 误判成贴底。
   */
  const STICK_THRESHOLD_PX = 48;

  /** 输入法组字期间的 Enter 是「选字」，不是「发送」。 */
  let composing = $state(false);

  const models = $derived(
    llm.list
      .filter((provider) => provider.enabled)
      .flatMap((provider) =>
        provider.models.map((model) => ({
          value: `${provider.id}::${model.model}`,
          label: `${provider.name} · ${model.alias ?? model.model}`,
        })),
      ),
  );

  const selected = $derived(chat.model ? `${chat.model.providerId}::${chat.model.model}` : "");

  $effect(() => {
    void chat.loadConversations();
    if (llm.list.length === 0) void llm.load();
  });

  // 默认选中第一个可用模型：让用户为了发第一句话先去下拉框点一下，是白加的一步。
  $effect(() => {
    const first = models[0];
    if (!chat.model && first) pickModel(first.value);
  });

  // 消息或流入的正文变了就滚到底，但只在原本贴底时。两者都要被读到才能建立依赖 ——
  // 只读 `messages.length` 的话，流式期间正文在长而视图不动。
  $effect(() => {
    const count = chat.messages.length;
    const streamed = chat.streaming.length;
    if ((count === 0 && streamed === 0) || !atBottom || !scroller) return;
    scroller.scrollTop = scroller.scrollHeight;
  });

  function onScroll(e: Event & { currentTarget: HTMLDivElement }): void {
    const el = e.currentTarget;
    atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= STICK_THRESHOLD_PX;
  }

  function pickModel(value: string): void {
    const [providerId, ...rest] = value.split("::");
    const model = rest.join("::");
    if (!providerId || model.length === 0) return;
    chat.model = { providerId, model };
  }

  function submit(): void {
    if (chat.sending || draft.trim().length === 0) return;
    const text = draft;
    // 先清空：发送是乐观的，输入框留着旧文本会让人以为没发出去。
    draft = "";
    // 自己发言时总是跟到底部，哪怕刚才正在翻历史。
    atBottom = true;
    void chat.send(text);
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key !== "Enter" || e.shiftKey || composing || e.isComposing) return;
    e.preventDefault();
    submit();
  }

  /** unix 秒 → 时分。列表里日期全是最近几天，年月日占宽度换不来信息。 */
  function clock(sec: number): string {
    return new Date(sec * 1000).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  }
</script>

<div class="flex min-h-0 flex-1 flex-col lg:flex-row">
  <aside class="flex w-full shrink-0 flex-col border-b lg:w-[260px] lg:border-b-0 lg:border-r" aria-label="对话列表">
    <div class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2">
      <h2 class="text-xs font-semibold text-muted-foreground">对话</h2>
      <Button variant="outline" size="sm" onclick={() => chat.newConversation()}>新对话</Button>
    </div>
    {#if chat.list.length === 0}
      <p class="px-3 py-3 text-[11px] text-muted-foreground">
        {chat.loading ? "正在读取…" : "还没有对话。在右边说一句话就会建一个。"}
      </p>
    {:else}
      <ul class="min-h-0 flex-1 overflow-y-auto">
        {#each chat.list as item (item.id)}
          {@const current = chat.currentId === item.id}
          <li class="flex items-center gap-1 border-b px-1.5 py-1">
            <button
              type="button"
              class={cn(
                "min-w-0 flex-1 rounded-md px-2 py-1.5 text-left text-xs transition-colors",
                "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                current ? "bg-accent font-medium text-accent-foreground" : "hover:bg-accent/50",
              )}
              aria-current={current ? "true" : undefined}
              onclick={() => void chat.openConversation(item.id)}
            >
              <span class="block truncate">{item.title}</span>
              <span class="mt-0.5 block text-[11px] text-muted-foreground">{clock(item.updatedAt)}</span>
            </button>
            <Button
              variant="ghost"
              size="sm"
              title="删除这个对话"
              ariaLabel={`删除对话「${item.title}」`}
              onclick={() => void chat.deleteConversation(item.id)}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2m-9 0 1 14h8l1-14" /></svg>
            </Button>
          </li>
        {/each}
      </ul>
    {/if}
  </aside>

  <section class="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="对话内容">
    <div class="flex shrink-0 items-center gap-2 border-b px-4 py-2">
      <h2 class="min-w-0 flex-1 truncate text-sm font-semibold">{chat.currentTitle}</h2>
      {#if models.length === 0}
        <span class="text-[11px] text-warning">还没有可用模型，先去设置里添加入口并挑模型。</span>
      {:else}
        <label class="text-[11px] text-muted-foreground" for="chat-model">模型</label>
        <Select
          id="chat-model"
          class="w-auto max-w-[220px]"
          options={models}
          value={selected}
          disabled={chat.sending}
          onchange={(e) => pickModel(e.currentTarget.value)}
        />
      {/if}
    </div>

    <div
      bind:this={scroller}
      onscroll={onScroll}
      class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4"
      role="log"
      aria-live="polite"
      aria-label="消息记录"
    >
      {#if chat.messages.length === 0}
        <div class="m-auto max-w-sm rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">
          <p class="text-sm font-medium text-foreground">问一句出行的事</p>
          <p class="mt-1">比如「从光谷广场去汉口火车站，腿脚不便走哪条」。</p>
          <p class="mt-1">它会调用算路与生活圈工具，结果同时画到地图上。</p>
        </div>
      {:else}
        {#each chat.messages as message (message.id)}
          <ChatBubble {message} onretry={(id) => void chat.retry(id)} />
        {/each}
      {/if}
      {#if chat.streaming}
        <!-- 还没落库的正文。落库后由 `done` 事件换成 `messages` 里的真气泡，
             所以这里不复用 ChatBubble：那个组件要求一条有真实 id 的消息。 -->
        <div class="max-w-[85%] self-start rounded-lg bg-muted px-3 py-2 text-sm whitespace-pre-wrap">
          {chat.streaming}
        </div>
      {/if}
      {#if chat.sending && chat.toolHint}
        <p class="text-[11px] text-muted-foreground">{chat.toolHint}</p>
      {:else if chat.sending && !chat.streaming}
        <p class="text-[11px] text-muted-foreground">正在思考…</p>
      {/if}
    </div>

    {#if chat.error}
      <p class="mx-4 mb-2 shrink-0 rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">{chat.error}</p>
    {/if}

    <div class="flex shrink-0 items-end gap-2 border-t p-3">
      <textarea
        bind:value={draft}
        onkeydown={onKeydown}
        oncompositionstart={() => (composing = true)}
        oncompositionend={() => (composing = false)}
        rows="2"
        aria-label="输入消息"
        placeholder="Enter 发送，Shift+Enter 换行"
        class={cn(
          "min-h-[44px] flex-1 resize-none rounded-md border border-input bg-card px-3 py-2 text-sm",
          "placeholder:text-muted-foreground",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
        )}
      ></textarea>
      {#if chat.sending}
        <!-- 停止键顶掉发送键（§8.2 Loading 态）。输入框上面**不禁用**：
             允许用户在等回答的时候先把下一句打出来。 -->
        <Button variant="outline" onclick={() => void chat.stop()}>停止</Button>
      {:else}
        <Button disabled={draft.trim().length === 0} onclick={submit}>发送</Button>
      {/if}
    </div>
  </section>
</div>
