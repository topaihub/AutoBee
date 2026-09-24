/**
 * 后端日志面板状态。后端把日志写文件、没有事件推送（见 `src-tauri/src/logs.rs`），
 * 所以面板打开时每两秒拉一次尾部；关闭时停表，否则无人查看时仍在持续读文件加 IPC。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { LogEntry } from "../ipc/types";
import { errorMessage } from "./planner.svelte";

const LIMIT = 300;
const INTERVAL_MS = 2000;

export class LogsState {
  entries = $state<LogEntry[]>([]);
  loading = $state(false);
  error = $state("");
  onlyProblems = $state(false);

  #timer: ReturnType<typeof setInterval> | undefined;

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

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
