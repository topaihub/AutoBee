/**
 * MCP server 设置状态。
 *
 * 后端每次写操作都返回完整列表，前端不乐观拼接；URL 安全校验必须由后端决定，
 * 这里只负责加载、保存、删除和当场读取 tools/list 的结果。
 */
import { tauriPlannerClient, type PlannerClient } from "../ipc/client";
import type { McpServer, McpServerInput, McpToolInfo } from "../ipc/types";
import { errorMessage } from "./planner.svelte";

export class McpState {
  list = $state<McpServer[]>([]);
  loading = $state(false);
  saving = $state(false);
  error = $state("");
  saved = $state(false);
  fetching = $state<Record<string, boolean>>({});
  tools = $state<Record<string, McpToolInfo[]>>({});
  toolErrors = $state<Record<string, string>>({});

  constructor(private readonly client: PlannerClient = tauriPlannerClient) {}

  async load(): Promise<void> {
    this.loading = true;
    this.error = "";
    try {
      this.list = await this.client.listMcpServers();
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loading = false;
    }
  }

  async save(input: McpServerInput): Promise<void> {
    this.saving = true;
    this.error = "";
    try {
      this.list = await this.client.saveMcpServer(input);
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.saving = false;
    }
  }

  async remove(id: string): Promise<void> {
    this.error = "";
    try {
      this.list = await this.client.deleteMcpServer(id);
      const tools = { ...this.tools };
      delete tools[id];
      this.tools = tools;
      const toolErrors = { ...this.toolErrors };
      delete toolErrors[id];
      this.toolErrors = toolErrors;
      this.flashSaved();
    } catch (e) {
      this.error = errorMessage(e);
    }
  }

  async loadTools(id: string): Promise<void> {
    this.fetching = { ...this.fetching, [id]: true };
    this.toolErrors = { ...this.toolErrors, [id]: "" };
    try {
      this.tools = { ...this.tools, [id]: await this.client.listMcpTools(id) };
    } catch (e) {
      this.toolErrors = { ...this.toolErrors, [id]: errorMessage(e) };
    } finally {
      this.fetching = { ...this.fetching, [id]: false };
    }
  }

  flashSaved(): void {
    this.saved = true;
    setTimeout(() => (this.saved = false), 2500);
  }
}

export const mcp = new McpState();
