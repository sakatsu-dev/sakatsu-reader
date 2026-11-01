/** Startup counts completed checks, not an estimated fraction of loading time. */
/** @typedef {import('../model').EngineStatus} EngineStatus */
/** @typedef {{id: string, name: string, state: 'loading'|'ready'|'deferred'|'attention', detail: string}} StartupRow */

/** @param {{settings: boolean, resources: boolean, needsSetup: boolean, settingsFailed?: boolean, resourcesFailed?: boolean, engines: EngineStatus[]|null, error?: string}} input
 * @returns {StartupRow[]}
 */
export function startupRows(input) {
  /** @param {string} id @param {string} name @param {StartupRow['state']} state @param {string} detail */
  const row = (id, name, state, detail) => ({ id, name, state, detail });
  const rows = [
    row(
      "settings",
      "Preferences",
      !input.settings ? "loading" : input.settingsFailed ? "attention" : "ready",
      !input.settings
        ? "Reading your settings"
        : input.settingsFailed
          ? "Using defaults"
          : "Loaded",
    ),
    row(
      "resources",
      "Dictionaries and models",
      !input.resources
        ? "loading"
        : input.resourcesFailed
          ? "attention"
          : input.needsSetup
            ? "attention"
            : "ready",
      !input.resources
        ? "Checking installed resources"
        : input.resourcesFailed
          ? "Status unavailable"
          : input.needsSetup
            ? "Setup needed"
            : "Available",
    ),
  ];
  for (const role of ["analysis", "recognition"]) {
    const engine = input.engines?.find((item) => item.role === role);
    const id = role;
    const name = role === "analysis" ? "Japanese parser" : "Text recognition";
    if (input.resources && input.needsSetup) {
      rows.push(row(id, name, "deferred", "Available after setup"));
    } else if (input.error) {
      rows.push(row(id, name, "attention", "Could not read engine status"));
    } else if (!engine || engine.state === "loading") {
      rows.push(
        row(
          id,
          name,
          "loading",
          role === "analysis" ? "Starting the parser" : "Loading selected OCR",
        ),
      );
    } else if (engine.state === "not_installed" || engine.loaded === "stub") {
      rows.push(row(id, name, "attention", "Setup needed"));
    } else if (engine.state === "not_loaded") {
      rows.push(
        row(
          id,
          name,
          role === "analysis" ? "deferred" : "attention",
          role === "analysis" ? "Starts on first use" : "Not loaded; capture can start it",
        ),
      );
    } else if (engine.state === "ready") {
      const detail =
        role === "analysis"
          ? engine.loaded === "ichiran"
            ? "Ichiran ready"
            : engine.configured === "built-in"
              ? "Built-in parser ready"
              : "Built-in fallback ready"
          : "Selected OCR initialized";
      rows.push(row(id, name, "ready", detail));
    } else {
      rows.push(row(id, name, "attention", "Unrecognized engine status"));
    }
  }
  return rows;
}

/** A disposed screen never accepts a late response or schedules another poll.
 * @param {() => Promise<import('../model').RuntimeStatus>} read
 * @param {(status: import('../model').RuntimeStatus|null, error: string) => void} publish
 * @param {{schedule?: typeof setTimeout, cancel?: typeof clearTimeout}} [clock]
 */
export function startupPolling(read, publish, clock = {}) {
  const schedule = clock.schedule ?? setTimeout;
  const cancel = clock.cancel ?? clearTimeout;
  let alive = true;
  let pending = false;
  /** @type {ReturnType<typeof setTimeout>|undefined} */
  let timer;
  async function refresh() {
    if (!alive || pending) return;
    if (timer !== undefined) cancel(timer);
    pending = true;
    let succeeded = false;
    try {
      const status = await read();
      if (alive) publish(status, "");
      succeeded = true;
    } catch {
      if (alive) publish(null, "Engine status is unavailable. You can retry or continue.");
    } finally {
      pending = false;
      if (alive && succeeded) timer = schedule(refresh, 400);
    }
  }
  return {
    refresh,
    dispose() {
      alive = false;
      if (timer !== undefined) cancel(timer);
    },
  };
}
