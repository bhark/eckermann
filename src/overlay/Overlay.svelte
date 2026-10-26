<script lang="ts">
  import { emit, listen } from "@tauri-apps/api/event";
  import { flow } from "../flow";

  type Event =
    | { kind: "recording"; value: boolean }
    | { kind: "level"; value: number }
    | { kind: "preview"; value: string }
    | { kind: "pending"; value: number }
    | { kind: "notice"; value: string };

  const MAX_WIDTH = 520;
  // overlay.rs puts the window under the menu bar on macOS
  const top = navigator.userAgent.includes("Mac");

  let shown = $state(false);
  let recording = $state(false);
  let preview = $state("");
  let pending = $state(0);
  let notice = $state("");
  let textWidth = $state(0);
  // 0 to 1; read every frame, so not reactive
  let voice = 0;
  let floorDb = Infinity;
  let canvas: HTMLCanvasElement;
  let stage: HTMLDivElement;

  listen<boolean>("shown", (e) => (shown = e.payload));
  listen<Event>("overlay", ({ payload: e }) => {
    if (e.kind === "recording") {
      recording = e.value;
      voice = 0;
      floorDb = Infinity;
    } else if (e.kind === "level") {
      const db = 20 * Math.log10(Math.max(e.value, 3e-5));
      // noise floor drops to any quieter moment and creeps up 3 dB/s, so mic gain does not matter
      floorDb = Math.min(db, floorDb + 0.1);
      voice = Math.min(1, Math.max(0, (db - floorDb - 6) / 20));
    }
    else if (e.kind === "preview") preview = e.value;
    else if (e.kind === "pending") pending = e.value;
    else notice = e.value;
  });

  // line breaks would collapse to nothing in the one-line preview
  let text = $derived(notice || preview.replaceAll("\n\n", " ¶ ").replaceAll("\n", " ↵ "));

  const energy = () => (recording ? voice : pending > 0 ? 0.35 : 0);

  let frame: ReturnType<typeof flow> | undefined;
  let running = false;
  let leaving = false;

  $effect(() => {
    if (!shown) {
      leaving = true;
      // getAnimations also returns the leave transitions this change just started
      Promise.allSettled(stage.getAnimations().map((a) => a.finished)).then(() => {
        leaving = false;
        // overlay.rs ignores this when shown again meanwhile
        emit("overlay-left");
      });
      return;
    }
    frame ??= flow(canvas);
    if (running) return;
    running = true;
    let last = performance.now();
    const tick = (now: number) => {
      frame!((now - last) / 1000, energy());
      last = now;
      // keep drawing through the leave transition, then rest
      if (shown || leaving) requestAnimationFrame(tick);
      else running = false;
    };
    requestAnimationFrame(tick);
  });
</script>

<div class="stage" class:shown class:top bind:this={stage}>
  <div
    class="glass pill"
    class:open={!!text}
    class:notice={!!notice}
    class:clipped={textWidth > MAX_WIDTH}
    style:width="{Math.min(textWidth, MAX_WIDTH)}px"
  >
    <span bind:clientWidth={textWidth}>{text}</span>
  </div>
  <canvas class="orb" bind:this={canvas}></canvas>
</div>

<style>
  .stage {
    height: 100%;
    box-sizing: border-box;
    padding-bottom: 48px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    font: 15px/1 system-ui, sans-serif;
    color: #f2e6d2;
    /* orb centre */
    transform-origin: 50% calc(100% - 86px);
    translate: 0 170px;
    scale: 0.6;
    opacity: 0;
    /* leave: shrink, then sink */
    transition:
      scale 240ms cubic-bezier(0.5, 0, 0.75, 0),
      translate 380ms 160ms cubic-bezier(0.55, 0, 1, 0.45),
      opacity 180ms 360ms;
  }
  .stage.shown {
    translate: 0 0;
    scale: 1;
    opacity: 1;
    /* enter: rise small and settle, then grow into place */
    transition:
      translate 620ms cubic-bezier(0.16, 1, 0.3, 1),
      scale 560ms 420ms cubic-bezier(0.34, 1.56, 0.64, 1),
      opacity 160ms;
  }
  /* mirrored: orb on top, dropping in from above */
  .stage.top {
    flex-direction: column-reverse;
    padding: 16px 0 0;
    transform-origin: 50% 54px;
    translate: 0 -170px;
  }
  .stage.top.shown {
    translate: 0 0;
  }
  .pill {
    height: 34px;
    border-radius: 17px;
    display: flex;
    align-items: center;
    /* while the width catches up, the newest words stay visible */
    justify-content: flex-end;
    opacity: 0;
    scale: 0.92;
    transition:
      width 360ms cubic-bezier(0.2, 0.9, 0.25, 1),
      opacity 220ms,
      scale 260ms,
      background-color 300ms;
  }
  .pill.open {
    opacity: 1;
    scale: 1;
  }
  .pill.notice {
    background: rgb(122 54 30 / 0.86);
  }
  .pill.clipped {
    mask-image: linear-gradient(to right, transparent, #000 48px);
  }
  .pill span {
    flex: none;
    padding: 0 16px;
    white-space: nowrap;
  }
  .orb {
    width: 76px;
    height: 76px;
    flex-shrink: 0;
  }
</style>
