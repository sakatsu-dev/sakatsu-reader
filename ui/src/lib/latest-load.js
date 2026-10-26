import { latestRequest } from "./latest-request.js";

/**
 * Load one replaceable value without allowing an older response to overwrite a newer one.
 *
 * Resource reload events can arrive while the initial request is still in flight. The request
 * that began last owns the value, regardless of which backend response happens to arrive last.
 *
 * @template T
 * @param {() => Promise<T>} load
 * @param {(value: T) => void} accept
 * @param {(error: unknown) => void} reject
 */
export function latestLoad(load, accept, reject = () => {}) {
  const requests = latestRequest();

  return {
    async run() {
      const request = requests.begin();
      try {
        const value = await load();
        if (requests.isCurrent(request)) accept(value);
      } catch (thrown) {
        if (requests.isCurrent(request)) reject(thrown);
      }
    },
    cancel() {
      requests.cancel();
    },
  };
}
