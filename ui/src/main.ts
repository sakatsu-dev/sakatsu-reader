import { mount } from "svelte";
// Design tokens and base styles. Imported once here so both the main window and the
// capture frame get them — components reference the tokens and never raw colours.
import "./theme.css";
import App from "./App.svelte";
import Frame from "./lib/Frame.svelte";
import { applyStoredTheme } from "./lib/theme-live";

// The capture frame runs in its own transparent window, but out of the same bundle:
// `?frame=1` picks it. A separate Vite entry point would mean a multi-page build for one
// component, and the bundle is local and tiny either way.
const isFrame = new URLSearchParams(window.location.search).has("frame");

/**
 * The palette first, then the window.
 *
 * **Painted before `mount` on purpose.** `theme.css` carries the dark values statically, so a
 * light theme applied from inside a component would paint dark and repaint a frame later — a
 * flash on every launch. This is the one place that runs first for *both* windows, which is the
 * other half of why it is here: the capture frame is a separate document and gets no help from
 * the main one.
 *
 * **An async function rather than a top-level `await`**, which the build target (es2021) does not
 * have — esbuild refuses the bundle outright rather than degrading, so this is a build error and
 * not a runtime surprise. Raising the target for one statement would be a larger change than
 * wrapping it.
 *
 * A theme that cannot be read leaves the static dark in place and mounts anyway. Nothing about a
 * palette is worth an empty window.
 */
async function start() {
  const params = new URLSearchParams(window.location.search);
  // Browser QA exercises the real Svelte tree and CSS with deterministic desktop IPC. Kept out
  // of production by the compile-time DEV guard; installed before theme loading because that is
  // the first settings request the app makes.
  if (import.meta.env.DEV && params.has("qa")) {
    const { installAnalysisQa } = await import("./qa/analysis");
    await installAnalysisQa(params);
  }
  try {
    await applyStoredTheme(document.documentElement);
  } catch {
    // The stylesheet already says dark.
  }
  mount(isFrame ? Frame : App, {
    target: document.getElementById("app")!,
  });
}

void start();
