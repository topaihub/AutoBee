<script lang="ts" module>
  import { cn } from "$lib/utils/cn";

  export type BadgeVariant = "default" | "secondary" | "outline" | "success";

  const VARIANTS: Record<BadgeVariant, string> = {
    default: "bg-primary text-primary-foreground",
    secondary: "bg-secondary text-secondary-foreground",
    outline: "border text-foreground",
    success: "bg-success/15 text-success",
  };
</script>

<script lang="ts">
  let {
    variant = "secondary",
    class: className = "",
    children,
  }: {
    variant?: BadgeVariant;
    class?: string;
    children?: import("svelte").Snippet;
  } = $props();
</script>

<span
  class={cn(
    "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium",
    VARIANTS[variant],
    className,
  )}
>
  {@render children?.()}
</span>
