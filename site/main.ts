import "@fontsource/ibm-plex-mono/latin-400.css";
import "./site.css";
import { mount } from "svelte";
import Demo from "./Demo.svelte";
import { spin } from "./orb";

spin(document.querySelector<HTMLCanvasElement>(".logo canvas")!);
// the still ball only stands in until the live one draws
document.querySelector(".logo img")!.remove();

const demo = document.getElementById("demo")!;
demo.replaceChildren();
mount(Demo, { target: demo, props: { steps: document.querySelector<HTMLElement>(".steps")! } });
