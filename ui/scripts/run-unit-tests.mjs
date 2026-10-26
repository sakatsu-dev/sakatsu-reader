/**
 * Run every unit test under `src/lib`, found rather than listed.
 *
 * **This exists because the list went stale silently.** `check` used to name all twenty-seven
 * files by hand, so a new `save-notice.test.mjs` simply did not run — 245 tests before adding it
 * and 245 after, with nothing on screen suggesting a file had been skipped. A roster of gates
 * that can quietly shrink is worse than no roster, because the count still looks healthy.
 *
 * `node --test src/lib` would also discover them, and is not used here for two reasons this
 * script exists to supply:
 *
 *   - **An empty selection must fail.** A typo in a path, a renamed directory, or a `.test.mjs`
 *     suffix convention that drifts would otherwise make the command succeed instantly while
 *     testing nothing, which is the same failure in a new shape.
 *   - **Order must be deterministic.** Directory order is a filesystem detail and differs between
 *     machines; sorting makes one run comparable to the next and to CI's.
 *
 * The exit code is propagated, so a failing test fails `npm run check`.
 */
import { once } from "node:events";
import { spawn } from "node:child_process";
import { readdir } from "node:fs/promises";
import { relative, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const unitTests = resolve(root, "src/lib");

/**
 * Every `*.test.mjs` beneath `dir`, at any depth.
 *
 * A directory that is not there yields nothing rather than throwing, so that a moved or renamed
 * `src/lib` lands on the empty-selection guard below — which says what went wrong — instead of an
 * ENOENT stack trace. Both are failures; only one of them is readable.
 */
async function discover(dir) {
  const found = [];
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch (thrown) {
    if (thrown?.code === "ENOENT") return found;
    throw thrown;
  }
  for (const entry of entries) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) found.push(...(await discover(path)));
    else if (entry.name.endsWith(".test.mjs")) found.push(path);
  }
  return found;
}

const files = (await discover(unitTests)).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

if (files.length === 0) {
  // Loud, and naming the directory it looked in: "0 tests, all passing" is the exact answer this
  // script was written to stop anything from giving.
  console.error(`No *.test.mjs files under ${relative(root, unitTests)}. Nothing was tested.`);
  process.exit(1);
}

const tests = spawn(process.execPath, ["--test", ...process.argv.slice(2), ...files], {
  cwd: root,
  windowsHide: true,
  stdio: "inherit",
});
const [code] = await once(tests, "exit");
process.exitCode = typeof code === "number" ? code : 1;
