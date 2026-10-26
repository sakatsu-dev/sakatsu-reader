/** @typedef {import("../model").RequestIdentity} RequestIdentity */

/**
 * One asynchronous surface: identity, busy state, error and disposal.
 *
 * Values remain presentation-specific and are committed by the caller. The controller owns the
 * permission to commit them, so a stale success, rejection or finally handler cannot mutate the
 * current surface through a forgotten comparison.
 *
 * @param {{
 *   operation: string,
 *   requests: ReturnType<import("./request-owner.js").requestOwner>,
 *   changed?: (state: { busy: boolean, error: string, disposed: boolean }) => void,
 * }} options
 */
export function operationController({ operation, requests, changed = () => {} }) {
  let generation = 0;
  let disposed = false;
  let busy = false;
  let error = "";

  function publish() {
    changed({ busy, error, disposed });
  }

  /** @param {{ generation: number, request: RequestIdentity }} ticket */
  function current(ticket) {
    return !disposed && ticket.generation === generation;
  }

  return {
    begin() {
      if (disposed) throw new Error(`${operation} controller is disposed`);
      generation += 1;
      busy = true;
      error = "";
      const request = requests.begin(operation);
      publish();
      return { generation, request };
    },
    current,
    /** @param {{ generation: number, request: RequestIdentity }} ticket @param {() => void} commit */
    accept(ticket, commit) {
      if (!current(ticket)) return false;
      commit();
      return true;
    },
    /** @param {{ generation: number, request: RequestIdentity }} ticket @param {unknown} thrown @param {(message: string) => void} [commit] */
    reject(ticket, thrown, commit) {
      if (!current(ticket)) return false;
      error = String(thrown);
      if (commit) commit(error);
      publish();
      return true;
    },
    /** @param {{ generation: number, request: RequestIdentity }} ticket */
    finish(ticket) {
      requests.complete(ticket.request);
      if (!current(ticket)) return false;
      busy = false;
      publish();
      return true;
    },
    cancel() {
      generation += 1;
      requests.cancel(operation);
      busy = false;
      error = "";
      publish();
    },
    dispose() {
      if (disposed) return;
      this.cancel();
      disposed = true;
      publish();
    },
    snapshot() {
      return { busy, error, disposed };
    },
  };
}
