/**
 * Central shortcut registry. Every discrete keyboard shortcut the package installs lives in
 * ONE editable table below — add future shortcuts here (id, default combo, a one-line
 * description) instead of scattering key handling across the engine. Platforms re-key any
 * binding through the host feature's option (e.g. `create_chart_grid(..., { shortcuts: {
 * "grid.split_vertical": "ctrl+shift+x" } })` — no engine rebuild needed for them; editing
 * DEFAULT_SHORTCUTS + rebuilding re-keys the engine's own defaults.
 */

/** Action ids the package ships shortcuts for. */
export type shortcut_action =
  | "grid.split_horizontal"
  | "grid.split_vertical"
  | "drawing.undo"
  | "drawing.redo";

export interface shortcut_binding {
  /** Default key combo: `"ctrl+h"`, `"ctrl+shift+x"`, `"alt+1"` (ctrl ≡ ctrl/cmd on any OS). */
  combo: string;
  description: string;
}

/** The engine's default key table — edit here and rebuild to re-key globally. */
export const DEFAULT_SHORTCUTS: Record<shortcut_action, shortcut_binding> = {
  "grid.split_horizontal": {
    combo: "ctrl+h",
    description: "Split the active chart horizontally (side by side)",
  },
  "grid.split_vertical": {
    combo: "ctrl+v",
    description: "Split the active chart vertically (stacked)",
  },
  "drawing.undo": {
    combo: "ctrl+z",
    description: "Undo the active chart's last drawing operation",
  },
  "drawing.redo": {
    combo: "ctrl+shift+z",
    description: "Redo the active chart's last drawing operation",
  },
};

/** A resolved shortcut ready to install: parsed combo plus its handler. */
export interface resolved_shortcut {
  combo: string;
  run: (event: KeyboardEvent) => void;
}

interface parsed_combo {
  key: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
}

/** Parse `"ctrl+shift+x"`-style combos; `ctrl` matches Ctrl or Cmd (meta) at event time. */
function parse_combo(combo: string): parsed_combo | null {
  const parts = combo
    .toLowerCase()
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean);
  const key = parts[parts.length - 1] ?? "";
  if (key.length !== 1) return null;
  return {
    key,
    ctrl: parts.includes("ctrl") || parts.includes("cmd") || parts.includes("meta"),
    shift: parts.includes("shift"),
    alt: parts.includes("alt"),
  };
}

/** Keystrokes landing in a field are editing, not shortcuts. */
function is_editing_target(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return (
    el !== null &&
    (el.tagName === "INPUT" ||
      el.tagName === "TEXTAREA" ||
      el.tagName === "SELECT" ||
      el.isContentEditable)
  );
}

/**
 * Install a set of shortcuts on the document: each fires when its combo matches exactly
 * (modifiers equal, key equal, no auto-repeat, not while editing). The first match wins and
 * gets `preventDefault` (browser history/paste would otherwise eat Ctrl+H/Ctrl+V). Returns
 * the detach function.
 */
export function install_shortcuts(bindings: resolved_shortcut[]): () => void {
  const parsed = bindings
    .map((binding) => {
      const combo = parse_combo(binding.combo);
      return combo === null ? null : { ...combo, run: binding.run };
    })
    .filter((b): b is NonNullable<typeof b> => b !== null);
  const on_keydown = (e: KeyboardEvent) => {
    if (e.defaultPrevented || e.repeat || is_editing_target(e.target)) return;
    const wants_ctrl = e.ctrlKey || e.metaKey;
    for (const binding of parsed) {
      if (
        e.key.toLowerCase() === binding.key &&
        wants_ctrl === binding.ctrl &&
        e.shiftKey === binding.shift &&
        e.altKey === binding.alt
      ) {
        e.preventDefault();
        binding.run(e);
        return;
      }
    }
  };
  document.addEventListener("keydown", on_keydown);
  return () => document.removeEventListener("keydown", on_keydown);
}
