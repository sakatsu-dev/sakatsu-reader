import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, copyFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// ONNX Runtime's Windows static library still uses the dynamic Microsoft C++ standard
// library. A developer machine hides that dependency because Visual Studio installs the
// CRT globally; a clean machine does not. Stage an app-local copy so the per-user NSIS
// installer remains offline, non-elevated, and able to launch on its first attempt.
const requiredDlls = [
  "msvcp140.dll",
  "msvcp140_1.dll",
  "vcruntime140.dll",
  "vcruntime140_1.dll",
];

if (process.platform !== "win32") {
  console.log("Skipping Microsoft C++ runtime staging on a non-Windows host.");
  process.exit(0);
}

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outputDirectory = path.join(repositoryRoot, "src-tauri", "windows-runtime");
const redistRoots = [];

function addRoot(candidate) {
  if (candidate && existsSync(candidate) && !redistRoots.includes(candidate)) {
    redistRoots.push(candidate);
  }
}

addRoot(process.env.VCToolsRedistDir);
if (process.env.VSINSTALLDIR) {
  addRoot(path.join(process.env.VSINSTALLDIR, "VC", "Redist", "MSVC"));
}

const programFilesX86 = process.env["ProgramFiles(x86)"];
const vswhere = programFilesX86
  ? path.join(programFilesX86, "Microsoft Visual Studio", "Installer", "vswhere.exe")
  : null;
if (vswhere && existsSync(vswhere)) {
  try {
    const installationPath = execFileSync(
      vswhere,
      ["-latest", "-products", "*", "-property", "installationPath"],
      { encoding: "utf8" },
    ).trim();
    addRoot(path.join(installationPath, "VC", "Redist", "MSVC"));
  } catch {
    // The fixed-location probes below still cover standard Visual Studio installs.
  }
}

for (const year of ["2022", "2019"]) {
  for (const edition of ["BuildTools", "Community", "Professional", "Enterprise"]) {
    if (programFilesX86) {
      addRoot(
        path.join(
          programFilesX86,
          "Microsoft Visual Studio",
          year,
          edition,
          "VC",
          "Redist",
          "MSVC",
        ),
      );
    }
  }
}

function crtDirectories(redistRoot) {
  const versionRoots = [redistRoot];
  try {
    versionRoots.push(
      ...readdirSync(redistRoot, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .map((entry) => path.join(redistRoot, entry.name))
        .sort((left, right) => right.localeCompare(left, undefined, { numeric: true })),
    );
  } catch {
    return [];
  }

  const candidates = [];
  for (const versionRoot of versionRoots) {
    const architectureRoot = path.join(versionRoot, "x64");
    try {
      for (const entry of readdirSync(architectureRoot, { withFileTypes: true })) {
        if (entry.isDirectory() && /^Microsoft\.VC\d+\.CRT$/i.test(entry.name)) {
          candidates.push(path.join(architectureRoot, entry.name));
        }
      }
    } catch {
      // This root simply is not a complete x64 redistributable installation.
    }
  }
  return candidates;
}

const crtDirectory = redistRoots
  .flatMap(crtDirectories)
  .find((candidate) =>
    requiredDlls.every((name) => existsSync(path.join(candidate, name))),
  );

if (!crtDirectory) {
  throw new Error(
    "Could not find the x64 Microsoft Visual C++ runtime files required for packaging. " +
      "Install Visual Studio Build Tools with the latest MSVC x64 redistributable, or set " +
      "VCToolsRedistDir to its version directory.",
  );
}

mkdirSync(outputDirectory, { recursive: true });
for (const name of requiredDlls) {
  copyFileSync(path.join(crtDirectory, name), path.join(outputDirectory, name));
}

console.log(`Staged Microsoft C++ runtime from ${crtDirectory}`);
