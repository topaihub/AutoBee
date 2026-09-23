import { afterEach, describe, expect, it, vi } from "vitest";
import type { PlannerClient } from "../ipc/client";
import type { LogEntry } from "../ipc/types";
import { LogsState } from "./logs.svelte";

function entry(seq: number, level: string, message: string): LogEntry {
  return { seq, time: "2026-09-23T02:37:59.100951Z", level, target: "t", message };
}

function fakeClient(entries: LogEntry[]): { client: PlannerClient; readLogs: ReturnType<typeof vi.fn> } {
  const readLogs = vi.fn(async () => entries);
  return { client: { readLogs } as unknown as PlannerClient, readLogs };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("LogsState", () => {
  it("最新的排在最前面：排查时看的是刚发生的事", async () => {
    const { client } = fakeClient([entry(0, "INFO", "旧"), entry(1, "INFO", "新")]);
    const state = new LogsState(client);

    await state.refresh();

    expect(state.visible.map((e) => e.message)).toEqual(["新", "旧"]);
  });

  it("只看问题时留下 WARN 与 ERROR", async () => {
    const { client } = fakeClient([
      entry(0, "DEBUG", "噪音"),
      entry(1, "WARN", "瓦片获取失败"),
      entry(2, "INFO", "开始规划路线"),
      entry(3, "ERROR", "规划失败"),
    ]);
    const state = new LogsState(client);
    await state.refresh();

    state.onlyProblems = true;

    expect(state.visible.map((e) => e.message)).toEqual(["规划失败", "瓦片获取失败"]);
  });

  it("读失败时留住已有内容并给出提示", async () => {
    // 面板正在显示的那几行是用户唯一的线索，不能因为一次读失败被清空。
    const state = new LogsState({
      readLogs: vi.fn(async () => {
        throw new Error("读取日志失败");
      }),
    } as unknown as PlannerClient);
    state.entries = [entry(0, "INFO", "之前读到的")];

    await state.refresh();

    expect(state.entries.map((e) => e.message)).toEqual(["之前读到的"]);
    expect(state.error).toContain("读取日志");
  });

  it("重复 start 不会叠加定时器，stop 之后不再拉取", async () => {
    vi.useFakeTimers();
    const { client, readLogs } = fakeClient([]);
    const state = new LogsState(client);

    state.start();
    state.start();
    await vi.advanceTimersByTimeAsync(2000);
    expect(readLogs).toHaveBeenCalledTimes(2);

    state.stop();
    await vi.advanceTimersByTimeAsync(10000);
    expect(readLogs).toHaveBeenCalledTimes(2);
  });
});
