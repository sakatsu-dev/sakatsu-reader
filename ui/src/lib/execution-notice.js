/**
 * What the reader is told about **which analyzer read the passage**, from its execution record.
 *
 * Provenance is always recorded by core; this decides only how loudly to say it:
 *
 * | the record | shown |
 * |---|---|
 * | no fallback | nothing |
 * | Ichiran failed on *this passage* and the built-in analyzer retried it | a prominent notice, under any policy |
 * | Ichiran is still starting | nothing — it is a moment at launch, not something to act on |
 * | no Ichiran this session, and it was asked for **by name** | a prominent notice |
 * | no Ichiran this session, under **Automatic** | a quiet label only — the reader chose "whichever answers" |
 *
 * The technical account is kept for an expandable detail and never replaces the sentence.
 */

/**
 * @typedef {{ reason: string; scope: string; detail: string }} FallbackLike
 * @typedef {{ requested: string; engine: string; fallback: FallbackLike | null }} ExecutionLike
 * @typedef {{ tone: "prominent" | "quiet"; text: string; detail: string }} Notice
 */

/**
 * @param {ExecutionLike | null | undefined} execution
 * @returns {Notice | null}
 */
export function noticeFor(execution) {
  const fallback = execution?.fallback;
  if (!execution || !fallback) return null;
  if (fallback.scope === "passage") {
    return {
      tone: "prominent",
      text: "Using Lindera because Ichiran could not analyze this passage.",
      detail: fallback.detail,
    };
  }
  // The stand-in while Ichiran starts is still recorded on the passage; it is not announced.
  if (fallback.reason === "starting") return null;
  if (execution.requested === "ichiran") {
    return {
      tone: "prominent",
      text: "Ichiran is unavailable, so the built-in analyzer read this passage.",
      detail: fallback.detail,
    };
  }
  return { tone: "quiet", text: "Built-in analyzer", detail: fallback.detail };
}
