/**
 * What the reader is told about the engines: the phrase beside the capture hint, and the rows in
 * Settings → Diagnostics.
 *
 * # Only what was reported
 *
 * Every phrase here comes from a status the backend recorded at the time — which model Auto
 * picked, what llama.cpp said it offloaded. Where nothing was reported, the phrase says nothing
 * rather than guess: an unknown device is left out, never rounded up to "GPU". And all-layers
 * offloaded is said as "GPU acceleration active (N/M layers)", not "runs on the GPU", because
 * the layer count is what llama.cpp reported and the rest of the pipeline is not covered by it.
 *
 * # Only for the request it belongs to
 *
 * A status slot holds the latest *published* result, and the reader may have moved on since.
 * {@link sameRequest} is how the interface checks that a result answers the request it owns
 * before showing it.
 *
 * Its own module because `App.svelte` and `Settings.svelte` use runes and `node --test` cannot
 * import them.
 */

/** @typedef {import("../model").Acceleration} Acceleration */
/** @typedef {import("../model").EngineStatus} EngineStatus */
/** @typedef {import("../model").OperationResult} OperationResult */
/** @typedef {import("../model").RequestIdentity} RequestIdentity */
/** @typedef {(engine: string) => string} Label */

/**
 * Whether two request identities name the same request.
 *
 * @param {RequestIdentity | null | undefined} left
 * @param {RequestIdentity | null | undefined} right
 */
export function sameRequest(left, right) {
  return (
    Boolean(left && right) &&
    left?.owner === right?.owner &&
    left?.operation === right?.operation &&
    left?.generation === right?.generation
  );
}

/**
 * The device, as reported. Empty when nothing was reported.
 *
 * @param {Acceleration | null | undefined} acceleration
 */
export function accelerationPhrase(acceleration) {
  switch (acceleration?.kind) {
    case "cpu":
      return "CPU";
    case "gpu_layers":
      return `GPU acceleration active (${acceleration.offloaded}/${acceleration.total} layers)`;
    default:
      return "";
  }
}

/**
 * The short phrase beside the capture hint for the recognition that produced the passage on
 * screen, or `null` when there is nothing worth adding to the engine's name.
 *
 * Two things are worth adding: which model Auto handed the image to, and verified GPU
 * acceleration. A processor-only read adds nothing to a hint about how to capture; it is in
 * Settings → Diagnostics for whoever asks.
 *
 * @param {OperationResult | null | undefined} result
 * @param {string} configured The engine the settings ask for, as the backend names it.
 * @param {Label} label
 */
export function recognitionPhrase(result, configured, label) {
  if (!result || result.outcome.kind !== "succeeded" || !result.engine) return null;
  const parts = [];
  if (result.engine !== configured) parts.push(`read by ${label(result.engine)}`);
  if (result.acceleration.kind === "gpu_layers")
    parts.push(accelerationPhrase(result.acceleration));
  return parts.length > 0 ? parts.join(", ") : null;
}

const HOSTING = {
  in_process: "in the app",
  managed_local: "a local service the app runs",
};

/**
 * Where an engine runs, in words.
 *
 * @param {EngineStatus["hosting"]} hosting
 */
export function hostingPhrase(hosting) {
  if (hosting.kind === "endpoint") {
    return hosting.external ? "an external endpoint" : "a local endpoint";
  }
  return HOSTING[hosting.kind];
}

const STATE = {
  not_installed: "not installed",
  not_loaded: "not loaded yet",
  loading: "loading",
  ready: "ready",
};

/**
 * One Settings → Diagnostics row per engine.
 *
 * @param {EngineStatus} engine
 * @param {Label} label
 * @returns {{ role: string, name: string, detail: string }}
 */
export function engineRow(engine, label) {
  const role = { recognition: "Recognition", analysis: "Analysis", translation: "Translation" }[
    engine.role
  ];
  // Nothing installed has no name to give and nowhere to run: "none not installed" said it twice.
  if (engine.state === "not_installed") return { role, name: "Not installed", detail: "" };
  const loaded =
    engine.loaded && engine.loaded !== engine.configured ? `, using ${label(engine.loaded)}` : "";
  const device = accelerationPhrase(engine.acceleration);
  const detail = [`${STATE[engine.state]}${loaded}`, hostingPhrase(engine.hosting), device]
    .filter(Boolean)
    .join(" · ");
  return { role, name: label(engine.configured), detail };
}

/**
 * The last result of one kind, in words — or `null` when there has been none this session.
 *
 * @param {OperationResult | null | undefined} result
 * @param {Label} label
 */
export function resultPhrase(result, label) {
  if (!result) return null;
  if (result.outcome.kind === "failed") {
    return `${result.engine ? `${label(result.engine)} · ` : ""}failed (${result.outcome.code.replaceAll("_", " ")})`;
  }
  const cache = { fresh: "", cached: "from cache", mixed: "partly from cache" }[result.cache];
  const device = result.cache === "cached" ? "" : accelerationPhrase(result.acceleration);
  return [label(result.engine), cache, device].filter(Boolean).join(" · ");
}

/** @type {Record<string, string>} */
const STATUS_NAMES = {
  fugumt: "FuguMT",
  lindera: "Lindera",
  ichiran: "Ichiran",
  automatic: "Automatic",
  "built-in": "Built-in",
  none: "none",
};

/**
 * A label for every engine name the status uses, falling back to the recognition labels.
 *
 * @param {Label} label The recognition label, `engineLabel` from `model.ts`.
 * @returns {Label}
 */
export function statusLabel(label) {
  return (engine) => STATUS_NAMES[engine] ?? label(engine);
}

/**
 * Whether an address is this computer's own — the same rule as `sakatsu_core::net::is_loopback`.
 *
 * Only `localhost`, `127.0.0.0/8` and `::1`. A private-network address is another computer, so it
 * is not loopback: saying "local" about it would tell a reader their image stays on their machine
 * when it does not. Anything unparseable is not loopback either.
 *
 * @param {string} url
 */
export function isLoopback(url) {
  let host;
  try {
    host = new URL(url.trim()).hostname.toLowerCase();
  } catch {
    return false;
  }
  host = host.replace(/^\[|\]$/g, "");
  return host === "localhost" || host === "::1" || /^127(\.\d{1,3}){3}$/.test(host);
}
