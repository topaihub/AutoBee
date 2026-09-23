<script lang="ts">
  /** 单个方案的摘要卡片 + 展开后的分段明细。 */
  import type { Route } from "$lib/ipc/types";
  import { burdenReason, routeBurden, type TravelProfile } from "$lib/domain/accessibility";
  import { formatDistance, formatDuration, formatPrice } from "$lib/domain/format";
  import { hasBus, hasSubway, lineNames, totalWalkDistance } from "$lib/domain/route";
  import Badge from "$lib/components/ui/badge/Badge.svelte";

  let {
    route,
    profile,
    selected,
    onselect,
  }: {
    route: Route;
    profile: TravelProfile;
    selected: boolean;
    onselect: () => void;
  } = $props();

  let expanded = $derived(selected);

  const burden = $derived(routeBurden(route));
</script>

<button
  type="button"
  class="w-full rounded-lg border bg-card p-3 text-left transition-colors hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 {selected
    ? 'border-primary ring-1 ring-primary/40'
    : ''}"
  onclick={onselect}
>
  <!-- 摘要行 -->
  <div class="flex items-baseline justify-between gap-2">
    <span class="text-base font-semibold">{formatDuration(route.duration)}</span>
    <span class="text-xs text-muted-foreground">
      步行 {formatDistance(totalWalkDistance(route))} · {formatPrice(route.price)}
    </span>
  </div>
  <p class="mt-1 text-[11px] text-muted-foreground">{burdenReason(route, profile)}</p>

  {#if burden.longestWalk >= 500 || (profile !== "standard" && burden.transfers > 0)}
    <p class="mt-1 text-[11px] text-warning">设施无障碍信息待确认</p>
  {/if}

  <!-- 线路徽标链 -->
  <div class="mt-2 flex flex-wrap items-center gap-1">
    {#if hasSubway(route)}
      <Badge variant="default">地铁</Badge>
    {/if}
    {#if hasBus(route)}
      <Badge variant="secondary">公交</Badge>
    {/if}
    {#each lineNames(route) as name, i (`${route.id}-${i}-${name}`)}
      {#if i > 0}
        <span class="text-[10px] text-muted-foreground">›</span>
      {/if}
      <span class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px]">
        <span class="size-2 rounded-full bg-[var(--lc)]" style="--lc: {route.segments.filter((s) => s.line?.title === name)[0]?.line?.color ?? '#999'}"></span>
        {name}
      </span>
    {/each}
  </div>

  <!-- 分段明细：选中即展开 -->
  {#if expanded}
    <ol class="mt-3 space-y-2 border-t pt-3">
      {#each route.segments as segment, i (i)}
        <li class="flex gap-2 text-xs">
          <span
            class="mt-1 size-2 shrink-0 rounded-full bg-[var(--lc)]"
            style="--lc: {segment.line?.color ?? '#9aa0a6'}"
          ></span>
          <div class="min-w-0">
            {#if segment.kind === "walk"}
              <p class="font-medium">步行 {formatDistance(segment.distance)}</p>
              {#if segment.instruction?.length}
                <ol class="mt-1 divide-y border-t">
                  {#each segment.instruction as instruction, index (`${index}-${instruction}`)}
                    <li class="py-2 leading-5 text-muted-foreground">{instruction}</li>
                  {/each}
                </ol>
              {/if}
            {:else if segment.line}
              <p class="font-medium">
                {segment.line.getOn.title} 上车 → {segment.line.destination ?? segment.line.getOff.title}
                <span class="text-muted-foreground">
                  （{segment.line.stationCount} 站 · {formatDuration(segment.duration)}）
                </span>
              </p>
              <p class="mt-0.5 text-muted-foreground">
                {segment.line.title}
                {#if segment.line.getOff.exit}· {segment.line.getOff.exit} 出口{/if}
                {#if segment.line.firstTime}· 首班 {segment.line.firstTime}{/if}
                {#if segment.line.lastTime}· 末班 {segment.line.lastTime}{/if}
              </p>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</button>
