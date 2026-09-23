/**
 * 展示格式化。只回答「这个值怎么显示」，不回答「这个值意味着什么」——
 * 后者（字段语义）归 Rust，这条分界线是 ai-installer 踩过两实现漂移后定的。
 */

/** 距离：米 → 人读的字符串。1 公里以内用米，否则一位小数的公里。 */
export function formatDistance(meters: number): string {
  if (meters < 1000) return `${Math.round(meters)} 米`;
  return `${(meters / 1000).toFixed(1)} 公里`;
}

/** 时长：分钟 → 「45 分钟」/「1 小时 5 分」。 */
export function formatDuration(minutes: number): string {
  const m = Math.round(minutes);
  if (m < 60) return `${m} 分钟`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest === 0 ? `${h} 小时` : `${h} 小时 ${rest} 分`;
}

/** 票价：元 → 「5 元」/「免费」。0.5 的小数票价会自然带上小数。 */
export function formatPrice(yuan: number): string {
  if (yuan <= 0) return "免费";
  const text = Number.isInteger(yuan) ? String(yuan) : yuan.toFixed(1);
  return `${text} 元`;
}

/** 顺带把「坐 N 站」缩成短语，0 站不显示。 */
export function formatStations(count: number): string {
  return count > 0 ? `${count} 站` : "";
}
