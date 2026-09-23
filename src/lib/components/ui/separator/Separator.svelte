<script lang="ts">
  import { cn } from "$lib/utils/cn";

  let {
    class: className = "",
    orientation = "horizontal",
  }: {
    class?: string;
    orientation?: "horizontal" | "vertical";
  } = $props();
</script>

<div
  role="separator"
  class={cn(
    "bg-border shrink-0",
    orientation === "horizontal" ? "h-px w-full" : "h-full w-px",
    className,
  )}
></div>
