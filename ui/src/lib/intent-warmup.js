/**
 * Debounce an invisible, best-effort preparation request.
 *
 * This owns no result state: the ordinary foreground action remains authoritative. Cancelling
 * prevents work that has not started; a request already in Rust is made harmless by the backend's
 * newest-intent and exact-request gates.
 *
 * @template T
 * @param {number} delay
 * @param {(value: T) => void} prepare
 */
export function delayedWarmup(delay, prepare) {
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let timer;
  return {
    /** @param {T} value */
    schedule(value) {
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        prepare(value);
      }, delay);
    },
    cancel() {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
  };
}

/**
 * Nearby selectable tokens, forward first, never including the visible selection.
 *
 * @param {number[]} words selectable token indices in reading order
 * @param {number} selected selected token index
 * @param {number} [radius]
 */
export function neighboringWordIndices(words, selected, radius = 2) {
  const at = words.indexOf(selected);
  if (at < 0 || radius <= 0) return [];
  const nearby = [];
  for (let distance = 1; distance <= radius; distance += 1) {
    if (at + distance < words.length) nearby.push(words[at + distance]);
    if (at - distance >= 0) nearby.push(words[at - distance]);
  }
  return nearby;
}
