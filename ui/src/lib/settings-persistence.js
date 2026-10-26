/**
 * Ordered settings persistence, including the restart barrier.
 *
 * @template T
 * @param {{
 *   read: () => T,
 *   signature: () => string,
 *   save: (settings: T) => Promise<T>,
 *   apply: (settings: T) => void,
 *   failed: (error: unknown) => void,
 *   restart: () => Promise<unknown>,
 *   delay?: number,
 * }} options
 */
export function settingsPersistence(options) {
  let lastPersisted = "";
  let generation = 0;
  let ready = false;
  let disposed = false;
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let timer;
  let queue = Promise.resolve();

  /** @param {string} expected */
  function persist(expected) {
    const settings = options.read();
    const mine = (generation += 1);
    const request = queue
      .catch(() => {})
      .then(() => options.save(settings))
      .then((kept) => {
        if (disposed) return;
        if (mine === generation && options.signature() === expected) {
          options.apply(kept);
          lastPersisted = options.signature();
        } else {
          // Disk holds a correctly ordered older snapshot. A later scheduled save owns bringing it
          // forward, so this snapshot must not be called current.
          lastPersisted = "";
        }
      })
      .catch((error) => {
        if (!disposed && mine === generation) options.failed(error);
        throw error;
      });
    queue = request;
    return request;
  }

  /** @param {string} signature */
  function loaded(signature) {
    ready = true;
    lastPersisted = signature;
  }

  /** @param {string} signature */
  function schedule(signature) {
    if (!ready || disposed || signature === lastPersisted) return;
    clearTimeout(timer);
    timer = setTimeout(() => void persist(signature).catch(() => {}), options.delay ?? 400);
  }

  return {
    loaded,
    schedule,
    persist,
    /** @template R
     * @param {() => Promise<R>} action
     * @returns {Promise<R>}
     */
    async beforeExit(action) {
      clearTimeout(timer);
      timer = undefined;
      try {
        await queue;
      } catch {
        // The current signature below decides whether the failed write needs a retry.
      }
      const current = options.signature();
      if (ready && current !== lastPersisted) await persist(current);
      return await action();
    },
    async restart() {
      await this.beforeExit(options.restart);
    },
    dispose() {
      disposed = true;
      clearTimeout(timer);
      timer = undefined;
    },
    snapshot() {
      return { ready, lastPersisted, generation, disposed };
    },
  };
}
