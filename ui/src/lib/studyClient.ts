/**
 * The typed client for the study slice.
 *
 * Every call into the analysis and study service goes through here, so the command name and its
 * argument name are written **once** instead of at each call site. `App.svelte` had the only
 * `invoke("study")` in the tree and spelled both inline.
 *
 * # What this catches, and what it does not
 *
 * It is worth being exact, because a typed wrapper is easy to over-credit:
 *
 * - **A DTO change is caught here.** `StudyView` comes from `model.ts`, which
 *   `model.contract.ts` type-checks against `model.golden.json` — generated from the real
 *   serialized bytes. A field renamed, removed or added in Rust fails `npm run check`.
 * - **A command or argument rename in Rust is NOT caught here.** `invoke<StudyView>("study", {
 *   text })` compiles perfectly against a backend that has renamed `study` or `text`; TypeScript
 *   has no way to know what the Rust side registered. That drift is an integration concern, and
 *   `src-tauri/tests/ipc.rs` is what covers it — it invokes by string through the real
 *   `invoke_handler`, so a rename fails there. Keeping that coverage is part of this slice, not an
 *   optional extra.
 *
 * So: the wrapper buys one spelling and a checked return type. It does not buy a compile-time
 * guarantee about the wire, and saying otherwise would be the kind of claim this project keeps
 * having to take back.
 */

import { invoke } from "@tauri-apps/api/core";
import type { Analysis, RequestIdentity, StudyView } from "../model";

/** The command names this client owns, spelled once. */
const COMMANDS = {
  study: "study",
  analyze: "analyze",
} as const;

/**
 * Everything the reader needs for one passage, in one round trip.
 *
 * Rejects on the transport error string the shell produces from the service's error.
 */
export function study(text: string, request?: RequestIdentity): Promise<StudyView> {
  return invoke<StudyView>(COMMANDS.study, { text, request });
}

/**
 * The tokens alone — the analyzer and the displayed pipeline, without the derivations.
 *
 * A strict prefix of {@link study}, and that is enforced on the Rust side: both call one service,
 * so the two can no longer disagree about a reading the way they once did.
 */
export function analyze(text: string, request?: RequestIdentity): Promise<Analysis> {
  return invoke<Analysis>(COMMANDS.analyze, { text, request });
}
