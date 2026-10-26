"""Collect every third-party licence the built app has to carry, into one notices file.

# Why this exists

The About page names what 屠つ is built on, but naming is not what the licences ask for. MIT wants
its copyright notice *and* its permission notice to travel with the software; Apache-2.0 §4 wants
the licence itself and any NOTICE file the project ships. A page that prints "Serde — MIT OR
Apache-2.0" carries neither, and until this existed nothing in the installer did.

So this walks what actually ships, reads each package's own licence text off disk, and writes a
single file that goes into the bundle. It is a build input, not documentation: the output is
checked in and `core/tests/notices_cover_dependencies.rs` fails if a dependency appears that the
file does not name.

# What counts as shipping

`cargo tree --edges normal` for the desktop binary, which excludes dev-dependencies and
build-dependencies -- neither is distributed. Plus the npm packages whose code is in the bundle
the webview loads; the toolchain that builds it (Vite, TypeScript, svelte-check) is not.

The licence *text* is read from the package as published rather than reconstructed from its SPDX
id, because MIT without its copyright line is not MIT and there is no way to guess the line. Where
a package ships no licence file at all, that is recorded as such rather than papered over.

    python scripts/collect-licences.py

Writes `src-tauri/THIRD-PARTY-NOTICES.txt`.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "THIRD-PARTY-NOTICES.txt"

LICENCE_FILES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")
RULE = "=" * 78
THIN = "-" * 78


def shipped_crates():
    """The crates linked into the binary: normal edges only, no dev, no build."""
    out = subprocess.run(
        [
            "cargo", "tree", "-p", "sakatsu-desktop",
            "--edges", "normal", "--prefix", "none", "--format", "{p}",
        ],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout
    names = set()
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        # "name v1.2.3 (/path)" or "name v1.2.3 (proc-macro)"
        m = re.match(r"^(\S+) v(\S+)", line)
        if m:
            names.add((m.group(1), m.group(2)))
    return names


def crate_metadata():
    """Where each package lives on disk, and what it declares."""
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--all-features"],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True,
        ).stdout
    )
    by_key = {}
    for pkg in meta["packages"]:
        by_key[(pkg["name"], pkg["version"])] = {
            "name": pkg["name"],
            "version": pkg["version"],
            "spdx": pkg.get("license") or "",
            "license_file": pkg.get("license_file"),
            "repository": pkg.get("repository") or pkg.get("homepage") or "",
            "authors": pkg.get("authors") or [],
            "dir": Path(pkg["manifest_path"]).parent,
        }
    return by_key


def normalise(text):
    """A key for deciding two licence texts are the same one.

    Whitespace only -- trailing spaces and blank-line runs differ between packages that ship
    byte-for-byte the same Apache-2.0, and treating those as different texts printed it four
    times. Copyright lines are *not* normalised away: two MIT files naming different holders are
    genuinely different notices and have to stay apart.
    """
    lines = [line.rstrip() for line in text.strip().splitlines()]
    out, blank = [], False
    for line in lines:
        if line:
            out.append(line)
            blank = False
        elif not blank:
            out.append("")
            blank = True
    return "\n".join(out)


def read_licences(entry):
    """Every licence-ish file a package ships, in a stable order."""
    found = []
    directory = entry["dir"]
    if not directory.is_dir():
        return found
    for path in sorted(directory.iterdir()):
        if not path.is_file():
            continue
        upper = path.name.upper()
        if not upper.startswith(LICENCE_FILES):
            continue
        if path.suffix.lower() in (".rs", ".toml", ".json"):
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace").strip()
        except OSError:
            continue
        if text:
            found.append((path.name, text))
    # A `license-file = "..."` in Cargo.toml that is not named like one of the above.
    named = entry.get("license_file")
    if named and not found:
        path = directory / named
        if path.is_file():
            text = path.read_text(encoding="utf-8", errors="replace").strip()
            if text:
                found.append((named, text))
    return found


def npm_packages():
    """The runtime half of the interface -- what is inside the bundle the webview loads."""
    wanted = []
    ui = ROOT / "ui"
    manifest = json.loads((ui / "package.json").read_text(encoding="utf-8"))
    for name in sorted(manifest.get("dependencies", {})):
        wanted.append(name)
    # Svelte is a devDependency by convention because it compiles away -- but its runtime is
    # linked into the built bundle, so it ships and has to be credited like anything else.
    for name in ("svelte",):
        if name not in wanted:
            wanted.append(name)

    out = []
    for name in wanted:
        directory = ui / "node_modules" / Path(*name.split("/"))
        pkg_json = directory / "package.json"
        if not pkg_json.is_file():
            print(f"  ! {name}: not installed, skipped", file=sys.stderr)
            continue
        info = json.loads(pkg_json.read_text(encoding="utf-8"))
        entry = {
            "name": name,
            "version": info.get("version", ""),
            "spdx": info.get("license") or "",
            "license_file": None,
            "repository": (info.get("repository") or {}).get("url", "")
            if isinstance(info.get("repository"), dict)
            else (info.get("repository") or ""),
            "authors": [],
            "dir": directory,
        }
        entry["licences"] = read_licences(entry)
        out.append(entry)
    return out


def main():
    shipped = shipped_crates()
    meta = crate_metadata()
    ours = {"sakatsu-core", "sakatsu-desktop"}

    entries = []
    for key in sorted(shipped, key=lambda k: (k[0].lower(), k[1])):
        if key[0] in ours:
            continue
        entry = meta.get(key)
        if entry is None:
            print(f"  ! {key[0]} {key[1]}: no metadata, skipped", file=sys.stderr)
            continue
        entry["licences"] = read_licences(entry)
        entries.append(entry)

    node = npm_packages()

    missing = [e for e in entries + node if not e["licences"]]
    # `[workspace.package]` in the root manifest -- the crates all say `version.workspace = true`.
    version = re.search(
        r'^version = "([^"]+)"',
        (ROOT / "Cargo.toml").read_text(encoding="utf-8").split("[workspace.package]", 1)[-1],
        re.M,
    )

    lines = []
    add = lines.append
    add("THIRD-PARTY NOTICES")
    add(f"屠つ (Sakatsu Reader) {version.group(1) if version else ''}".rstrip())
    add("")
    add("This application is built on the open-source software listed below. Each entry is")
    add("reproduced with the licence text its authors published, which is what those licences")
    add("ask for -- naming a project is not the same as carrying its terms.")
    add("")
    add("Generated by scripts/collect-licences.py from the packages that are actually linked")
    add("into the shipped binary and into the interface bundle. Build-time and test-only")
    add("dependencies are not here, because they are not distributed.")
    add("")
    add(f"{len(entries)} Rust packages, {len(node)} JavaScript packages.")
    add("")
    add("The data 屠つ can download -- dictionaries, recognition models, frequency lists -- is")
    add("not covered here. None of it is bundled; each carries its own licence, shown before the")
    add("download and again on the About page under 'Dictionaries, models and data'.")
    add("")

    add(RULE)
    add("CONTENTS")
    add(RULE)
    for entry in entries + node:
        add(f"  {entry['name']} {entry['version']}".rstrip() + (f"  [{entry['spdx']}]" if entry["spdx"] else ""))
    add("")

    # **Each distinct licence text once, with the packages it covers named above it.**
    #
    # Verbatim per package would be 3.4 MB, because ninety of them ship the same Apache-2.0
    # text. Nothing is lost by printing it once and saying who it belongs to: the licences ask
    # for their terms to accompany the software, not for a copy per package. Grouping is by the
    # text itself -- including its copyright lines, so two MIT files naming different holders
    # stay separate, which is the part that actually matters.
    grouped = {}
    for entry in entries + node:
        for filename, text in entry["licences"]:
            grouped.setdefault(normalise(text), (text, []))[1].append(
                f"{entry['name']} {entry['version']}".rstrip()
            )

    add("")
    add(RULE)
    add("LICENCE TEXTS")
    add(RULE)
    add("")
    add("Each text below is reproduced once, preceded by the packages it applies to. Where")
    add("several packages publish the same terms, they share an entry; where they publish the")
    add("same licence with different copyright holders, the entries are separate, because the")
    add("copyright line is part of what has to travel.")
    for text, covered in sorted(grouped.values(), key=lambda pair: (-len(pair[1]), pair[0][:40])):
        add("")
        add(RULE)
        add(f"Applies to {len(covered)} package{'' if len(covered) == 1 else 's'}:")
        for name in sorted(set(covered)):
            add(f"  {name}")
        add(THIN)
        add("")
        add(text)
        add("")

    if missing:
        add("")
        add(RULE)
        add("PACKAGES THAT PUBLISH NO LICENCE FILE")
        add(RULE)
        add("")
        add("These ship no licence text of their own. Their terms are the SPDX expression each")
        add("declares in its manifest, and the canonical text of that licence applies. Listed")
        add("rather than omitted, so the absence is on the record instead of being invisible.")
        for entry in sorted(missing, key=lambda e: e["name"].lower()):
            add("")
            add(f"  {entry['name']} {entry['version']}".rstrip())
            add(f"    SPDX: {entry['spdx'] or 'not declared'}")
            if entry["repository"]:
                add(f"    Source: {entry['repository']}")
            if entry["authors"]:
                add("    Authors: " + "; ".join(entry["authors"]))
        add("")

    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    size = OUT.stat().st_size
    print(f"wrote {OUT.relative_to(ROOT)}  ({size / 1024:.0f} kB)")
    print(f"  {len(entries)} Rust packages, {len(node)} JavaScript packages")
    if missing:
        print(f"  {len(missing)} shipped no licence file: " + ", ".join(e["name"] for e in missing[:12]))


if __name__ == "__main__":
    main()
