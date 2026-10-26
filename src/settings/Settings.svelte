<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { flow } from "../flow";
  import { comboFromEvent, keyCaps } from "./combo";
  import { t } from "./i18n";
  import Picker from "./Picker.svelte";

  type Settings = {
    trailing_space: boolean;
    unload_after_min: number;
    pause_ms: number;
    hotkey: string | null;
    mic: string | null;
    language: string;
  };
  type Mic = { id: string; name: string };
  type HotkeyStatus =
    | { kind: "niri"; included: boolean; include_line: string }
    | { kind: "manual"; command: string; snippet: string | null }
    | { kind: "x11" }
    | { kind: "mac" };
  type Permissions = { microphone: boolean; accessibility: boolean };
  type Download =
    | { kind: "running"; done: number; total: number }
    | { kind: "failed" }
    | { kind: "done" };

  let settings = $state<Settings | null>(null);
  let status = $state<HotkeyStatus | null>(null);
  let download = $state<Download | null>(null);
  let autostart = $state<boolean | null>(null);
  let permissions = $state<Permissions | null>(null);
  let mics = $state<Mic[]>([]);
  let capturing = $state(false);
  let error = $state("");
  let version = $state("");
  let canvas: HTMLCanvasElement;
  let main: HTMLElement;
  let content: HTMLDivElement;

  const refreshMics = () => invoke<Mic[]>("microphones").then((m) => (mics = m));
  const refreshPermissions = () => invoke<Permissions | null>("permissions").then((p) => (permissions = p));
  listen<Download>("download", (e) => (download = e.payload));
  const loaded = Promise.allSettled([
    invoke<Settings>("get_settings").then((s) => (settings = s)),
    invoke<HotkeyStatus>("hotkey_status").then((s) => (status = s)),
    getVersion().then((v) => (version = v)),
    invoke<Download>("download_status").then((d) => (download = d)),
    invoke<boolean>("autostart").then((on) => (autostart = on)),
    refreshMics(),
    refreshPermissions(),
  ]);

  // the window takes the panel's height; the panel fills the window, so measure its content
  function fit() {
    const box = getComputedStyle(main);
    const padding = parseFloat(box.paddingTop) + parseFloat(box.paddingBottom);
    const border = main.offsetHeight - main.clientHeight;
    const height = content.getBoundingClientRect().height + padding + border + 2 * main.offsetTop;
    invoke("fit_settings", { height });
  }
  $effect(() => {
    const observer = new ResizeObserver(fit);
    // all loaded first, so the window resizes once on opening
    loaded.then(() => observer.observe(content));
    return () => observer.disconnect();
  });
  // each language by its own name
  const LANGUAGES = [
    { value: "da", label: "Dansk" },
    { value: "en", label: "English" },
  ];
  const micOptions = $derived([
    { value: null, label: t.systemDefault },
    ...mics.map((m) => ({ value: m.id, label: m.name })),
    ...(settings?.mic && !mics.some((m) => m.id === settings?.mic)
      ? [{ value: settings.mic, label: t.notConnected }]
      : []),
  ]);

  const mb = (bytes: number) => Math.round(bytes / 1e6).toLocaleString(t.locale);

  // macOS: an event tap takes the key, fn unless another is chosen
  const mac = $derived(status?.kind === "mac");
  const hotkey = $derived(settings?.hotkey ?? (mac ? "Fn" : null));

  const ASK = [
    { what: "microphone", name: t.microphone, why: t.microphoneWhy },
    { what: "accessibility", name: t.accessibility, why: t.accessibilityWhy },
  ] as const;
  const missing = $derived(!!permissions && !(permissions.microphone && permissions.accessibility));
  // granted in System Settings, so look again until both are there
  $effect(() => {
    if (!missing) return;
    const timer = setInterval(refreshPermissions, 2000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    const frame = flow(canvas);
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      frame(1 / 30, 0);
      return;
    }
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      frame((now - last) / 1000, 0);
      last = now;
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });

  const close = () => getCurrentWindow().close();

  async function save() {
    try {
      await invoke("set_settings", { settings });
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function saveAutostart() {
    try {
      await invoke("set_autostart", { on: autostart });
      error = "";
    } catch (e) {
      error = String(e);
      autostart = !autostart;
    }
  }

  function capture(e: MouseEvent) {
    // a focused button would turn the captured Space or Enter into another click
    (e.currentTarget as HTMLButtonElement).blur();
    capturing = true;
  }

  async function onkeydown(e: KeyboardEvent) {
    if (!capturing) {
      if (e.key === "Escape") close();
      return;
    }
    if (!settings) return;
    e.preventDefault();
    if (e.key === "Escape") {
      capturing = false;
      return;
    }
    const combo = comboFromEvent(e, mac);
    if (!combo) return;
    capturing = false;
    await setHotkey(combo);
  }

  async function setHotkey(combo: string) {
    if (!settings) return;
    try {
      await invoke("set_hotkey", { combo });
      settings.hotkey = combo;
      error = "";
    } catch (err) {
      error = String(err);
    }
  }
</script>

<svelte:window {onkeydown} onfocus={refreshMics} />

<main class="glass" bind:this={main}>
  <button class="close" onclick={close} aria-label={t.close}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
  </button>

  <div class="content" bind:this={content}>
    <header>
      <canvas class="ball" bind:this={canvas}></canvas>
      <div>
        <h1>Eckermann</h1>
        <p class="dedication">Til Rune</p>
        {#if version}<p>Version {version}</p>{/if}
      </div>
    </header>

    {#if download?.kind === "running"}
      <div class="download">
        {t.downloading}
        <span class="hint">
          {download.total ? t.downloaded(mb(download.done), mb(download.total)) : ""}{t.onlyOnce}
        </span>
        <progress value={download.done} max={download.total || 1}></progress>
      </div>
    {:else if download?.kind === "failed"}
      <div class="row">
        <span>
          {t.downloadFailed}
          <span class="hint">{t.checkConnection}</span>
        </span>
        <button onclick={() => invoke("download_models")}>{t.retry}</button>
      </div>
    {/if}

    {#if permissions}
      {#each ASK as ask}
        {#if !permissions[ask.what]}
          <div class="row">
            <span>
              {ask.name}
              <span class="hint">{ask.why}</span>
            </span>
            <button onclick={() => invoke("grant", { what: ask.what })}>{t.grant}</button>
          </div>
        {/if}
      {/each}
    {/if}

    {#if settings}
      <div class="row">
        <span>
          {t.hotkey}
          <span class="hint">
            {#if status?.kind === "manual"}
              {t.manualHint}
            {:else if capturing}
              {t.needsModifier(mac)}
            {:else}
              {t.toggleHint}
            {/if}
          </span>
        </span>
        {#if status?.kind !== "manual"}
          <span class="field">
            {#if capturing}
              <span class="listening">{t.pressNow}</span>
              <button onclick={() => (capturing = false)}>{t.cancel}</button>
            {:else}
              {#if hotkey}
                {#each keyCaps(hotkey, mac, t.keys) as key}<kbd>{key}</kbd>{/each}
              {/if}
              {#if mac && hotkey !== "Fn"}
                <button onclick={() => setHotkey("Fn")}>{t.useFn}</button>
              {/if}
              <button onclick={capture}>{hotkey ? t.change : t.choose}</button>
            {/if}
          </span>
        {/if}
      </div>
      {#if status?.kind === "manual"}
        <code>{status.command}</code>
        {#if status.snippet}
          <p class="hint">{t.snippetHint}</p>
          <code>{status.snippet}</code>
        {/if}
      {:else if status?.kind === "niri" && !status.included}
        <p class="hint">{t.niriHint}</p>
        <code>{status.include_line}</code>
      {:else if mac && hotkey === "Fn"}
        <p class="hint">{t.fnHint}</p>
      {/if}

      <div class="row">
        <span id="language-label">{t.language}</span>
        <Picker options={LANGUAGES} bind:value={settings.language} labelledby="language-label" onchange={save} />
      </div>

      <div class="row">
        <span id="mic-label">{t.microphone}</span>
        <Picker options={micOptions} bind:value={settings.mic} labelledby="mic-label" onchange={save} />
      </div>

      <label class="row">
        <span>
          {t.pause}
          <span class="hint">{t.pauseHint}</span>
        </span>
        <span class="field">
          <input type="number" min="0" max="5000" step="100" bind:value={settings.pause_ms} onchange={save} />
          <span class="unit">ms</span>
        </span>
      </label>

      <label class="row">
        <span>{t.trailingSpace}</span>
        <input type="checkbox" class="switch" bind:checked={settings.trailing_space} onchange={save} />
      </label>

      {#if autostart !== null}
        <label class="row">
          <span>{t.autostart}</span>
          <input type="checkbox" class="switch" bind:checked={autostart} onchange={saveAutostart} />
        </label>
      {/if}

      <label class="row">
        <span>
          {t.unload}
          <span class="hint">{t.unloadHint}</span>
        </span>
        <span class="field">
          <input type="number" min="1" max="120" bind:value={settings.unload_after_min} onchange={save} />
          <span class="unit">min</span>
        </span>
      </label>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
</main>

<style>
  main {
    --text: #ecebe8;
    --muted: #96938e;
    --rule: rgb(255 255 255 / 0.07);
    /* one height for fields, keys, buttons and the switch, so rows line up */
    --control: 28px;
    /* the margin around the panel holds its shadow */
    position: fixed;
    inset: 28px;
    box-sizing: border-box;
    overflow-y: auto;
    /* the last row's own 18px makes the bottom gap match the 36px sides */
    padding: 36px 36px 18px;
    border-radius: 26px;
    border-color: rgb(255 255 255 / 0.09);
    background: rgb(26 26 27 / 0.97);
    color: var(--text);
    font-size: 16px;
    line-height: 1.4;
  }
  .content {
    /* holds the children's margins, so its height is all the panel needs */
    display: flow-root;
  }
  header {
    display: flex;
    align-items: center;
    gap: 18px;
    margin-bottom: 24px;
  }
  .ball {
    width: 64px;
    height: 64px;
    flex-shrink: 0;
  }
  h1 {
    margin: 0;
    font: 600 26px/1.1 ui-rounded, system-ui, sans-serif;
    letter-spacing: -0.01em;
  }
  header p {
    margin: 4px 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  header .dedication {
    color: var(--text);
    font-size: 15px;
    font-style: italic;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 18px 0;
    border-top: 1px solid var(--rule);
  }
  .download {
    padding: 18px 0;
    border-top: 1px solid var(--rule);
  }
  progress {
    display: block;
    width: 100%;
    height: 6px;
    margin-top: 12px;
    border: none;
    border-radius: 3px;
    overflow: hidden;
    appearance: none;
  }
  progress::-webkit-progress-bar {
    background: rgb(255 255 255 / 0.1);
  }
  progress::-webkit-progress-value {
    background: var(--accent);
  }
  .hint {
    display: block;
    margin-top: 2px;
    color: var(--muted);
    font-size: 14px;
  }
  p.hint {
    margin: 0 0 8px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    color: var(--muted);
  }
  .unit {
    min-width: 1.8em;
  }
  .listening {
    color: var(--accent);
  }
  kbd {
    box-sizing: border-box;
    display: grid;
    place-items: center;
    min-width: var(--control);
    height: var(--control);
    padding: 0 9px;
    border: 1px solid rgb(255 255 255 / 0.16);
    border-bottom-width: 2px;
    border-radius: 7px;
    color: var(--text);
    font: 600 15px/1 system-ui, sans-serif;
  }
  code {
    display: block;
    margin-bottom: 18px;
    padding: 8px 10px;
    border-radius: 8px;
    background: rgb(0 0 0 / 0.3);
    font: 13px/1.4 ui-monospace, monospace;
    overflow-wrap: anywhere;
    user-select: all;
  }
  button {
    flex-shrink: 0;
    min-height: var(--control);
    padding: 0 12px;
    border: 1px solid rgb(255 255 255 / 0.12);
    border-radius: 14px;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    cursor: pointer;
  }
  button:hover {
    background: rgb(255 255 255 / 0.07);
  }
  .close {
    position: absolute;
    top: 16px;
    right: 16px;
    z-index: 2;
    display: grid;
    place-items: center;
    width: 32px;
    min-height: 32px;
    padding: 0;
    border: none;
  }
  .close svg {
    width: 14px;
    height: 14px;
    stroke: var(--muted);
    stroke-width: 1.8;
    stroke-linecap: round;
  }
  input[type="number"] {
    box-sizing: border-box;
    width: 4.2em;
    height: var(--control);
    padding: 0 10px;
    border: 1px solid rgb(255 255 255 / 0.1);
    border-radius: 8px;
    background: rgb(255 255 255 / 0.05);
    color: var(--text);
    font: inherit;
    text-align: right;
    appearance: textfield;
  }
  input[type="number"]::-webkit-inner-spin-button,
  input[type="number"]::-webkit-outer-spin-button {
    appearance: none;
    margin: 0;
  }
  .switch {
    appearance: none;
    position: relative;
    flex-shrink: 0;
    width: 46px;
    height: var(--control);
    margin: 0;
    border-radius: 14px;
    background: rgb(255 255 255 / 0.14);
    cursor: pointer;
    transition: background-color 160ms;
  }
  .switch::before {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--text);
    transition: translate 160ms;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::before {
    translate: 18px 0;
  }
  :is(button, input):focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .error {
    margin: 8px 0 18px;
    padding: 10px 14px;
    border-radius: 12px;
    background: rgb(122 54 30 / 0.5);
  }
  @media (prefers-reduced-motion: reduce) {
    .switch,
    .switch::before {
      transition: none;
    }
  }
</style>
