<script lang="ts">
  type Option = { value: string | null; label: string };
  let {
    options,
    value = $bindable(),
    labelledby,
    onchange,
  }: { options: Option[]; value: string | null; labelledby: string; onchange: () => void } = $props();

  const id = $props.id();
  let open = $state(false);
  let active = $state(0);
  let root: HTMLDivElement;
  let button: HTMLButtonElement;
  let list = $state<HTMLUListElement>();
  const current = $derived(options.find((o) => o.value === value));

  $effect(() => list?.focus());
  $effect(() => list?.children[active]?.scrollIntoView({ block: "nearest" }));

  function show() {
    active = Math.max(0, options.findIndex((o) => o.value === value));
    open = true;
  }

  function hide() {
    open = false;
    button.focus();
  }

  function pick(i: number) {
    value = options[i].value;
    hide();
    onchange();
  }

  function onButtonKey(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    show();
  }

  function onListKey(e: KeyboardEvent) {
    // kept here, so Escape closes the list and not the window
    e.stopPropagation();
    const last = options.length - 1;
    switch (e.key) {
      case "ArrowDown":
        active = Math.min(active + 1, last);
        break;
      case "ArrowUp":
        active = Math.max(active - 1, 0);
        break;
      case "Home":
        active = 0;
        break;
      case "End":
        active = last;
        break;
      case "Enter":
      case " ":
        pick(active);
        break;
      case "Escape":
        hide();
        break;
      case "Tab":
        open = false;
        return;
      default:
        return;
    }
    e.preventDefault();
  }

  function onWindowPointer(e: PointerEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onpointerdown={onWindowPointer} />

<div class="picker" bind:this={root}>
  <button
    bind:this={button}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-labelledby="{labelledby} {id}-current"
    onclick={() => (open ? hide() : show())}
    onkeydown={onButtonKey}
  >
    <span id="{id}-current">{current?.label}</span>
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5l3 3 3-3" /></svg>
  </button>
  {#if open}
    <ul
      bind:this={list}
      role="listbox"
      tabindex="-1"
      aria-labelledby={labelledby}
      aria-activedescendant="{id}-{active}"
      onkeydown={onListKey}
    >
      {#each options as option, i (option.value)}
        <li
          id="{id}-{i}"
          role="option"
          tabindex="-1"
          aria-selected={option.value === value}
          class:active={i === active}
          onpointermove={() => (active = i)}
          onclick={() => pick(i)}
          onkeydown={onListKey}
        >
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 6.5l2.5 2.5 4.5-5.5" /></svg>
          <span>{option.label}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    min-width: 0;
  }
  button {
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 16em;
    height: var(--control);
    padding: 0 8px 0 10px;
    border: 1px solid rgb(255 255 255 / 0.1);
    border-radius: 8px;
    background: rgb(255 255 255 / 0.05);
    color: var(--text);
    font: inherit;
    font-size: 15px;
    cursor: pointer;
  }
  button:hover,
  button[aria-expanded="true"] {
    background: rgb(255 255 255 / 0.09);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  svg {
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  ul {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 3;
    box-sizing: border-box;
    min-width: 100%;
    width: max-content;
    max-width: 20em;
    max-height: 240px;
    margin: 0;
    padding: 4px;
    overflow-y: auto;
    border: 1px solid rgb(255 255 255 / 0.12);
    border-radius: 10px;
    background: rgb(40 40 42);
    box-shadow: 0 14px 36px rgb(0 0 0 / 0.5);
    list-style: none;
    font-size: 15px;
  }
  ul:focus {
    outline: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px 6px 8px;
    border-radius: 6px;
    cursor: pointer;
  }
  li.active {
    background: rgb(255 255 255 / 0.08);
  }
  li svg {
    visibility: hidden;
    stroke: var(--accent);
    stroke-width: 1.8;
  }
  li[aria-selected="true"] svg {
    visibility: visible;
  }
</style>
