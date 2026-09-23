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

<div class={cn("p-4 pt-0", className)}>
  {@render children?.()}
</div>
