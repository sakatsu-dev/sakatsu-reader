import { once } from "node:events";
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const vite = resolve(root, "node_modules/vite/bin/vite.js");
const playwright = resolve(root, "node_modules/@playwright/test/cli.js");
let serverLog = "";

const server = spawn(process.execPath, [vite, "--host", "127.0.0.1"], {
  cwd: root,
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
});
server.stdout.on("data", (chunk) => (serverLog += String(chunk)));
server.stderr.on("data", (chunk) => (serverLog += String(chunk)));

async function waitForServer() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (server.exitCode !== null) throw new Error(`Vite exited early.\n${serverLog}`);
    try {
      const response = await fetch("http://127.0.0.1:1420");
      if (response.ok) return;
    } catch {
      // The listener is not ready yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Vite did not become ready.\n${serverLog}`);
}

let exitCode = 1;
try {
  await waitForServer();
  const tests = spawn(process.execPath, [playwright, "test", ...process.argv.slice(2)], {
    cwd: root,
    windowsHide: true,
    stdio: "inherit",
  });
  const [code] = await once(tests, "exit");
  exitCode = typeof code === "number" ? code : 1;
} finally {
  // Vite itself is the direct Node child, so terminating this exact PID tears down only the
  // server this run created. This avoids the orphaned npm child that held Windows CI open.
  if (server.exitCode === null) server.kill();
  if (server.exitCode === null) {
    await Promise.race([once(server, "exit"), new Promise((resolve) => setTimeout(resolve, 5000))]);
  }
}

process.exitCode = exitCode;
