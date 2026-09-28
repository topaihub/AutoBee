/**
 * 用户侧 Skill 状态。组件只通过这里调用 IPC，离线包的二进制读取也不散落进设置页。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { Skill } from "../ipc/types";
import { errorMessage } from "./planner.svelte";

export class SkillsState {
  list = $state<Skill[]>([]);
  loading = $state(false);
  importing = $state(false);
  saving = $state(false);
  error = $state("");
  saved = $state(false);

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  async load(): Promise<void> {
    this.loading = true;
    this.error = "";
    try {
      this.list = await this.client.listSkills();
    } catch (error) {
      this.error = errorMessage(error);
    } finally {
      this.loading = false;
    }
  }

  /** 导入本地 ZIP 的原始字节；后端只解析 manifest 与 SKILL.md，不会解压执行。 */
  async importPackage(file: File): Promise<void> {
    this.importing = true;
    this.error = "";
    this.saved = false;
    try {
      this.list = await this.client.importSkillPackage([...new Uint8Array(await file.arrayBuffer())]);
      this.saved = true;
    } catch (error) {
      this.error = errorMessage(error);
    } finally {
      this.importing = false;
    }
  }

  async setEnabled(id: string, enabled: boolean): Promise<void> {
    await this.#save(() => this.client.setSkillEnabled(id, enabled));
  }

  async setMcpServers(id: string, serverIds: string[]): Promise<void> {
    await this.#save(() => this.client.setSkillMcpServers(id, serverIds));
  }

  async remove(id: string): Promise<void> {
    await this.#save(() => this.client.deleteSkill(id));
  }

  async #save(request: () => Promise<Skill[]>): Promise<void> {
    this.saving = true;
    this.error = "";
    this.saved = false;
    try {
      this.list = await request();
      this.saved = true;
    } catch (error) {
      this.error = errorMessage(error);
    } finally {
      this.saving = false;
    }
  }
}


export const skills = new SkillsState();
