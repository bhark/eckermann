import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import tauri from "../src-tauri/tauri.conf.json" with { type: "json" };

export default defineConfig({
  plugins: [
    svelte(),
    { name: "version", transformIndexHtml: { order: "pre", handler: (html) => html.replace("%VERSION%", tauri.version) } },
  ],
});
