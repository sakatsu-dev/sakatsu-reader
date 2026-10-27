import { readdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export async function verifyQaBoundary(directory, mode) {
  if (!["layout", "production"].includes(mode)) throw new Error("Unknown build mode");
  const assets = resolve(directory, "assets");
  const scripts = (await readdir(assets)).filter((name) => name.endsWith(".js"));
  if (!scripts.length) throw new Error("Build contains no JavaScript assets");
  const qa = (
    await Promise.all(scripts.map((name) => readFile(resolve(assets, name), "utf8")))
  ).some((source) => source.includes("__SAKATSU_QA__"));
  if (qa !== (mode === "layout")) {
    throw new Error(
      mode === "layout"
        ? "Layout build is missing QA fixtures"
        : "QA fixtures leaked into production",
    );
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await verifyQaBoundary(resolve(process.argv[2]), process.argv[3]);
}
