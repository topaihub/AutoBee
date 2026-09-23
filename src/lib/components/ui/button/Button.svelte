<script lang="ts" module>
  import { cn } from "$lib/utils/cn";

  export type ButtonVariant = "default" | "outline" | "ghost" | "destructive";
  export type ButtonSize = "default" | "sm" | "icon";

  const VARIANTS: Record<ButtonVariant, string> = {
    default: "bg-primary text-primary-foreground hover:bg-primary/90",
    outline: "border bg-card text-foreground hover:bg-accent hover:text-accent-foreground",
    ghost: "hover:bg-accent hover:text-accent-foreground",
    destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90",
  };

  const SIZES: Record<ButtonSize, string> = {
    default: "h-9 px-4",
    sm: "h-8 px-3 text-xs",
    icon: "size-9",
  };
</script>

<script lang="ts">
  let {
    variant = "default",
    size = "default",
    class: className = "",
    type = "button",
    disabled = false,
    title = undefined,
    ariaLabel = undefined,
    onclick = () => {},
    children,
  }: {
    variant?: ButtonVariant;
    size?: ButtonSize;
    class?: string;
    type?: "button" | "submit";
    disabled?: boolean;
    title?: string;
    ariaLabel?: string;
    onclick?: (e: MouseEvent) => void;
    children?: import("svelte").Snippet;
  } = $props();
</script>

<button
  {type}
  class={cn(
    // focus-visible 的 ring 不做动画（hallmark：焦点环必须即时出现）
    "inline-flex items-center justify-center gap-2 rounded-md text-sm font-medium",
    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
    "active:translate-y-px transition-colors",
    "disabled:pointer-events-none disabled:opacity-50",
    VARIANTS[variant],
    SIZES[size],
    className,
  )}
  {disabled}
  {title}
  aria-label={ariaLabel}
  {onclick}
>
  {@render children?.()}
</button>
