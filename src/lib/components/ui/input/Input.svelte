<script lang="ts">
  import { cn } from "$lib/utils/cn";

  let {
    class: className = "",
    value = $bindable(""),
    type = "text",
    placeholder = undefined,
    disabled = false,
    id = undefined,
    ariaLabel = undefined,
    oninput = () => {},
    onkeydown = () => {},
    onfocus = () => {},
    onblur = () => {},
  }: {
    class?: string;
    value?: string;
    type?: string;
    placeholder?: string;
    disabled?: boolean;
    id?: string;
    /** 无可见标签时的可访问名。紧凑行里放不下 `<Label>`，但控件必须有名字。 */
    ariaLabel?: string;
    oninput?: (e: Event & { currentTarget: HTMLInputElement }) => void;
    onkeydown?: (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => void;
    onfocus?: (e: FocusEvent & { currentTarget: HTMLInputElement }) => void;
    onblur?: (e: FocusEvent & { currentTarget: HTMLInputElement }) => void;
  } = $props();
</script>

<input
  {type}
  {id}
  aria-label={ariaLabel}
  bind:value
  {placeholder}
  {disabled}
  class={cn(
    "flex h-9 w-full rounded-md border border-input bg-card px-3 text-sm",
    "placeholder:text-muted-foreground",
    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
    "disabled:cursor-not-allowed disabled:opacity-50",
    className,
  )}
  {oninput}
  {onkeydown}
  {onfocus}
  {onblur}
/>
