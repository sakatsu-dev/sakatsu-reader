/**
 * Downloading the pieces the installer cannot carry.
 *
 * Shared by the first-run screen and the setup panel, which offer the same work in two
 * places — one for someone who has just installed the app, one for whoever comes back for
 * the OCR model later. Two copies of "is this downloading, and how far along" would drift
 * the moment one grew a state the other did not.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CatalogEntry, FetchProgress } from "../model";

// The adapter and setup scripts remain in the tree while the VLM experiment is paused, but
// an unavailable reader choice must not still be advertised from a different setup screen.
// Nothing is hidden now. `paddle-vl` was, while the reader it installs was paused; that
// ended when the corpus scored it best of anything here. Kept as a set rather than deleted
// because hiding a component is a thing this screen may need to do again.
const HIDDEN_COMPONENTS = new Set<string>([]);

/** What is happening to one component right now. */
export type Job = {
  /** The file or the step, as the backend describes it. */
  what: string;
  done: number;
  /** Null while a step has no byte count — importing, indexing — or has no length. */
  total: number | null;
  step: number;
  steps: number;
};

class Fetching {
  /** Everything on offer, refreshed after each fetch so `ready` stays true. */
  entries = $state<CatalogEntry[]>([]);
  /** Component id → progress, for those in flight. */
  jobs = $state<Record<string, Job>>({});
  /** Component id → what went wrong, kept until the next attempt. */
  failures = $state<Record<string, string>>({});
  loaded = $state(false);
  /**
   * What has been fetched this session and is therefore **on disk but not yet in use**.
   *
   * The open dictionaries and both lookup tables are chosen at startup and deliberately
   * not swapped mid-session — `import_dictionary` records why. So a successful fetch
   * leaves the app in a state where `catalog` truthfully says "installed" and a lookup
   * still finds nothing, and the only thing that reconciles them is a restart. The
   * existing import path already says this; the setup screens must too, or the reward for
   * waiting on 95 MB is "No definition found" with no explanation.
   */
  fetched = $state<string[]>([]);

  readonly busy = $derived(Object.keys(this.jobs).length > 0);

  constructor() {
    listen<FetchProgress>("fetch-progress", (event) => {
      const { id, file, done, total, step, steps } = event.payload;
      this.jobs[id] = {
        what: file,
        done,
        total,
        step: step ?? 1,
        steps: steps ?? 1,
      };
    });

    listen<{ id: string; ok: boolean; detail: string }>("fetch-done", (event) => {
      const { id, ok, detail } = event.payload;
      delete this.jobs[id];
      if (ok) {
        delete this.failures[id];
        if (!this.fetched.includes(id)) this.fetched.push(id);
      } else {
        this.failures[id] = detail;
      }
      // Ask rather than assume: what is installed is a fact about the disk, and the
      // backend has just changed it.
      this.refresh();
    });
  }

  async refresh() {
    try {
      const entries = await invoke<CatalogEntry[]>("catalog");
      this.entries = entries.filter((entry) => !HIDDEN_COMPONENTS.has(entry.id));
    } catch {
      this.entries = [];
    } finally {
      this.loaded = true;
    }
  }

  /** Start a fetch, unless this session is already tracking that component. */
  start(id: string) {
    if (id in this.jobs) return;
    delete this.failures[id];
    this.jobs[id] = { what: "starting…", done: 0, total: null, step: 1, steps: 1 };
    invoke("fetch_component", { id }).catch((thrown) => {
      delete this.jobs[id];
      this.failures[id] = String(thrown);
    });
  }

  /** Start several, one after another rather than at once. */
  async startAll(ids: string[]) {
    for (const id of ids) {
      this.start(id);
      // Serially: three downloads sharing a connection all finish later than three taken
      // in turn, and a progress bar per file is only legible one at a time.
      await this.settled(id);
    }
  }

  /**
   * Resolves when nothing is in flight for `id`, or after `STUCK_MS` regardless.
   *
   * The backend now always reports, so the timeout should never fire. It is here because
   * of what happened when one path did not: `busy` is bound to `disabled` on every Get
   * button in both screens, so a single missed event left the entire setup interface
   * inert with nothing on screen to explain it and no way out but a restart. A stuck job
   * should cost one component, not all of them.
   */
  private settled(id: string): Promise<void> {
    const STUCK_MS = 30 * 60 * 1000;
    const started = Date.now();
    return new Promise((resolve) => {
      const tick = () => {
        if (!(id in this.jobs)) return resolve();
        if (Date.now() - started > STUCK_MS) {
          this.cancel(id);
          delete this.jobs[id];
          this.failures[id] = "gave up waiting — nothing was reported";
          return resolve();
        }
        setTimeout(tick, 150);
      };
      tick();
    });
  }

  cancel(id: string) {
    invoke("cancel_fetch", { id }).catch(() => {});
  }

  entry(id: string) {
    return this.entries.find((candidate) => candidate.id === id);
  }

  /** Those in a tier, in catalog order. */
  tier(name: CatalogEntry["tier"]) {
    return this.entries.filter((entry) => entry.tier === name);
  }

  /** The one-click group, and what it costs — only counting what is not already here. */
  get essentials() {
    const missing = this.tier("essential").filter((entry) => !entry.ready);
    const bytes = missing.reduce((total, entry) => total + entry.bytes, 0);
    return { missing, bytes };
  }
}

export const fetching = new Fetching();

/** Bytes as something a person can judge. Mirrors `setup::catalog::human_size`. */
export function humanSize(bytes: number): string {
  const MB = 1024 * 1024;
  if (bytes >= 1024 * MB) return `${(bytes / (1024 * MB)).toFixed(1)} GB`;
  return `${Math.round(bytes / MB)} MB`;
}

/** A job as a percentage, or null when the total is unknown. */
export function share(job: Job): number | null {
  if (!job.total || job.total <= 0) return null;
  return Math.min(100, Math.round((job.done / job.total) * 100));
}
