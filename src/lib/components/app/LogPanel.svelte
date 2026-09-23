<script lang="ts">
  /**
   * 后台日志面板。
   *
   * 桌面程序里后端的 stdout 没人看得到，出问题时「界面弹一句失败」是唯一线索。
   * 这里把后端真正写下的那几行摊开：哪一家服务商失败了、失败在哪一步、是不是
   * 根本没填 Key。
   *
   * 等宽字体 + 每行一条，不折行也不做卡片 —— 日志是拿来扫的，扫的是左边那列
   * 时间和级别，排版越规整越快。
   */
  import { logs } from "$lib/state/logs.svelte";
  import { cn } from "$lib/utils/cn";
  import Button from "$lib/components/ui/button/Button.svelte";

  /** 挂载时开始轮询，卸载时停掉。面板不在时后台不该还在每两秒读文件。 */
  $effect(() => {
    logs.start();
    return () => logs.stop();
  });

  /** 级别配色：只让 WARN/ERROR 抢眼，其余压成弱色，否则满屏都在喊。 */
  function levelClass(level: string): string {
    if (level === "ERROR") return "text-destructive";
    if (level === "WARN") return "text-warning";
    if (level === "DEBUG" || level === "TRACE") return "text-muted-foreground/60";
    return "text-muted-foreground";
  }

  /** 只留时分秒：日期全是今天，年月日占掉一半宽度换不来信息。 */
  function clock(time: string): string {
    return time.slice(11, 19);
  }

  /** `route_planner_lib::credentials::store` 这种长模块名只留最后一节。 */
  function shortTarget(target: string): string {
    const parts = target.split("::");
    return parts[parts.length - 1] ?? target;
  }
</script>

<section class="flex min-h-0 flex-1 flex-col" aria-labelledby="log-heading">
  <div class="flex shrink-0 items-center gap-2 border-b px-4 py-2">
    <h2 id="log-heading" class="text-xs font-semibold uppercase tracking-[0.18em] text-primary">
      后台日志
    </h2>
    <span class="text-[11px] text-muted-foreground">
      {logs.entries.length} 条 · 每 2 秒刷新
    </span>
    <label class="ml-auto flex cursor-pointer items-center gap-1.5 text-[11px] text-muted-foreground">
      <input type="checkbox" class="size-3.5 accent-primary" bind:checked={logs.onlyProblems} />
      只看问题
    </label>
    <Button variant="ghost" size="sm" onclick={() => logs.refresh()} disabled={logs.loading}>
      {logs.loading ? "读取中" : "刷新"}
    </Button>
  </div>

  {#if logs.error}
    <p class="border-b bg-destructive/10 px-4 py-2 text-xs text-destructive">{logs.error}</p>
  {/if}

  {#if logs.visible.length === 0}
    <p class="p-4 text-xs text-muted-foreground">
      {logs.onlyProblems ? "没有警告或错误 —— 这是好消息。" : "还没有日志。用一次规划或生活圈就会有了。"}
    </p>
  {:else}
    <ul class="min-h-0 flex-1 overflow-auto p-2 font-mono text-[11px] leading-relaxed">
      {#each logs.visible as entry (entry.seq)}
        <li class="flex gap-2 rounded px-2 py-0.5 hover:bg-muted/60">
          <span class="shrink-0 text-muted-foreground/70">{clock(entry.time)}</span>
          <span class={cn("w-10 shrink-0 font-semibold", levelClass(entry.level))}>{entry.level}</span>
          <span class="shrink-0 text-muted-foreground/70">{shortTarget(entry.target)}</span>
          <span class="min-w-0 break-all">{entry.message}</span>
        </li>
      {/each}
    </ul>
  {/if}
</section>
