/**
 * The palette, joined to the running app.
 *
 * `theme.js` is deliberately pure — it does colour arithmetic and knows nothing about Tauri, which
 * is what lets `node --test` run it directly. This is the other half: reading the stored theme,
 * asking the desktop what it wants when the answer is "system", and keeping both windows in step
 * as either changes.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { applyTheme, resolveMode, type ThemeInput } from "./theme.js";
import type { ThemeSettings } from "../model";

/**
 * `sakatsu_core::store::Theme` — the inputs, not the tokens.
 *
 * **An alias, not a second copy.** This was a hand-written duplicate of `ThemeSettings`, which is
 * a field-for-field transcription of the same Rust struct kept in two places: adding `kind_spin`
 * to one left the other a field short, and the only thing that noticed was the compiler at a call
 * site three files away. One name for one struct, so the next field lands once.
 */
export type Theme = ThemeSettings;

/** The event `save_settings` emits; `commands::THEME_CHANGED`. */
const CHANGED = "theme-changed";

/**
 * `Theme::default()`, once.
 *
 * **A function, not a constant, and one copy rather than three.** The same literal was written out
 * in `App.svelte`, in `Frame.svelte` and here — so `kind_spin` had to be added in three places and
 * nothing but the compiler was going to mention the one that got missed. A factory rather than a
 * shared object because each caller hands it to `$state`, and a shared `overrides` would be one
 * map behind two windows that both think they own it.
 *
 * Still a transcription of the Rust default, which is the authority; it exists because the window
 * has to paint before any settings have been read back.
 */
export function defaultTheme(): Theme {
  return {
    mode: "dark",
    surface_hue: 300,
    accent_hue: 297,
    kind_spin: 0,
    tint: 1,
    contrast: 1,
    depth: 0,
    overrides: {},
  };
}

const DEFAULT: Theme = defaultTheme();

/**
 * What the desktop is asking for.
 *
 * **Asked of the window, not of `prefers-color-scheme`.** A media query inside the webview answers
 * whatever the window's theme is set to, and Tauri lets that be pinned — it was pinned to `Dark`
 * in `tauri.conf.json` until this landed, which made "follow the system" quietly unobservable.
 * `apply_window_theme` passes `None` for system mode so this question reaches the OS.
 */
async function osPrefers(): Promise<"light" | "dark"> {
  try {
    return (await getCurrentWindow().theme()) === "light" ? "light" : "dark";
  } catch {
    // No window to ask — a browser, or a test harness. The static palette is dark.
    return "dark";
  }
}

/** Turn the stored inputs into the shape `theme.js` takes. */
function toInput(theme: Theme): ThemeInput {
  return {
    mode: theme.mode,
    surfaceHue: theme.surface_hue,
    kindSpin: theme.kind_spin,
    accentHue: theme.accent_hue,
    tint: theme.tint,
    contrast: theme.contrast,
    depth: theme.depth,
    overrides: theme.overrides,
  };
}

/** Derive and write a theme onto a root, resolving `system` first. */
export async function paint(root: HTMLElement, theme: Theme) {
  applyTheme(root, toInput(theme), resolveMode(theme.mode, await osPrefers()));
}

/**
 * Read the saved theme and apply it.
 *
 * Used by `main.ts` before either window mounts, so nothing is ever painted in the wrong palette
 * and corrected afterwards.
 */
export async function applyStoredTheme(root: HTMLElement): Promise<Theme> {
  let theme = DEFAULT;
  try {
    const saved = await invoke<{ theme?: Theme }>("settings");
    if (saved?.theme) theme = saved.theme;
  } catch {
    // Settings unreadable. Dark is the default and the static stylesheet already says so.
  }
  await paint(root, theme);
  return theme;
}

/**
 * Follow the theme for as long as the window lives.
 *
 * Two sources, and both are needed:
 *
 * - **The app's own changes**, pushed by `save_settings`. This is the only route into the capture
 *   frame, whose window is created once and only ever hidden — the same reason the tint is pushed.
 * - **The desktop's changes**, which matter only under `system` but cannot be predicted, so the
 *   listener is always attached and the mode decides whether it does anything.
 *
 * @returns a function that stops both listeners
 */
export function watchTheme(root: HTMLElement, current: () => Theme): () => void {
  const stops: Array<() => void> = [];

  listen<Theme>(CHANGED, (event) => {
    void paint(root, event.payload);
  })
    .then((off) => stops.push(off))
    .catch(() => {});

  getCurrentWindow()
    .onThemeChanged(() => {
      // Only `system` is listening for this; an explicit light or dark ignores the desktop by
      // definition, and repainting anyway would be a flicker for nothing.
      if (current().mode === "system") void paint(root, current());
    })
    .then((off) => stops.push(off))
    .catch(() => {});

  return () => {
    for (const stop of stops) stop();
  };
}
