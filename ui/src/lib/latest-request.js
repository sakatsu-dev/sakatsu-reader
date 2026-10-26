/**
 * A generation token for asynchronous work whose result replaces one UI surface.
 *
 * Starting newer work makes every older token stale. `cancel` does the same without
 * starting a replacement, which is what clearing a passage or selecting another word needs.
 * The operation still runs to completion in the backend; it simply loses permission to write
 * into a surface that now belongs to something else.
 */
export function latestRequest() {
  let generation = 0;

  return {
    begin() {
      generation += 1;
      return generation;
    },
    cancel() {
      generation += 1;
    },
    /** @param {number} candidate */
    isCurrent(candidate) {
      return candidate === generation;
    },
  };
}
