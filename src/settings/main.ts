import { invoke } from "@tauri-apps/api/core";
import { mount } from "svelte";
import "../theme.css";
import { setLanguage } from "./i18n";
import Settings from "./Settings.svelte";

invoke<string>("ui_language")
  .then(setLanguage)
  .finally(() => mount(Settings, { target: document.getElementById("app")! }));
