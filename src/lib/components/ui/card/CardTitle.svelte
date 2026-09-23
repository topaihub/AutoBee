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

<h3 class={cn("text-sm font-semibold leading-none", className)}>
  {@render children?.()}
</h3>
