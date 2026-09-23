<script module lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
</script>

<script lang="ts">
  /**
   * 设置弹窗：服务商 Key 配置（对标 ai-installer 的模型服务商配置页）+ 全局偏好。
   * 数据全走 ProvidersState，这里只做表单绑定与按钮回调。
   */
  import { providers } from "$lib/state/providers.svelte";
  import type { Crs, MapProviderInput, ProviderId } from "$lib/ipc/types";
  import Dialog from "$lib/components/ui/dialog/Dialog.svelte";
  import Button from "$lib/components/ui/button/Button.svelte";
  import Input from "$lib/components/ui/input/Input.svelte";
  import Label from "$lib/components/ui/label/Label.svelte";
  import Select from "$lib/components/ui/select/Select.svelte";
  import Separator from "$lib/components/ui/separator/Separator.svelte";
  import Switch from "$lib/components/ui/switch/Switch.svelte";
  import Badge from "$lib/components/ui/badge/Badge.svelte";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  /** 正在输入的明文 Key，按 slug 记；保存后清空。 */
  let keyDrafts = $state<Record<string, string>>({});
  /** 每条档案本地编辑中的启用/优先级（点「保存」才落库）。 */
  let drafts = $state<Record<string, { enabled: boolean; priority: string }>>({});

  $effect(() => {
    if (open) {
      void providers.load();
      keyDrafts = {};
    }
  });

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
    const input: MapProviderInput = {
      slug: p.slug as ProviderId,
      name: p.name,
      enabled: d.enabled,
      priority: d.priority === "" ? null : Number(d.priority),
      apiKey: key ? key : null,
    };
    await providers.save(input);
    delete keyDrafts[p.slug];
  }

  async function remove(slug: string) {
    await providers.remove(slug);
    delete drafts[slug];
    delete keyDrafts[slug];
  }

  function updateSettings(patch: Partial<NonNullable<typeof providers.settings>>) {
    if (!providers.settings) return;
    void providers.saveSettings({ ...providers.settings, ...patch });
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
              <Input
                class="h-8 flex-1 text-xs"
                type="password"
                placeholder={p.apiKeyMasked || (p.slug === "amap" ? "填写 Web服务 API Key" : "填写 API Key")}
                value={keyDrafts[p.slug] ?? ""}
                oninput={(e) => (keyDrafts[p.slug] = e.currentTarget.value)}
              />
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
            {#if p.slug === "baidu"}
              <p class="mt-1.5 text-[11px] text-muted-foreground">填写百度 Web 服务 AK 后可用于 15 分钟安心生活圈；路线适配仍在开发中。</p>
            {/if}
          </div>
        {/each}
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
    {/if}
  </section>
</Dialog>
