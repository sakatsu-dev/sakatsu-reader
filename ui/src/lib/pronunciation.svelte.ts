/**
 * Which words have a recording, so a button is only drawn where pressing it will do something.
 *
 * ## The endpoint cannot be asked politely, but it can be asked cheaply
 *
 * Asking for a word it has never recorded returns **HTTP 200 with a fixed placeholder MP3**, not
 * a 404 — `core/src/audio/mod.rs` recognises that clip by md5. So availability is not something
 * the reply status can answer.
 *
 * A `HEAD` can, and nearly for free: the endpoint answers one with a `Content-Length`, and the
 * placeholder is a single fixed file, so **any other length is a real recording**. Only a length
 * matching the placeholder exactly is ambiguous, and `WordAudio::available` settles that one case
 * by fetching and checking the digest. Checking a word therefore costs headers rather than the
 * ~52 KB a full fetch would, which is what makes checking every displayed word reasonable at all.
 *
 * ## Asked once per word, ever
 *
 * Every answer is written to `audio-availability.json` in the store, hits as well as misses.
 * Remembering only the misses would mean re-asking about every word that *does* have audio on
 * every run, which is most of them. `load()` reads the file at startup, so a second run of the
 * same search sends no requests at all.
 *
 * The cache is deliberately not precious: an unreadable file is simply empty and refills itself,
 * where a broken `words.json` is set aside and reported. Losing it costs some requests, not work.
 *
 * ## Batched, because a search is eighteen words at once
 *
 * Components call `want()` as they render and never wait for it. The calls are collected and sent
 * as one request on the next tick, so a page of results is a single round trip rather than one per
 * card. Rust probes the unknown ones a few at a time.
 *
 * Shared module state rather than props threaded from `App`, following `fetching.svelte`: the two
 * pages that draw pronunciation controls would otherwise both carry a list neither of them owns.
 */
import { invoke } from "@tauri-apps/api/core";

/** `sakatsu_core::store::WordKey`. */
type WordKey = { term: string; reading: string };
/** `sakatsu_core::store::AudioRecord`. */
type AudioRecord = { key: WordKey; available: boolean };

function id(expression: string, reading: string): string {
  return JSON.stringify([expression, reading]);
}

class Pronunciation {
  /** What is known, by `id`. Absent means "not asked yet", which is not the same as "no". */
  #known = $state<Record<string, boolean>>({});
  /** Asked for this tick, not yet sent. */
  #pending = new Set<string>();
  #scheduled = false;

  /** Read what earlier runs learned. Failure leaves it empty, which only costs requests. */
  async load() {
    try {
      const records = await invoke<AudioRecord[]>("audio_known");
      const known: Record<string, boolean> = {};
      for (const record of records) known[id(record.key.term, record.key.reading)] = record.available;
      this.#known = known;
    } catch {
      this.#known = {};
    }
  }

  /**
   * Whether to draw a button for this word.
   *
   * **False while unknown**, so nothing is offered before the answer is in. A button that
   * appears a moment later is honest; one that appears immediately and then vanishes is not.
   */
  has(expression: string, reading: string): boolean {
    return this.#known[id(expression, reading)] === true;
  }

  /**
   * Note that this word is on screen and its answer is wanted.
   *
   * Safe to call on every render: a word already known, or already queued, adds nothing.
   */
  want(expression: string, reading: string) {
    if (!expression && !reading) return;
    const key = id(expression, reading);
    if (key in this.#known || this.#pending.has(key)) return;
    this.#pending.add(key);
    this.#schedule();
  }

  /** Record an answer learned by actually playing — Rust has already written it down. */
  markSilent(expression: string, reading: string) {
    this.#known = { ...this.#known, [id(expression, reading)]: false };
  }

  #schedule() {
    if (this.#scheduled) return;
    this.#scheduled = true;
    // A tick, so one page of results becomes one request. `setTimeout` rather than a microtask
    // because the components are still rendering when the first `want` arrives, and a microtask
    // would send before the rest of the list had asked.
    setTimeout(() => this.#flush(), 0);
  }

  async #flush() {
    this.#scheduled = false;
    const asking = [...this.#pending];
    this.#pending.clear();
    if (asking.length === 0) return;

    const words: WordKey[] = asking.map((key) => {
      const [term, reading] = JSON.parse(key) as [string, string];
      return { term, reading };
    });

    try {
      const records = await invoke<AudioRecord[]>("audio_availability", { words });
      const known = { ...this.#known };
      for (const record of records) known[id(record.key.term, record.key.reading)] = record.available;
      this.#known = known;
    } catch {
      // Unreachable, most likely offline. Deliberately not recorded as "no recording": the
      // words stay unknown, so the next render asks again rather than hiding them forever.
    }
  }
}

export const pronunciation = new Pronunciation();
