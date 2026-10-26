/** @typedef {import("../model").RequestIdentity} RequestIdentity */
/** @typedef {import("../model").WorkPriority} WorkPriority */

/**
 * Native request identities owned by one webview lifetime.
 *
 * The local generation guard remains the authority over painting results. This companion tells
 * the backend which queued engine work has already lost that authority.
 *
 * @param {string} owner
 * @param {(request: RequestIdentity) => Promise<unknown>} withdraw
 */
export function requestOwner(owner, withdraw) {
  /** @type {Map<string, number>} */
  const generations = new Map();
  /** @type {Map<string, RequestIdentity>} */
  const current = new Map();
  let disposed = false;

  /** @param {string} operation */
  function cancel(operation) {
    const request = current.get(operation);
    if (!request) return;
    current.delete(operation);
    void withdraw(request).catch(() => {});
  }

  return {
    /** @param {string} operation @param {WorkPriority} [priority] */
    begin(operation, priority = "foreground") {
      if (disposed) throw new Error("request owner is disposed");
      cancel(operation);
      const generation = (generations.get(operation) ?? 0) + 1;
      generations.set(operation, generation);
      const request = { owner, operation, generation, priority };
      current.set(operation, request);
      return request;
    },
    cancel,
    /** @param {RequestIdentity} request */
    complete(request) {
      if (current.get(request.operation) === request) current.delete(request.operation);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      for (const operation of [...current.keys()]) cancel(operation);
    },
  };
}
