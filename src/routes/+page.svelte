<script lang="ts">
  /** 参赛主页：安心换乘与 15 分钟生活圈的决策工作台。 */
  import { planner } from "$lib/state/planner.svelte";
  import type { SortBy, TravelMode } from "$lib/ipc/types";
  import { DEFAULT_LABEL_THRESHOLDS, PROFILE_OPTIONS, decisionLabels, routeBurden, type LabelThresholds, type TravelProfile } from "$lib/domain/accessibility";
  import { formatDistance, formatDuration } from "$lib/domain/format";
  import { cn } from "$lib/utils/cn";
  import Button from "$lib/components/ui/button/Button.svelte";
  import Tabs from "$lib/components/ui/tabs/Tabs.svelte";
  import Select from "$lib/components/ui/select/Select.svelte";
  import Separator from "$lib/components/ui/separator/Separator.svelte";
  import EndpointField from "$lib/components/app/EndpointField.svelte";
  import RouteCard from "$lib/components/app/RouteCard.svelte";
  import MapCanvas from "$lib/components/app/MapCanvas.svelte";
  import SettingsDialog from "$lib/components/app/SettingsDialog.svelte";
  import LogPanel from "$lib/components/app/LogPanel.svelte";
  import ChatPanel from "$lib/components/ChatPanel.svelte";
  import { providers } from "$lib/state/providers.svelte";
  type View = "route" | "circle" | "chat" | "logs";
  let settingsOpen = $state(false);
  let view = $state<View>("route");
  /** 日志视图默认不占位，设置里打开才出现（见 SettingsDto.show_logs）。 */
  const showLogs = $derived(providers.settings?.showLogs ?? false);
  // 偏好只在设置弹窗打开时才会被加载，页面自己也得读一次，否则页签永远不出现。
  $effect(() => {
    if (!providers.settings) void providers.load();
  });
  // 关掉开关时正停在日志页：退回换乘，否则界面卡在一个已经不存在的页签上。
  $effect(() => {
    if (!showLogs && view === "logs") view = "route";
  });
  const modeOptions = [
    { value: "subway", label: "地铁" },
    { value: "bus", label: "公交" },
    { value: "walking", label: "步行" },
  ];
  const sortOptions = [
    { value: "time", label: "耗时最短" },
    { value: "distance", label: "距离最短" },
  ];
  const circlePackages = [
    { value: "care" as const, label: "就医安心", hint: "医院 · 药店 · 卫生间" },
    { value: "family" as const, label: "亲子安心", hint: "公园 · 便利店 · 地铁站" },
    { value: "emergency" as const, label: "应急安心", hint: "医院 · 药店 · 警务室" },
  ];
  const recommendedRoute = $derived(planner.routes[0] ?? null);
  const recommendedBurden = $derived(recommendedRoute ? routeBurden(recommendedRoute) : null);
  // 决策标签对整组候选算一次。放在这里而不是卡片里，是因为「在候选中最低」
  // 这类相对判断需要看到全部候选。
  // 阈值来自设置（§4.2:不许写死在 UI）。偏好还没加载完时用同一组默认值，
  // 与后端 `DEFAULT_LONG_WALK_METERS` / `DEFAULT_MANY_TRANSFERS` 对应。
  const thresholds = $derived<LabelThresholds>({
    longWalkMeters: providers.settings?.longWalkMeters ?? DEFAULT_LABEL_THRESHOLDS.longWalkMeters,
    manyTransfers: providers.settings?.manyTransfers ?? DEFAULT_LABEL_THRESHOLDS.manyTransfers,
  });
  const routeLabels = $derived(decisionLabels(planner.routes, thresholds));
  function setMode(v: string) { planner.setMode(v as TravelMode); }
  function setSort(v: string) { planner.sort = v as SortBy; }
  function setProfile(v: string) { planner.setProfile(v as TravelProfile); }
  function selectView(next: View): void { view = next; }
  function packageLabel(value: string): string {
    return circlePackages.find((item) => item.value === value)?.label ?? "安心生活圈";
  }
</script>


<svelte:head>
  <title>AutoBee · 安心出行决策</title>
</svelte:head>

<div class="flex h-dvh min-h-0 flex-col overflow-hidden bg-background text-foreground">
  <header class="flex min-h-14 shrink-0 items-center gap-3 border-b bg-card px-4">
    <img src="/favicon.png" alt="AutoBee" class="size-6" />
    <div class="min-w-0">
      <h1 class="truncate text-sm font-semibold">AutoBee 安心出行</h1>
      <p class="truncate text-[11px] text-muted-foreground">少走一点，安心一点</p>
    </div>
    <nav class="ml-4 flex items-center gap-1 rounded-lg bg-muted p-1" aria-label="功能模式">
      <button type="button" class={cn("rounded-md px-3 py-1.5 text-xs font-medium transition-colors", view === "route" ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")} aria-pressed={view === "route"} onclick={() => selectView("route")}>安心换乘</button>
      <button type="button" class={cn("rounded-md px-3 py-1.5 text-xs font-medium transition-colors", view === "circle" ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")} aria-pressed={view === "circle"} onclick={() => selectView("circle")}>15 分钟生活圈</button>
      <button type="button" class={cn("rounded-md px-3 py-1.5 text-xs font-medium transition-colors", view === "chat" ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")} aria-pressed={view === "chat"} onclick={() => selectView("chat")}>问一问</button>
      {#if showLogs}
        <button type="button" class={cn("rounded-md px-3 py-1.5 text-xs font-medium transition-colors", view === "logs" ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")} aria-pressed={view === "logs"} onclick={() => selectView("logs")}>日志</button>
      {/if}
    </nav>
    <div class="ml-auto flex items-center gap-2">
      <span class="hidden text-[11px] text-muted-foreground sm:inline">百度地图驱动</span>
      <Button variant="ghost" size="sm" onclick={() => (settingsOpen = true)}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 1-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 1-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
        设置
      </Button>
    </div>
  </header>

  {#if view === "chat"}
    <!-- 对话页复用 planner 的路线状态；切换到安心换乘后继续编辑而不重算。 -->
    <ChatPanel onopenroute={() => selectView("route")} />
  {:else if view === "logs"}
    <!-- 日志占满：每行都长，挤在 400px 侧栏里全是折行。 -->
    <LogPanel />
  {:else}
  <div class="flex min-h-0 flex-1 flex-col lg:flex-row">
    <aside class="flex w-full shrink-0 flex-col gap-4 overflow-y-auto border-b p-4 lg:w-[400px] lg:border-b-0 lg:border-r">
      {#if view === "route"}
        <section class="flex flex-col gap-3" aria-labelledby="route-heading">
          <div>
            <p class="text-[11px] font-medium uppercase tracking-[0.18em] text-primary">Route / 01</p>
            <h2 id="route-heading" class="mt-1 text-lg font-semibold">先说你要怎么出行</h2>
            <p class="mt-1 text-xs text-muted-foreground">AutoBee 会把步行、换乘和耗时放在一起比较。</p>
          </div>
          <div class="flex flex-col gap-2">
            <label class="text-xs font-medium" for="from-place">从哪里出发</label>
            <EndpointField field="from" placeholder="地名 / 地址 / 纬度,经度" inputId="from-place" />
            <label class="text-xs font-medium" for="to-place">要去哪里</label>
            <EndpointField field="to" placeholder="地名 / 地址 / 纬度,经度" inputId="to-place" />
          </div>
          <div class="flex items-center justify-between gap-2">
            <Tabs options={modeOptions} value={planner.mode} onchange={setMode} />
            <Button variant="ghost" size="sm" title="交换起终点" ariaLabel="交换起终点" onclick={() => planner.swap()}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 16V4M7 4L3 8M7 4l4 4M17 8v12m0 0 4-4m-4 4-4-4" /></svg>
            </Button>
          </div>
          <div class="flex items-center gap-2">
            <Select class="flex-1" options={sortOptions} value={planner.sort} onchange={(e) => setSort(e.currentTarget.value)} />
            <Button class="flex-1" disabled={planner.loading} onclick={() => void planner.run()}>{planner.loading ? "规划中…" : "规划路线"}</Button>
          </div>
          <label class="text-xs font-medium" for="profile">出行画像</label>
          <Select id="profile" options={PROFILE_OPTIONS} value={planner.profile} onchange={(e) => setProfile(e.currentTarget.value)} />
          <!-- §6.3:把判断依据写在用户看得见的地方。沉默会被读成「设施没问题」。 -->
          <p class="text-[11px] text-muted-foreground">依据仅为步行距离与换乘次数；电梯、坡道、台阶等设施信息尚未接入。</p>
        </section>

        {#if recommendedRoute && recommendedBurden}
          <section class="rounded-lg border border-primary/30 bg-primary/5 p-4" aria-label="推荐结论">
            <p class="text-[11px] font-medium uppercase tracking-[0.18em] text-primary">Recommended</p>
            <h3 class="mt-1 text-sm font-semibold">为「{PROFILE_OPTIONS.find((item) => item.value === planner.profile)?.label}」推荐方案 1</h3>
            <div class="mt-3 flex items-end gap-3"><strong class="text-2xl font-semibold">{formatDuration(recommendedRoute.duration)}</strong><span class="pb-0.5 text-xs text-muted-foreground">步行 {formatDistance(recommendedBurden.walkDistance)} · 换乘 {recommendedBurden.transfers} 次</span></div>
            <p class="mt-2 text-xs text-muted-foreground">{recommendedBurden.longestWalk > thresholds.longWalkMeters ? `最长步行 ${Math.round(recommendedBurden.longestWalk)} 米，请预留体力。` : "在当前画像下，步行与换乘负担更低。"}</p>
          </section>
        {/if}
        {#if planner.error}<p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">{planner.error}</p>{/if}
        {#if planner.degradedNote}<p class="rounded-md bg-warning/10 px-3 py-2 text-xs text-warning">{planner.degradedNote}</p>{/if}
        {#if planner.plan}
          <Separator />
          <section class="flex flex-col gap-2" aria-labelledby="routes-heading">
            <div class="flex items-baseline justify-between"><h2 id="routes-heading" class="text-xs font-semibold text-muted-foreground">{planner.routes.length} 个候选方案</h2><span class="text-[11px] text-muted-foreground">百度 · {planner.plan.provider}</span></div>
            {#each planner.routes as route (route.id)}<RouteCard {route} {thresholds} profile={planner.profile} selected={planner.selectedRouteId === route.id} labels={routeLabels[route.id] ?? []} onselect={() => planner.selectRoute(route.id)} />{/each}
          </section>
        {:else if !planner.loading && !planner.error}
          <div class="rounded-lg border border-dashed p-4 text-xs text-muted-foreground"><p>输入起点和终点，开始比较安心方案。</p><p class="mt-1">也支持直接输入「纬度,经度」坐标。</p></div>
        {/if}
      {:else}
        <section class="flex flex-col gap-4" aria-labelledby="circle-heading">
          <div><p class="text-[11px] font-medium uppercase tracking-[0.18em] text-primary">Circle / 02</p><h2 id="circle-heading" class="mt-1 text-lg font-semibold">15 分钟内，附近有什么</h2><p class="mt-1 text-xs text-muted-foreground">选一个地点，逐个用百度步行路线验证真实可达服务。</p></div>

          <div class="flex flex-col gap-1.5">
            <label for="circle-center" class="text-xs font-medium">生活圈中心</label>
            <EndpointField field="circle" inputId="circle-center" placeholder="搜索地点，或沿用换乘起点" />
            <p class="text-[11px] text-muted-foreground">
              {#if planner.circlePlace}已选：{planner.circlePlace.title}
              {:else if planner.circleOrigin}沿用换乘起点：{planner.circleOrigin.label}
              {:else if planner.circleText.trim()}请从候选里点选一个地点以锁定坐标。
              {:else}留空则沿用安心换乘的起点。{/if}
            </p>
          </div>

          <fieldset class="flex flex-col gap-2">
            <legend class="mb-1.5 text-xs font-medium">需要哪些服务（可多选）</legend>
            {#each circlePackages as item (item.value)}
              {@const checked = planner.selectedPackages.includes(item.value)}
              <label class={cn("flex cursor-pointer items-start gap-2.5 rounded-lg border p-3 transition-colors", checked ? "border-primary bg-primary/5" : "hover:bg-accent")}>
                <input type="checkbox" class="mt-0.5 size-4 accent-primary" {checked} disabled={planner.lifeCircleLoading} onchange={() => planner.togglePackage(item.value)} />
                <span class="min-w-0"><span class="block text-sm font-medium">{item.label}</span><span class="mt-0.5 block text-[11px] text-muted-foreground">{item.hint}</span></span>
              </label>
            {/each}
          </fieldset>

          <Button class="w-full" disabled={planner.lifeCircleLoading} onclick={() => void planner.loadLifeCircle()}>
            {planner.lifeCircleLoading ? "正在验证步行可达…" : `查询 ${planner.selectedPackages.length} 个服务包`}
          </Button>

          {#if planner.lifeCircleError}
            <p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive" role="alert">{planner.lifeCircleError}</p>
          {/if}

          {#if planner.circleResults.length > 0}
            <Separator />
            <section class="flex flex-col gap-3" aria-label="生活圈结果">
              <div class="flex items-end justify-between">
                <div><p class="text-2xl font-semibold">{planner.reachablePois.length}</p><p class="text-xs text-muted-foreground">个 15 分钟步行可达服务点</p></div>
                <span class="text-[11px] text-muted-foreground">共 {planner.circleResults.length} 个服务包</span>
              </div>
              {#each planner.circleResults as result (result.package)}
                <article class="rounded-lg border">
                  <header class="flex items-baseline justify-between gap-2 border-b px-3 py-2">
                    <h3 class="text-xs font-semibold">{packageLabel(result.package)}</h3>
                    <span class="shrink-0 text-[11px] text-muted-foreground">{result.ok ? `${result.circle.pois.length} 个可达` : "未取到结果"}</span>
                  </header>
                  {#if !result.ok}
                    <p class="px-3 py-2 text-[11px] text-destructive">{result.message}</p>
                  {:else if result.circle.pois.length === 0}
                    <p class="px-3 py-2 text-[11px] text-muted-foreground">这一包在 15 分钟步行范围内没有可达服务点。</p>
                  {:else}
                    <ul class="flex flex-col divide-y text-xs">
                      {#each result.circle.pois.slice(0, 5) as poi (poi.name + poi.category)}
                        <li class="flex items-center justify-between gap-2 px-3 py-2">
                          <span class="min-w-0"><span class="block truncate font-medium">{poi.name}</span><span class="block truncate text-[11px] text-muted-foreground">{poi.category} · {formatDistance(poi.walkDistance)}</span></span>
                          <span class="shrink-0 text-muted-foreground">{Math.ceil(poi.walkDurationSec / 60)} 分钟</span>
                        </li>
                      {/each}
                    </ul>
                    {#if result.circle.pois.length > 5}
                      <p class="border-t px-3 py-1.5 text-[11px] text-muted-foreground">另有 {result.circle.pois.length - 5} 个已在地图上标出。</p>
                    {/if}
                  {/if}
                </article>
              {/each}
            </section>
          {:else if !planner.lifeCircleLoading && !planner.lifeCircleError}
            <div class="rounded-lg border border-dashed p-4 text-xs text-muted-foreground"><p>选好地点与服务包后开始查询。</p><p class="mt-1">没有百度 Web 服务 AK 时不会显示伪造结果。</p></div>
          {/if}
        </section>
      {/if}
    </aside>
    <main class="relative min-h-[420px] min-w-0 flex-1 p-2 lg:min-h-0">
      <div class="absolute left-5 top-5 z-10 rounded-md border bg-card/95 px-3 py-2 text-[11px] shadow-sm backdrop-blur"><span class="font-medium">百度地图</span><span class="ml-2 text-muted-foreground">{view === "route" ? "推荐路线" : "真实步行可达服务"}</span></div>
      <MapCanvas class="relative h-full w-full" />
    </main>
  </div>
  {/if}
</div>

<SettingsDialog bind:open={settingsOpen} />
