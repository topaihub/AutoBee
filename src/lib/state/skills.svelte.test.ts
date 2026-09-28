import { describe, expect, it, vi } from "vitest";
import type { PlannerClient } from "../ipc/client";
import { SkillsState } from "./skills.svelte";

describe("SkillsState", () => {
  it("导入的字节经 IPC 交给后端且失败不覆盖旧列表", async () => {
    const existing = [{ id: "old", packageId: "old", name: "旧", version: "1", description: "旧", instructions: "旧", enabled: false, mcpServerIds: [] }];
    const imported = [{ id: "new", packageId: "new", name: "新", version: "1", description: "新", instructions: "新", enabled: false, mcpServerIds: [] }];
    const importSkillPackage = vi.fn(async (bytes: number[]) => {
      expect(bytes).toEqual([1, 2, 3]);
      return imported;
    });
    const client = { listSkills: vi.fn(async () => existing), importSkillPackage } as unknown as PlannerClient;
    const state = new SkillsState(client);
    await state.load();
    await state.importPackage(new File([new Uint8Array([1, 2, 3])], "skill.zip"));
    expect(state.list).toEqual(imported);

    importSkillPackage.mockRejectedValueOnce(new Error("损坏 ZIP"));
    await state.importPackage(new File([new Uint8Array([9])], "bad.zip"));
    expect(state.list).toEqual(imported);
    expect(state.error).toContain("损坏 ZIP");
  });

  it("启停和绑定走独立 IPC 契约", async () => {
    const skill = { id: "s", packageId: "s", name: "技能", version: "1", description: "", instructions: "", enabled: false, mcpServerIds: [] };
    const setSkillEnabled = vi.fn(async () => [{ ...skill, enabled: true }]);
    const setSkillMcpServers = vi.fn(async () => [{ ...skill, mcpServerIds: ["m1"] }]);
    const client = { setSkillEnabled, setSkillMcpServers } as unknown as PlannerClient;
    const state = new SkillsState(client);
    await state.setEnabled("s", true);
    expect(setSkillEnabled).toHaveBeenCalledWith("s", true);
    await state.setMcpServers("s", ["m1"]);
    expect(setSkillMcpServers).toHaveBeenCalledWith("s", ["m1"]);
  });
});
