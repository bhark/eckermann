import { mount } from "svelte";
import "../theme.css";
import Settings from "./Settings.svelte";

mount(Settings, { target: document.getElementById("app")! });
