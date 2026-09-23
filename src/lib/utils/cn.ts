/** 合并类名。真值才参与拼接（`clsx` 依赖太轻，不值得为此引包）。 */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
