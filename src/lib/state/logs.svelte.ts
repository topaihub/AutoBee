/**
 * 后端日志面板的状态。
 *
 * # 为什么是轮询
 *
 * 后端把日志写文件，没有事件推送（见 `src-tauri/src/logs.rs` 的模块文档）。
 * 面板打开时每两秒拉一次尾部就够了 —— 这是给人看的，不是给机器消费的，
 * 人眼分不出 200ms 和 2s 的差别。
 *
 * # 为什么关掉面板要停掉轮询
 *
 * 不停的话，用户在别的页签上算路时后台还在每两秒读一次文件加一次 IPC。
 * 那笔开销买不到任何东西 —— 没人在看。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { LogEntry } from "../ipc/types";
import { errorMessage } from "./planner.svelte";

/** 一次拉多少条。一屏装不下这么多，多出来的是往上翻的余量。 */
const LIMIT = 300;
const INTERVAL_MS = 2000;

export class LogsState {
  entries = $state<LogEntry[]>([]);
  loading = $state(false);
  error = $state("");
  /** 只显示 WARN 以上。排查时先看这个，看不出来再放开。 */
  onlyProblems = $state(false);

  #timer: ReturnType<typeof setInterval> | undefined;

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  /** 过滤后的视图。倒序：最新的在最上面，不用滚到底。 */
  get visible(): LogEntry[] {
    const list = this.onlyProblems
      ? this.entries.filter((e) => e.level === "ERROR" || e.level === "WARN")
      : this.entries;
    return [...list].reverse();
  }

  async refresh(): Promise<void> {
    this.loading = true;
    try {
      this.entries = await this.client.readLogs(LIMIT);
      this.error = "";
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  /** 开始轮询。重复调用不会叠加定时器。 */
  start(): void {
    if (this.#timer !== undefined) return;
    void this.refresh();
    this.#timer = setInterval(() => void this.refresh(), INTERVAL_MS);
  }

  stop(): void {
    clearInterval(this.#timer);
    this.#timer = undefined;
  }
}

export const logs = new LogsState();
