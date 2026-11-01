"""Prepare the pinned app-local WebView2 runtime; network access requires --download."""

import argparse
import hashlib
import json
import os
import re
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit
from urllib.request import build_opener, HTTPRedirectHandler

ROOT = Path(__file__).resolve().parent.parent


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


class HttpsRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        if urlsplit(newurl).scheme != "https":
            raise ValueError("Runtime download redirected away from HTTPS")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def specification(root=ROOT):
    spec = json.loads((root / "scripts/webview2-runtime.json").read_text(encoding="utf-8"))
    file = root / "scripts/webview2-runtime-files.json"
    # Git checkouts may use CRLF on Windows. Bind the inventory's canonical LF bytes;
    # archive and executable checksums always cover their original binary bytes.
    if hashlib.sha256(file.read_bytes().replace(b"\r\n", b"\n")).hexdigest() != spec["files_manifest_sha256"]:
        raise ValueError("Runtime file inventory digest disagrees")
    files = json.loads(file.read_text(encoding="utf-8"))
    names = [r["file"] for r in files]
    if (not re.fullmatch(r"\d+\.\d+\.\d+\.\d+", spec["version"])
            or any(not re.fullmatch(r"[0-9a-f]{64}", spec[k]) for k in ("sha256", "executable_sha256", "files_manifest_sha256"))
            or not isinstance(spec["bytes"], int) or spec["bytes"] <= 0
            or any(not isinstance(f["bytes"], int) or f["bytes"] < 0 or not re.fullmatch(r"[0-9a-f]{64}", f["sha256"]) for f in files)
            or not files or len({n.casefold() for n in names}) != len(names)
            or any(PurePosixPath(n).is_absolute() or ".." in PurePosixPath(n).parts or ":" in n or "\\" in n for n in names)
            or sum(f["bytes"] for f in files) > spec["max_unpacked_bytes"]
            or spec["directory"] != f"Microsoft.WebView2.FixedVersionRuntime.{spec['version']}.x64"
            or urlsplit(spec["url"]).scheme != "https"):
        raise ValueError("Invalid pinned runtime inventory")
    return spec, files


def verify_directory(directory, spec, files):
    directory = Path(directory)
    if not directory.is_dir() or directory.is_symlink():
        return False
    actual = [p for p in directory.rglob("*") if p.is_file() or p.is_symlink()]
    if any(p.is_symlink() for p in actual) or {p.relative_to(directory).as_posix() for p in actual} != {r["file"] for r in files}:
        return False
    for row in files:
        path = directory / row["file"]
        if path.stat().st_size != row["bytes"] or digest(path) != row["sha256"]:
            return False
    return digest(directory / "msedgewebview2.exe") == spec["executable_sha256"]


def archive(spec, cache, download=False, opener=None):
    cache = Path(cache)
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / (spec["sha256"] + ".cab")
    if path.exists():
        if path.stat().st_size != spec["bytes"] or digest(path) != spec["sha256"]:
            raise ValueError("Cached runtime archive is corrupt")
        return path
    if not download:
        raise ValueError("Pinned runtime archive is missing; use --download explicitly")
    opener = opener or build_opener(HttpsRedirect()).open
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=cache, suffix=".partial", delete=False) as output:
            temporary = Path(output.name)
            with opener(spec["url"], timeout=120) as response:
                if urlsplit(response.geturl()).scheme != "https":
                    raise ValueError("Runtime download is not HTTPS")
                size = 0
                while chunk := response.read(1024 * 1024):
                    size += len(chunk)
                    if size > spec["bytes"]:
                        raise ValueError("Runtime download exceeded its pinned size")
                    output.write(chunk)
        if temporary.stat().st_size != spec["bytes"] or digest(temporary) != spec["sha256"]:
            raise ValueError("Runtime archive checksum or byte length disagrees")
        os.replace(temporary, path)
        return path
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def extracted_runtime(expanded, spec, files):
    expanded = Path(expanded)
    if verify_directory(expanded, spec, files):
        return expanded
    # Microsoft's CAB has one versioned wrapper. Accept only that exact wrapper,
    # with no siblings, and still verify every enclosed filename and binary byte.
    children = list(expanded.iterdir())
    wrapped = expanded / spec["directory"]
    if children == [wrapped] and verify_directory(wrapped, spec, files):
        return wrapped
    raise ValueError("Extracted runtime does not match the pinned file inventory or its sole versioned wrapper")


def prepare(destination, cache, download=False, root=ROOT):
    spec, files = specification(root)
    destination = Path(destination)
    if destination.name != spec["directory"]:
        raise ValueError("Runtime destination must use the pinned directory name")
    if destination.exists():
        if verify_directory(destination, spec, files):
            return destination
        raise ValueError("Existing runtime is incomplete or different; it was left untouched")
    cab = archive(spec, cache, download)
    if os.name != "nt":
        raise ValueError("Runtime extraction requires Windows expand.exe")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent, prefix="runtime-prepare-") as temporary:
        staged = Path(temporary)
        staged = staged / "runtime"
        staged.mkdir()
        subprocess.run(["expand.exe", "-F:*", str(cab.resolve()), str(staged.resolve())],
                       check=True, stdout=subprocess.DEVNULL)
        staged = extracted_runtime(staged, spec, files)
        # Publication is one rename after complete validation, never a partially populated runtime.
        staged.rename(destination)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", required=True, type=Path)
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--download", action="store_true")
    args = parser.parse_args()
    path = prepare(args.destination, args.cache, args.download)
    print(f"Verified app-local WebView2: {path.name}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f"Runtime preparation refused: {error}", file=sys.stderr)
        sys.exit(1)
