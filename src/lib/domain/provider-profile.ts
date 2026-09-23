import type { MapProvider } from "../ipc/types";

/**
 * 服务商档案的「显示逻辑」：几个字段组合出一种状态。
 * 判定放 domain 而不是组件里 —— 留在组件里会嵌三层 {#if}，而这里可以纯测。
 * （「值意味着什么」的判定在 Rust，这里只有「怎么显示」。）
 */

/** 档案点的状态：能用 / Key 还没验证 / 还没填 Key。 */
export type ProviderState = "ready" | "untested" | "empty";

/** 状态点用色（语义 token，组件里直接映射成 bg-success 等类名）。 */
export const PROVIDER_DOT: Record<ProviderState, string> = {
  ready: "bg-success",
  untested: "bg-warning",
  empty: "bg-pending",
};

/** 一个 provider 的当前状态。 */
export function providerState(p: MapProvider): ProviderState {
  if (!p.apiKeyMasked) return "empty";
  // 填了 Key 但还没点「测试」过 —— 能用与否要等一次真实请求才知道。
  return "untested";
}

/** 测试按钮禁用的原因；空串表示可以点。 */
export function testDisabledReason(p: MapProvider): string {
  if (!p.implemented) return "这家的接入还没实现";
  if (!p.apiKeyMasked) return "先填 API Key 再测试";
  return "";
}

/** 「优先级 N」的显示：用户没改过就不加标记。 */
export function priorityLabel(p: MapProvider): string {
  return p.priority !== undefined && p.priority !== p.defaultPriority
    ? `优先级 ${p.priority}（默认 ${p.defaultPriority}）`
    : `优先级 ${p.defaultPriority}`;
}

/** 排序：预置的在前，其余按 slug。列表顺序稳定才好找。 */
export function sortProviders(list: MapProvider[]): MapProvider[] {
  return [...list].sort((a, b) => {
    if (a.isPreset !== b.isPreset) return a.isPreset ? -1 : 1;
    return a.slug < b.slug ? -1 : 1;
  });
}

/** 一行摘要，列表项用：`CN · GCJ02 · 公交 · 地铁 · 步行`。 */
export function providerSummary(p: MapProvider): string {
  return [p.coverage, p.coordSys, p.capabilities].filter(Boolean).join(" · ");
}
