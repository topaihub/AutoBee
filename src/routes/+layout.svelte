<script lang="ts">
  import "../app.css";
  let { children } = $props<{ children: import("svelte").Snippet }>();
</script>

{@render children()}
