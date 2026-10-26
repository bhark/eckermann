<script lang="ts">
  import { onMount } from "svelte";
  import { spin } from "./orb";

  let { steps }: { steps: HTMLElement } = $props();

  const PHRASES = [
    "This is why modern man has heard enough about guilt and sin.",
    "He is sorely beset by his own bad conscience, and wants rather to know how he is to reconcile himself with his own nature;",
    "how he is to love the enemy in his own heart, and call the wolf his brother.",
  ].map((p) => p.split(" "));
  const WORDS = PHRASES.flat();
  const MAX_WIDTH = 520;

  // pipeline.rs cadence
  const PREVIEW_EVERY = 0.7;
  const WORD = 0.34;
  const MARK = 0.3;
  // vad pause before a phrase is cut, then transcribe and paste
  const CUT = 0.6;
  const PASTE = 0.9;
  const INK = 0.045;

  // one loop of the demo, in seconds
  const press = 1.6;
  let clock = press + 1.1;
  const phrases = PHRASES.map((words) => {
    const start = clock;
    const times = words.map((w) => {
      const at = clock;
      clock += WORD + (/[.,;]$/.test(w) ? MARK : 0);
      return at;
    });
    const end = clock;
    const paste = end + PASTE;
    clock = paste + 0.4;
    return { words, start, times, end, paste };
  });
  const last = phrases[phrases.length - 1];
  const stop = last.paste + last.words.length * INK + 1.2;
  const fadeAt = stop + 2.8;
  const total = stop + 3.5;

  function at(t: number) {
    const s = {
      key: (t >= press && t < press + 0.25) || (t >= stop && t < stop + 0.25),
      shown: t >= press + 0.1 && t < stop + 0.1,
      preview: "",
      written: 0,
      energy: 0,
      step: 1,
      fade: t >= fadeAt,
    };
    for (const p of phrases) {
      if (t >= p.paste) s.written += Math.min(p.words.length, Math.floor((t - p.paste) / INK) + 1);
      if (t >= p.start && t < p.paste) {
        // previews land in whole chunks, a beat behind the voice
        const tick = t >= p.end + 0.4 ? Infinity : p.start + Math.floor((t - p.start) / PREVIEW_EVERY) * PREVIEW_EVERY;
        s.preview = p.words.slice(0, p.times.filter((x) => x + 0.3 < tick).length).join(" ");
        s.step = 2;
        if (t < p.end - 0.1) s.energy = 1;
        else if (t >= p.end + CUT) s.energy = 0.35;
      }
      if (t >= p.paste && t < p.paste + p.words.length * INK + 0.4) s.step = 3;
    }
    return s;
  }

  // syllable-paced mic level, 0 to 1
  const voice = (t: number) => Math.max(0, Math.sin(t * 13) * 0.5 + Math.sin(t * 5.3) * 0.3 + 0.35);

  let t = $state(0);
  const s = $derived(at(t));
  let textWidth = $state(0);
  let canvas: HTMLCanvasElement;

  onMount(() => {
    const halt = spin(canvas, () => (s.energy === 1 ? voice(t) : s.energy));
    const t0 = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      t = ((now - t0) / 1000) % total;
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      halt();
      cancelAnimationFrame(raf);
    };
  });

  $effect(() => {
    steps.dataset.step = String(s.step);
    steps.toggleAttribute("data-key", s.key);
  });
</script>

<div class="window" role="img" aria-label="A passage from Jung dictated into a text editor">
  <div class="titlebar">letter.txt</div>
  <div class="letter">
    <p class:faded={s.fade}>
      {#each WORDS.slice(0, s.written) as word, i (i)}<span class="ink">{word}{i < s.written - 1 ? " " : ""}</span
        >{/each}<span class="caret" class:blink={s.step !== 3}></span>
    </p>
  </div>
  <div class="stage" class:shown={s.shown}>
    <div
      class="pill"
      class:open={!!s.preview}
      class:clipped={textWidth > MAX_WIDTH}
      style:width="{Math.min(textWidth, MAX_WIDTH)}px"
    >
      <span bind:clientWidth={textWidth}>{s.preview}</span>
    </div>
    <canvas class="orb" bind:this={canvas}></canvas>
  </div>
</div>

<style>
  p {
    transition: opacity 500ms;
  }
  .faded {
    opacity: 0;
  }
  .ink {
    display: inline-block;
    white-space: pre;
    animation: ink 420ms cubic-bezier(0.2, 0.9, 0.25, 1) both;
  }
  @keyframes ink {
    from {
      opacity: 0;
      filter: blur(6px);
      transform: translateY(3px);
    }
  }
  .caret {
    display: inline-block;
    width: 2px;
    height: 1.15em;
    margin-left: 1px;
    vertical-align: -0.2em;
    background: var(--accent);
  }
  .blink {
    animation: blink 1.1s step-end infinite;
  }
  @keyframes blink {
    55% {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .ink,
    .blink {
      animation: none;
    }
  }

  /* from Overlay.svelte, with the stage rising from the window's bottom edge */
  .stage {
    position: absolute;
    inset: auto 0 0;
    padding-bottom: 40px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    pointer-events: none;
    font: 15px/1 system-ui, sans-serif;
    color: #f2e6d2;
    transform-origin: 50% calc(100% - 78px);
    translate: 0 170px;
    scale: 0.6;
    opacity: 0;
    transition:
      scale 240ms cubic-bezier(0.5, 0, 0.75, 0),
      translate 380ms 160ms cubic-bezier(0.55, 0, 1, 0.45),
      opacity 180ms 360ms;
  }
  .stage.shown {
    translate: 0 0;
    scale: 1;
    opacity: 1;
    transition:
      translate 620ms cubic-bezier(0.16, 1, 0.3, 1),
      scale 560ms 420ms cubic-bezier(0.34, 1.56, 0.64, 1),
      opacity 160ms;
  }
  .pill {
    height: 34px;
    max-width: calc(100% - 32px);
    box-sizing: border-box;
    border-radius: 17px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    overflow: hidden;
    background: rgb(36 29 22 / 0.86);
    border: 1px solid rgb(240 222 190 / 0.14);
    box-shadow:
      inset 0 1px 0 rgb(255 245 225 / 0.1),
      0 10px 30px rgb(0 0 0 / 0.35),
      0 2px 6px rgb(0 0 0 / 0.25);
    opacity: 0;
    scale: 0.92;
    transition:
      width 360ms cubic-bezier(0.2, 0.9, 0.25, 1),
      opacity 220ms,
      scale 260ms;
  }
  .pill.open {
    opacity: 1;
    scale: 1;
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
