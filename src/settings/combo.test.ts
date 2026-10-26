import { expect, test } from "bun:test";
import { comboFromEvent, keyCaps } from "./combo";
import { da } from "./i18n";

const ev = (key: string, code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) => ({
  key,
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

test("maps key presses to niri combos", () => {
  expect(comboFromEvent(ev(" ", "Space", { ctrlKey: true, altKey: true }))).toBe("Ctrl+Alt+Space");
  expect(comboFromEvent(ev("D", "KeyD", { shiftKey: true, metaKey: true }))).toBe("Shift+Super+D");
  expect(comboFromEvent(ev("!", "Digit1", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+1");
  expect(comboFromEvent(ev("æ", "Semicolon", { altKey: true }))).toBe("Alt+ae");
  expect(comboFromEvent(ev("Pause", "Pause"))).toBe("Pause");
  expect(comboFromEvent(ev("F13", "F13"))).toBe("F13");
});

test("rejects keys that would block typing or that niri cannot name", () => {
  expect(comboFromEvent(ev("a", "KeyA"))).toBeNull();
  expect(comboFromEvent(ev("Control", "ControlLeft", { ctrlKey: true }))).toBeNull();
  expect(comboFromEvent(ev(",", "Comma", { ctrlKey: true }))).toBeNull();
});

test("names letters by their place for macOS", () => {
  expect(comboFromEvent(ev("a", "KeyQ", { metaKey: true }), true)).toBe("Super+Q");
  expect(comboFromEvent(ev("æ", "Semicolon", { altKey: true }), true)).toBe("Alt+ae");
  expect(comboFromEvent(ev(",", "Comma", { ctrlKey: true }), true)).toBeNull();
  expect(keyCaps("Ctrl+Super+Space", true, da.keys)).toEqual(["⌃", "⌘", "Mellemrum"]);
  expect(keyCaps("Fn", true)).toEqual(["fn"]);
});

test("names key caps in the UI's language", () => {
  expect(keyCaps("Ctrl+Alt+Space", false, da.keys)).toEqual(["Ctrl", "Alt", "Mellemrum"]);
  expect(keyCaps("Ctrl+BackSpace")).toEqual(["Ctrl", "BackSpace"]);
  expect(keyCaps("Super+oslash")).toEqual(["Super", "Ø"]);
  expect(keyCaps("F2")).toEqual(["F2"]);
});
