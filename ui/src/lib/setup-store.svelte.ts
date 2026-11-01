import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { latestRequest } from "./latest-request.js";
import { listenerGroup } from "./listener-group.js";

import type { SetupPhase, SetupComponentState, SetupArtifactState, StorageSummary, SetupSnapshot, SetupPreflight, SetupProgress } from "../bindings";
export type { SetupPhase, SetupComponentState, SetupArtifactState, StorageSummary, SetupSnapshot, SetupPreflight, SetupProgress } from "../bindings";

class SetupStore {
  snapshot = $state<SetupSnapshot | null>(null);
  preflight = $state<SetupPreflight | null>(null);
  progress = $state<SetupProgress | null>(null);
  loaded = $state(false);
  checking = $state(false);
  error = $state("");
  private listening = false;
  private listeners = listenerGroup();
  private refreshRequest = latestRequest();

  async initialise() {
    if (!this.listening) {
      this.listening = true;
      await Promise.all([
        this.listeners.add(() => listen("setup-snapshot-changed", () => this.refresh())),
        // Optional downloads use fetch_component rather than the setup worker. Their
        // completion (including pause) changes the same on-disk resumability snapshot.
        this.listeners.add(() => listen("fetch-done", () => this.refresh())),
        this.listeners.add(() =>
          listen<SetupProgress>("setup-progress", (event) => {
            if (
              this.snapshot &&
              event.payload.generation === this.snapshot.generation
            ) {
              this.progress = event.payload;
            }
          }),
        ),
      ]);
    }
    await this.refresh();
  }

  async refresh() {
    const request = this.refreshRequest.begin();
    this.checking = true;
    try {
      const snapshot = await invoke<SetupSnapshot>("setup_snapshot");
      if (!this.refreshRequest.isCurrent(request)) return;
      this.snapshot = snapshot;
      this.error = "";
      if (this.snapshot.required_ready) this.progress = null;
    } catch (thrown) {
      if (this.refreshRequest.isCurrent(request)) this.error = String(thrown);
    } finally {
      if (this.refreshRequest.isCurrent(request)) {
        this.loaded = true;
        this.checking = false;
      }
    }
  }

  dispose() {
    this.refreshRequest.cancel();
    this.listeners.dispose();
    this.listening = false;
    this.checking = false;
  }

  async checkSystem() {
    this.checking = true;
    try {
      this.preflight = await invoke<SetupPreflight>("setup_preflight");
      this.error = "";
    } catch (thrown) {
      this.error = String(thrown);
    } finally {
      this.checking = false;
    }
  }

  async start(downloadRepair = false) {
    this.error = "";
    try {
      await invoke<boolean>("setup_start", { downloadRepair });
      await this.refresh();
    } catch (thrown) {
      this.error = String(thrown);
    }
  }

  async pause() {
    await invoke("setup_pause");
    await this.refresh();
  }

  async useTextOnly() {
    await invoke("setup_use_text_only");
    await this.refresh();
  }

  async repair() {
    await invoke("setup_repair");
    await this.refresh();
    if (!this.snapshot?.required_ready) await this.start();
  }

  async chooseStorage(path: string) {
    await invoke("setup_choose_storage", { path });
    await this.checkSystem();
    await this.refresh();
  }

  async moveResources(target: string, removeOld: boolean) {
    const detail = await invoke<string>("setup_move_resources", { target, removeOld });
    await this.refresh();
    return detail;
  }

  async restartArtifact(componentId: string) {
    await invoke("setup_restart_artifact", { componentId });
    await this.refresh();
  }

  async installOffline(packagePath: string) {
    await invoke<boolean>("setup_import_offline", { packagePath });
    await this.refresh();
  }
}

export const setup = new SetupStore();

export function humanBytes(bytes: number | null) {
  if (bytes === null) return "unknown";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

export function duration(seconds: number | null) {
  if (seconds === null) return "estimating…";
  if (seconds < 60) return `${Math.max(1, Math.round(seconds))} sec`;
  if (seconds < 3600) return `${Math.ceil(seconds / 60)} min`;
  return `${(seconds / 3600).toFixed(1)} hr`;
}
