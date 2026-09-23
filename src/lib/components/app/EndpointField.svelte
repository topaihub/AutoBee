<script lang="ts">
  /**
   * 地点输入:防抖联想(state 层做了 250ms),下拉点选锁定坐标。
   *
   * 三个字段共用它 -- 起点、终点、以及生活圈自己的中心点。Enter 的动作按
   * 字段分:换乘两端触发规划,生活圈触发生活圈查询。
   */
  import { planner, type Field } from "$lib/state/planner.svelte";
  import Input from "$lib/components/ui/input/Input.svelte";

  let {
    field,
    placeholder,
    inputId = undefined,
  }: {
    field: Field;
    placeholder: string;
    inputId?: string;
  } = $props();

  const model = $derived(planner);
  const text = $derived(
    field === "from" ? model.fromText : field === "to" ? model.toText : model.circleText,
  );
  const suggestions = $derived(
    field === "from"
      ? model.fromSuggestions
      : field === "to"
        ? model.toSuggestions
        : model.circleSuggestions,
  );
  const busy = $derived(model.suggesting === field);
  const dotClass = $derived(
    field === "from" ? "bg-success" : field === "to" ? "bg-destructive" : "bg-primary",
  );

  let focused = $state(false);
  let open = $derived(focused && suggestions.length > 0);

  function onInput(e: Event & { currentTarget: HTMLInputElement }) {
    planner.onInput(field, e.currentTarget.value);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    if (field === "circle") void planner.loadLifeCircle();
    else void planner.run();
  }
</script>

<div class="relative">
  <span
    class="pointer-events-none absolute left-3 top-1/2 size-2 -translate-y-1/2 rounded-full {dotClass}"
    aria-hidden="true"
  ></span>
  <Input
    id={inputId}
    class="pl-6"
    {placeholder}
    value={text}
    oninput={onInput}
    onkeydown={onKeydown}
    onfocus={() => (focused = true)}
    onblur={() => setTimeout(() => (focused = false), 120)}
  />
  {#if busy}
    <span class="absolute right-3 top-1/2 -translate-y-1/2 text-[10px] text-muted-foreground">…</span>
  {/if}

  {#if open}
    <ul
      class="absolute top-full z-30 mt-1 max-h-64 w-full overflow-y-auto rounded-md border bg-popover py-1 shadow-md"
      role="listbox"
    >
      {#each suggestions as place (place.id ?? `${place.title}@${place.location.lat},${place.location.lng}`)}
        <li>
          <button
            type="button"
            class="w-full px-3 py-2 text-left text-sm hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
            onmousedown={(e) => e.preventDefault()}
            onclick={() => planner.pick(field, place)}
          >
            <span class="block truncate font-medium">{place.title}</span>
            {#if place.address}
              <span class="block truncate text-xs text-muted-foreground">{place.address}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
