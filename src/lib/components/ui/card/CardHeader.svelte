<script lang="ts">
  import { cn } from "$lib/utils/cn";

  let {
    class: className = "",
    children,
  }: {
    class?: string;
    children?: import("svelte").Snippet;
  } = $props();
</script>

<div class={cn("flex flex-col gap-1.5 p-4", className)}>
  {@render children?.()}
</div>
