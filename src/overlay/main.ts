import { mount } from "svelte";
import "../theme.css";
import Overlay from "./Overlay.svelte";

mount(Overlay, { target: document.getElementById("app")! });
