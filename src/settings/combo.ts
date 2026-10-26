// niri key names (xkb keysyms) by KeyboardEvent.code
const NAMED: Record<string, string> = {
  Space: "Space",
  Enter: "Return",
  Tab: "Tab",
  Backspace: "BackSpace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "Page_Up",
  PageDown: "Page_Down",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Pause: "Pause",
  ScrollLock: "Scroll_Lock",
  PrintScreen: "Print",
  ContextMenu: "Menu",
};
const LETTERS: Record<string, string> = { æ: "ae", ø: "oslash", å: "aring" };
// where a Danish keyboard has æ, ø and å, by KeyboardEvent.code
const DANISH: Record<string, string> = { Semicolon: "ae", Quote: "oslash", BracketLeft: "aring" };
// keys that type nothing, so they may be a hotkey without a modifier
const BARE = /^(F\d+|Insert|Pause|Scroll_Lock|Print|Menu)$/;

type Keys = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/**
 * Combo in niri syntax, e.g. "Ctrl+Alt+Space"; null while only modifiers are down or for unsupported keys.
 * `physical` names letters by their place on the keyboard, as macOS matches key codes.
 */
export function comboFromEvent(e: Keys, physical = false): string | null {
  const key = keyName(e, physical);
  if (!key) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(
    (m) => m !== false,
  );
  if (mods.length === 0 && !BARE.test(key)) return null;
  return [...mods, key].join("+");
}

function keyName(e: Keys, physical: boolean): string | null {
  if (e.code in NAMED) return NAMED[e.code];
  if (/^F\d{1,2}$/.test(e.code)) return e.code;
  const digit = /^Digit(\d)$/.exec(e.code);
  if (digit) return digit[1];
  if (physical) return /^Key([A-Z])$/.exec(e.code)?.[1] ?? DANISH[e.code] ?? null;
  // letters by produced key, since niri matches the layout's keysym
  const letter = e.key.toLowerCase();
  if (/^[a-z]$/.test(letter)) return letter.toUpperCase();
  return LETTERS[letter] ?? null;
}

const CAPS: Record<string, string> = {
  ae: "Æ",
  oslash: "Ø",
  aring: "Å",
  Return: "Enter",
  Page_Up: "Page Up",
  Page_Down: "Page Down",
  Scroll_Lock: "Scroll Lock",
};

const MAC: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘", Fn: "fn" };

/** Key cap texts for a niri combo, with Apple's modifier signs when `mac`; `words` names keys in the UI's language. */
export const keyCaps = (combo: string, mac = false, words: Record<string, string> = {}) =>
  combo.split("+").map((key) => (mac ? MAC[key] : undefined) ?? words[key] ?? CAPS[key] ?? key);
