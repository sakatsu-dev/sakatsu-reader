/**
 * Own asynchronous event subscriptions, including ones that finish after disposal.
 *
 * Tauri's `listen` returns its unsubscribe function asynchronously. A component can disappear in
 * that gap; accepting the handle afterwards leaks a listener whose owner no longer exists.
 */
export function listenerGroup() {
  let generation = 0;
  /** @type {Array<() => void>} */
  let unlisteners = [];

  return {
    /** @param {() => Promise<() => void>} subscribe */
    async add(subscribe) {
      const owner = generation;
      const unlisten = await subscribe();
      if (owner !== generation) {
        unlisten();
        return false;
      }
      unlisteners.push(unlisten);
      return true;
    },
    dispose() {
      generation += 1;
      const owned = unlisteners;
      unlisteners = [];
      for (const unlisten of owned) unlisten();
    },
  };
}
