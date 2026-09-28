<script module lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
</script>

<script lang="ts">
  /**
   * 设置弹窗：服务商 Key 配置（对标 ai-installer 的模型服务商配置页）+ 全局偏好。
   * 数据全走 ProvidersState，这里只做表单绑定与按钮回调。
   */
  import { providers } from "$lib/state/providers.svelte";
  import { llm } from "$lib/state/llm.svelte";
  import { mcp } from "$lib/state/mcp.svelte";
  import type { Crs, LlmProtocol, LlmProviderInput, MapProviderInput, McpServerInput, ProviderId } from "$lib/ipc/types";
  import { DEFAULT_LABEL_THRESHOLDS } from "$lib/domain/accessibility";
  import { partialKeyHint } from "$lib/domain/provider-profile";
  import Dialog from "$lib/components/ui/dialog/Dialog.svelte";
  import Button from "$lib/components/ui/button/Button.svelte";
  import Input from "$lib/components/ui/input/Input.svelte";
  import Label from "$lib/components/ui/label/Label.svelte";
  import Select from "$lib/components/ui/select/Select.svelte";
  import Separator from "$lib/components/ui/separator/Separator.svelte";
  import Switch from "$lib/components/ui/switch/Switch.svelte";
  import Badge from "$lib/components/ui/badge/Badge.svelte";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  /** 正在输入的明文服务端 Key，按 slug 记；保存后清空。 */
  let keyDrafts = $state<Record<string, string>>({});
  /** 正在输入的明文浏览器端 Key。只有百度这一格会被填。 */
  let browserKeyDrafts = $state<Record<string, string>>({});
  /** 每条档案本地编辑中的启用/优先级（点「保存」才落库）。 */
  let drafts = $state<Record<string, { enabled: boolean; priority: string }>>({});
  /** MCP server 本地编辑中的名称、地址与启用状态。 */
  let mcpDrafts = $state<Record<string, McpServerInput>>({});

  $effect(() => {
    if (open) {
      void providers.load();
      void llm.load();
      void mcp.load();
      keyDrafts = {};
      browserKeyDrafts = {};
      llmKeyDrafts = {};
      mcpDrafts = {};
    }
  });

  /** 正在输入的大模型明文 Key，按 id 记；保存后清空。 */
  let llmKeyDrafts = $state<Record<string, string>>({});

  /** 新建入口的表单。id 为空表示还没保存过。 */
  const 空白入口: LlmProviderInput = {
    name: "",
    baseUrl: "",
    protocol: "openai-chat",
    enabled: true,
    apiKey: "",
  };
  let newLlm = $state<LlmProviderInput>({ ...空白入口 });


  const llmProtocolOptions: Array<{ value: LlmProtocol; label: string }> = [
    { value: "openai-chat", label: "OpenAI 兼容（/chat/completions，多数中转商）" },
    { value: "openai-responses", label: "OpenAI Responses（/responses）" },
    { value: "anthropic-messages", label: "Anthropic（/messages）" },
  ];

  async function addLlm() {
    await llm.save({ ...newLlm, apiKey: newLlm.apiKey?.trim() || null });
    if (!llm.error) newLlm = { ...空白入口 };
  }

  async function saveLlm(p: (typeof llm.list)[number]) {
    const key = llmKeyDrafts[p.id]?.trim();
    await llm.save({
      id: p.id,
      name: p.name,
      baseUrl: p.baseUrl,
      protocol: p.protocol,
      enabled: p.enabled,
      // 空串表示没动过：框里显示的是掩码，原样提交不能把掩码写成真 Key。
      apiKey: key ? key : null,
    });
    delete llmKeyDrafts[p.id];
  }

  /** 勾选/取消一个模型后立刻覆盖保存 —— 多选框没有「保存」按钮。 */
  async function toggleModel(providerId: string, model: string, picked: boolean) {
    const current = llm.list.find((p) => p.id === providerId)?.models ?? [];
    const next = picked
      ? [...current.map((m) => ({ model: m.model, alias: m.alias, toolsOk: m.toolsOk })), { model, toolsOk: true }]
      : current.filter((m) => m.model !== model).map((m) => ({ model: m.model, alias: m.alias, toolsOk: m.toolsOk }));
    await llm.saveModels(providerId, next);
  }

  function mcpDraftOf(server: (typeof mcp.list)[number]): McpServerInput {
    return mcpDrafts[server.id] ?? {
      id: server.id,
      name: server.name,
      url: server.url,
      enabled: server.enabled,
    };
  }

  const BAIDU_MCP_ENDPOINT = "https://mcp.map.baidu.com/mcp";
  function isBaiduMcp(server: (typeof mcp.list)[number]): boolean {
    return server.url === BAIDU_MCP_ENDPOINT;
  }

  async function saveMcp(server: (typeof mcp.list)[number]) {
    await mcp.save(mcpDraftOf(server));
  }

  async function toggleMcp(server: (typeof mcp.list)[number], enabled: boolean) {
    await mcp.save({ ...mcpDraftOf(server), enabled });
  }

  const 空白Mcp: McpServerInput = { name: "", url: "", enabled: true };
  let newMcp = $state<McpServerInput>({ ...空白Mcp });

  async function addMcp() {
    await mcp.save({ ...newMcp, name: newMcp.name.trim(), url: newMcp.url.trim() });
    if (!mcp.error) newMcp = { ...空白Mcp };
  }

  // 列表加载后同步本地草稿（只在还没有草稿时初始化，避免覆盖用户输入）
  $effect(() => {
    for (const p of providers.list) {
      if (!drafts[p.slug]) {
        drafts[p.slug] = { enabled: p.enabled, priority: String(p.priority ?? p.defaultPriority) };
      }
    }
  });

  function draftOf(slug: string) {
    return (
      drafts[slug] ?? { enabled: false, priority: "0" }
    );
  }

  async function saveProvider(p: (typeof providers.list)[number]) {
    const d = draftOf(p.slug);
    const key = keyDrafts[p.slug]?.trim();
    const browserKey = browserKeyDrafts[p.slug]?.trim();
    const input: MapProviderInput = {
      slug: p.slug as ProviderId,
      name: p.name,
      enabled: d.enabled,
      priority: d.priority === "" ? null : Number(d.priority),
      apiKey: key ? key : null,
      browserKey: browserKey ? browserKey : null,
    };
    await providers.save(input);
    delete keyDrafts[p.slug];
    delete browserKeyDrafts[p.slug];
  }

  async function remove(slug: string) {
    await providers.remove(slug);
    delete drafts[slug];
    delete keyDrafts[slug];
    delete browserKeyDrafts[slug];
  }

  function updateSettings(patch: Partial<NonNullable<typeof providers.settings>>) {
    if (!providers.settings) return;
    void providers.saveSettings({ ...providers.settings, ...patch });
  }

  /**
   * 数字框读数。空串、负数、非数字都退回默认值 —— 存进去一个 `NaN` 会让
   * 每个阈值比较都变成 false，提醒标签从此永远不出现，而且界面上看不出异常。
   */
  function thresholdOf(e: Event & { currentTarget: HTMLInputElement }, fallback: number): number {
    const n = Number(e.currentTarget.value);
    return Number.isFinite(n) && n >= 0 ? Math.round(n) : fallback;
  }

  const crsOptions: Array<{ value: Crs; label: string }> = [
    { value: "WGS84", label: "WGS84（GPS / OSM）" },
    { value: "GCJ02", label: "GCJ02（火星坐标，高德/腾讯）" },
    { value: "BD09", label: "BD09（百度）" },
    { value: "CGCS2000", label: "CGCS2000（天地图）" },
  ];

  const tileOptions = [{ value: "arcgis-street", label: "ArcGIS 街道" }];
</script>

<Dialog bind:open title="设置">
  {#if providers.error}
    <p class="mb-3 rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">{providers.error}</p>
  {/if}
  {#if providers.saved}
    <p class="mb-3 rounded-md bg-success/10 px-3 py-2 text-xs text-success">已保存。</p>
  {/if}

  <section class="space-y-3">
    <h3 class="text-xs font-semibold text-muted-foreground">地图服务商</h3>
    {#if providers.loading}
      <p class="py-6 text-center text-xs text-muted-foreground">加载中…</p>
    {:else}
      <div class="space-y-2">
        {#each providers.list as p (p.slug)}
          <div class="rounded-lg border p-3">
            <div class="flex items-center justify-between gap-2">
              <div class="flex min-w-0 items-center gap-2">
                <Switch checked={drafts[p.slug] ? drafts[p.slug].enabled : p.enabled}
                  onchange={(v) => (drafts[p.slug] = { ...draftOf(p.slug), enabled: v })} />
                <span class="truncate text-sm font-medium">{p.name}</span>
                {#if p.isPreset}<Badge>内置</Badge>{/if}
                {#if p.implemented}
                  <Badge variant="success">可用</Badge>
                {:else if p.slug === "baidu"}
                  <Badge variant="success">生活圈可用</Badge>
                {:else}
                  <Badge variant="outline">待接入</Badge>
                {/if}
              </div>
              <div class="flex shrink-0 items-center gap-1">
                <span class="text-[11px] text-muted-foreground">优先级</span>
                <Input
                  class="h-7 w-14 px-2 text-xs"
                  type="number"
                  value={draftOf(p.slug).priority}
                  oninput={(e) => (drafts[p.slug] = { ...draftOf(p.slug), priority: e.currentTarget.value })}
                />
              </div>
            </div>

            <div class="mt-2 flex items-center gap-2">
              {#if p.slug === "baidu"}
                <!-- 百度要两把 AK：控制台里「浏览器端」与「服务端」是两种应用
                     类型，互相不能用。只给一个框的话用户必然填错一半，而症状
                     是「地图空白」或「搜索报 240」，看不出是填错了位置。 -->
                <div class="flex flex-1 flex-col gap-1.5">
                  <label class="flex items-center gap-2">
                    <span class="w-16 shrink-0 text-[11px] text-muted-foreground">服务端 AK</span>
                    <Input
                      class="h-8 flex-1 text-xs"
                      type="password"
                      placeholder={p.apiKeyMasked || "搜索、路线规划用"}
                      value={keyDrafts[p.slug] ?? ""}
                      oninput={(e) => (keyDrafts[p.slug] = e.currentTarget.value)}
                    />
                  </label>
                  <label class="flex items-center gap-2">
                    <span class="w-16 shrink-0 text-[11px] text-muted-foreground">浏览器 AK</span>
                    <Input
                      class="h-8 flex-1 text-xs"
                      type="password"
                      placeholder={p.browserKeyMasked || "地图渲染用（JS API GL）"}
                      value={browserKeyDrafts[p.slug] ?? ""}
                      oninput={(e) => (browserKeyDrafts[p.slug] = e.currentTarget.value)}
                    />
                  </label>
                </div>
              {:else}
                <Input
                  class="h-8 flex-1 text-xs"
                  type="password"
                  placeholder={p.apiKeyMasked || (p.slug === "amap" ? "填写 Web服务 API Key" : "填写 API Key")}
                  value={keyDrafts[p.slug] ?? ""}
                  oninput={(e) => (keyDrafts[p.slug] = e.currentTarget.value)}
                />
              {/if}
              {#if p.keyApplyUrl}
                <button
                  type="button"
                  class="shrink-0 text-xs text-primary underline-offset-2 hover:underline"
                  onclick={() => void openUrl(p.keyApplyUrl)}
                >申请 Key ↗</button>
              {/if}
              {#if p.slug !== "baidu"}
                <Button size="sm" variant="outline" onclick={() => void providers.test(p.slug)}>
                  {providers.testing[p.slug] ? "测试中…" : "测试"}
                </Button>
              {/if}
              <Button size="sm" onclick={() => void saveProvider(p)}>保存</Button>
              {#if !p.isPreset}
                <Button size="sm" variant="destructive" onclick={() => void remove(p.slug)}>删除</Button>
              {/if}
            </div>

            {#if providers.results[p.slug]?.message}
              <p class="mt-1.5 text-[11px] {providers.results[p.slug].ok ? 'text-success' : 'text-destructive'}">
                {providers.results[p.slug].message}
              </p>
            {/if}
            <p class="mt-1.5 text-[11px] text-muted-foreground">
              覆盖 {p.coverage} · 坐标系 {p.coordSys} · 能力 {p.capabilities}
            </p>
            {#if partialKeyHint(p)}
              <p class="mt-1.5 text-[11px] text-warning">{partialKeyHint(p)}</p>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <Separator class="my-4" />

  <section class="space-y-3">
    <h3 class="text-xs font-semibold text-muted-foreground">大模型服务</h3>
    {#if llm.error}
      <p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">{llm.error}</p>
    {/if}

    {#if llm.loading}
      <p class="py-6 text-center text-xs text-muted-foreground">加载中…</p>
    {:else}
      <div class="space-y-2">
        {#each llm.list as p (p.id)}
          <div class="rounded-lg border p-3">
            <div class="flex items-center justify-between gap-2">
              <div class="flex min-w-0 items-center gap-2">
                <Switch checked={p.enabled} onchange={(v) => void llm.save({ ...p, enabled: v, apiKey: null })} />
                <span class="truncate text-sm font-medium">{p.name}</span>
                {#if p.models.length > 0}
                  <Badge variant="success">{p.models.length} 个模型</Badge>
                {:else}
                  <Badge variant="outline">未挑模型</Badge>
                {/if}
              </div>
              <div class="flex shrink-0 items-center gap-1">
                <Button size="sm" variant="outline" onclick={() => void llm.fetchModels(p.id)}>
                  {llm.fetching[p.id] ? "获取中…" : "获取模型列表"}
                </Button>
                <Button size="sm" onclick={() => void saveLlm(p)}>保存</Button>
                <Button size="sm" variant="destructive" onclick={() => void llm.remove(p.id)}>删除</Button>
              </div>
            </div>

            <div class="mt-2 flex items-center gap-2">
              <Input class="h-8 flex-1 text-xs" value={p.baseUrl} ariaLabel="{p.name} 的 base_url"
                oninput={(e) => (p.baseUrl = e.currentTarget.value)} />
              <Input
                class="h-8 flex-1 text-xs"
                type="password"
                ariaLabel="{p.name} 的 API Key"
                placeholder={p.apiKeyMasked || "填写 API Key"}
                value={llmKeyDrafts[p.id] ?? ""}
                oninput={(e) => (llmKeyDrafts[p.id] = e.currentTarget.value)}
              />
            </div>

            {#if llm.fetchError[p.id]}
              <p class="mt-1.5 text-[11px] text-destructive" role="alert">{llm.fetchError[p.id]}</p>
            {/if}

            {#if llm.candidates[p.id]?.length}
              <div class="mt-2 max-h-40 space-y-1 overflow-y-auto rounded-md border bg-muted/30 p-2">
                {#each llm.candidates[p.id] as model (model)}
                  <label class="flex cursor-pointer items-center gap-2 text-xs">
                    <input
                      type="checkbox"
                      class="size-3.5"
                      checked={p.models.some((m) => m.model === model)}
                      onchange={(e) => void toggleModel(p.id, model, e.currentTarget.checked)}
                    />
                    <span class="truncate">{model}</span>
                  </label>
                {/each}
              </div>
            {:else if p.models.length > 0}
              <p class="mt-1.5 text-[11px] text-muted-foreground">
                已选：{p.models.map((m) => m.alias ?? m.model).join("、")}
              </p>
            {/if}
          </div>
        {/each}
      </div>

      <div class="rounded-lg border border-dashed p-3">
        <p class="text-xs font-medium">添加服务入口</p>
        <div class="mt-2 grid gap-2 sm:grid-cols-2">
          <div class="space-y-1">
            <Label for="llm-new-name">名称</Label>
            <Input id="llm-new-name" class="h-8 text-xs" placeholder="如「某中转」"
              value={newLlm.name} oninput={(e) => (newLlm.name = e.currentTarget.value)} />
          </div>
          <div class="space-y-1">
            <Label for="llm-new-url">base_url</Label>
            <Input id="llm-new-url" class="h-8 text-xs" placeholder="https://example.com/v1"
              value={newLlm.baseUrl} oninput={(e) => (newLlm.baseUrl = e.currentTarget.value)} />
          </div>
          <div class="space-y-1">
            <Label for="llm-new-proto">协议</Label>
            <Select id="llm-new-proto" options={llmProtocolOptions} value={newLlm.protocol}
              onchange={(e) => (newLlm.protocol = e.currentTarget.value as LlmProtocol)} />
          </div>
          <div class="space-y-1">
            <Label for="llm-new-key">API Key</Label>
            <Input id="llm-new-key" class="h-8 text-xs" type="password" placeholder="sk-…"
              value={newLlm.apiKey ?? ""} oninput={(e) => (newLlm.apiKey = e.currentTarget.value)} />
          </div>
        </div>
        <div class="mt-2 flex justify-end">
          <Button size="sm" onclick={() => void addLlm()}>添加</Button>
        </div>
        <p class="mt-1.5 text-[11px] text-muted-foreground">
          Key 用 Windows DPAPI 加密后落盘，settings.db 里搜不到明文。
        </p>
      </div>
    {/if}
  </section>

  <Separator class="my-4" />

  <section class="space-y-3">
    <div class="flex items-center justify-between gap-2">
      <div>
        <h3 class="text-xs font-semibold text-muted-foreground">MCP 工具服务</h3>
        <p class="mt-1 text-[11px] text-muted-foreground">只允许 HTTPS，或指向本机的 HTTP 服务。</p>
      </div>
      {#if mcp.saved}<Badge variant="success">已保存</Badge>{/if}
    </div>
    {#if mcp.error}
      <p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">{mcp.error}</p>
    {/if}
    {#if mcp.loading}
      <p class="py-4 text-center text-xs text-muted-foreground">加载中…</p>
    {:else}
      <div class="space-y-2">
        {#each mcp.list as server (server.id)}
          {@const draft = mcpDraftOf(server)}
          <div class="rounded-lg border p-3">
            <div class="flex items-center justify-between gap-2">
              <div class="flex min-w-0 items-center gap-2">
                <Switch checked={draft.enabled} onchange={(value) => void toggleMcp(server, value)} />
                <span class="truncate text-sm font-medium">{server.name}</span>
                {#if mcp.tools[server.id]}
                  <Badge variant="success">{mcp.tools[server.id].length} 个工具</Badge>
                {:else}
                  <Badge variant="outline">未连接</Badge>
                {/if}
              </div>
              <div class="flex shrink-0 items-center gap-1">
                <Button size="sm" variant="outline" onclick={() => void mcp.loadTools(server.id)}>
                  {mcp.fetching[server.id] ? "获取中…" : "读取工具"}
                </Button>
                <Button size="sm" onclick={() => void saveMcp(server)}>保存</Button>
                <Button size="sm" variant="destructive" onclick={() => void mcp.remove(server.id)}>删除</Button>
              </div>
            </div>
            {#if isBaiduMcp(server)}
              <p class="mt-2 text-[11px] text-muted-foreground">复用“百度地图”服务商中已配置的服务端 AK。</p>
            {:else}
              <div class="mt-2 grid gap-2 sm:grid-cols-2">
                <Input class="h-8 text-xs" ariaLabel="MCP server 名称" value={draft.name}
                  oninput={(event) => (mcpDrafts[server.id] = { ...draft, name: event.currentTarget.value })} />
                <Input class="h-8 text-xs" ariaLabel="MCP server 地址" value={draft.url}
                  oninput={(event) => (mcpDrafts[server.id] = { ...draft, url: event.currentTarget.value })} />
              </div>
            {/if}
            {#if mcp.toolErrors[server.id]}
              <p class="mt-1.5 text-[11px] text-destructive" role="alert">{mcp.toolErrors[server.id]}</p>
            {/if}
            {#if mcp.tools[server.id]?.length}
              <div class="mt-2 max-h-32 space-y-1 overflow-y-auto rounded-md border bg-muted/30 p-2">
                {#each mcp.tools[server.id] as tool (tool.name)}
                  <div class="text-[11px]">
                    <span class="font-medium">{tool.name}</span>
                    {#if tool.description}<span class="text-muted-foreground"> — {tool.description}</span>{/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
      <div class="rounded-lg border border-dashed p-3">
        <p class="text-xs font-medium">添加 MCP server</p>
        <div class="mt-2 grid gap-2 sm:grid-cols-2">
          <Input class="h-8 text-xs" ariaLabel="新 MCP server 名称" placeholder="如 baidu"
            value={newMcp.name} oninput={(event) => (newMcp.name = event.currentTarget.value)} />
          <Input class="h-8 text-xs" ariaLabel="新 MCP server 地址" placeholder="https://example.com/mcp"
            value={newMcp.url} oninput={(event) => (newMcp.url = event.currentTarget.value)} />
        </div>
        <div class="mt-2 flex justify-end">
          <Button size="sm" disabled={mcp.saving} onclick={() => void addMcp()}>{mcp.saving ? "保存中…" : "添加"}</Button>
        </div>
      </div>
    {/if}
  </section>

  <Separator class="my-4" />

  <section class="space-y-3">
    <h3 class="text-xs font-semibold text-muted-foreground">规划偏好</h3>
    {#if providers.settings}
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm">自动选择服务商</p>
          <p class="text-[11px] text-muted-foreground">按起点所在地区自动挑选，失败时按优先级依次降级。</p>
        </div>
        <Switch
          checked={providers.settings.autoProvider}
          onchange={(v) => updateSettings({ autoProvider: v })}
        />
      </div>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm">允许跨国路线</p>
          <p class="text-[11px] text-muted-foreground">关闭后起终点跨服务商覆盖区时直接报错，不再尝试。</p>
        </div>
        <Switch
          checked={providers.settings.allowCrossBorder}
          onchange={(v) => updateSettings({ allowCrossBorder: v })}
        />
      </div>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm">显示后台日志</p>
          <p class="text-[11px] text-muted-foreground">在主界面多出一个「日志」视图，能看到后台真正做了什么。</p>
        </div>
        <Switch
          checked={providers.settings.showLogs}
          onchange={(v) => updateSettings({ showLogs: v })}
        />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="space-y-1.5">
          <Label for="out-crs">输出坐标系</Label>
          <Select
            id="out-crs"
            options={crsOptions}
            value={providers.settings.outCrs}
            onchange={() => updateSettings({ outCrs: providers.settings!.outCrs })}
          />
        </div>
        <div class="space-y-1.5">
          <Label for="tile-src">底图样式</Label>
          <Select
            id="tile-src"
            options={tileOptions}
            value={providers.settings.tileSource}
            onchange={(e) => updateSettings({ tileSource: e.currentTarget.value })}
          />
        </div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="space-y-1.5">
          <Label for="long-walk">长步行提醒阈值（米）</Label>
          <!-- 落焦时才存：`oninput` 会在输入「80」的中途先存一个 8。 -->
          <Input
            id="long-walk"
            type="number"
            value={String(providers.settings.longWalkMeters)}
            onblur={(e) => updateSettings({ longWalkMeters: thresholdOf(e, DEFAULT_LABEL_THRESHOLDS.longWalkMeters) })}
          />
          <p class="text-[11px] text-muted-foreground">最长单段步行超过它就提醒，默认 500 米。</p>
        </div>
        <div class="space-y-1.5">
          <Label for="many-transfers">多换乘提醒阈值（次）</Label>
          <Input
            id="many-transfers"
            type="number"
            value={String(providers.settings.manyTransfers)}
            onblur={(e) => updateSettings({ manyTransfers: thresholdOf(e, DEFAULT_LABEL_THRESHOLDS.manyTransfers) })}
          />
          <p class="text-[11px] text-muted-foreground">换乘次数超过它才提醒，默认 2 次。</p>
        </div>
      </div>
    {/if}
  </section>
</Dialog>
