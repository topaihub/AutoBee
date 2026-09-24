import type { MapProvider } from "../ipc/types";

// 档案的「怎么显示」判定。放 domain 不放组件：组件里要嵌三层 {#if}，这里可以纯测。

export type ProviderState = "ready" | "untested" | "empty" | "partial";

export const PROVIDER_DOT: Record<ProviderState, string> = {
  ready: "bg-success",
  untested: "bg-warning",
  empty: "bg-pending",
  partial: "bg-warning",
};

// 百度的服务端 AK 与浏览器端 AK 在控制台是两种应用类型，互相不能用；
// 只填一把的症状（地图空白，或搜索失败）看不出是少填了一格。
function needsTwoKeys(p: MapProvider): boolean {
  return p.slug === "baidu";
}

export function providerState(p: MapProvider): ProviderState {
  if (needsTwoKeys(p)) {
    if (!p.apiKeyMasked && !p.browserKeyMasked) return "empty";
    if (!p.apiKeyMasked || !p.browserKeyMasked) return "partial";
    return "untested";
  }
  if (!p.apiKeyMasked) return "empty";
  // 填了 Key 也只是 untested：能用与否要等一次真实请求。
  return "untested";
}

export function partialKeyHint(p: MapProvider): string {
  if (providerState(p) !== "partial") return "";
  return p.apiKeyMasked
    ? "还差浏览器端 AK —— 搜索和路线能用，但地图不会渲染"
    : "还差服务端 AK —— 地图能渲染，但搜索和路线规划会失败";
}

export function testDisabledReason(p: MapProvider): string {
  if (!p.implemented) return "这家的接入还没实现";
  if (!p.apiKeyMasked) return "先填 API Key 再测试";
  return "";
}

export function priorityLabel(p: MapProvider): string {
  return p.priority !== undefined && p.priority !== p.defaultPriority
    ? `优先级 ${p.priority}（默认 ${p.defaultPriority}）`
    : `优先级 ${p.defaultPriority}`;
}

export function sortProviders(list: MapProvider[]): MapProvider[] {
  return [...list].sort((a, b) => {
    if (a.isPreset !== b.isPreset) return a.isPreset ? -1 : 1;
    return a.slug < b.slug ? -1 : 1;
  });
}

export function providerSummary(p: MapProvider): string {
  return [p.coverage, p.coordSys, p.capabilities].filter(Boolean).join(" · ");
}
