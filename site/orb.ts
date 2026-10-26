import { flow } from "../src/flow";

/** Runs the orb until the returned stop is called; energy is read every frame. */
export function spin(canvas: HTMLCanvasElement, energy: () => number = () => 0) {
  const frame = flow(canvas);
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    frame(1 / 30, 0);
    return () => {};
  }
  let raf = 0;
  let last = performance.now();
  const tick = (now: number) => {
    frame((now - last) / 1000, energy());
    last = now;
    raf = requestAnimationFrame(tick);
  };
  raf = requestAnimationFrame(tick);
  return () => cancelAnimationFrame(raf);
}
