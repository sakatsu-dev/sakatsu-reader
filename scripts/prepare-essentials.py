"""Prepare the one installer's pinned essentials without adding data to Git."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SNAPSHOT = ROOT / "scripts/offline-essentials-catalog.json"
DEFAULT_OUTPUT = ROOT / "src-tauri/resources/essentials/essentials.zip"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def catalog_source_digest():
    # The build hook can reject stale source without starting a second Cargo build.
    source = (ROOT / "core/src/setup/catalog.rs").read_bytes().replace(b"\r\n", b"\n")
    return hashlib.sha256(source).hexdigest()


def artifacts(catalog):
    for component in catalog["components"]:
        for artifact in component["artifacts"]:
            yield component["id"], artifact


def verify_file(path, artifact):
    if not path.is_file() or path.stat().st_size != artifact["bytes"]:
        raise ValueError(f"Missing or incorrect size: {path.name}")
    if digest(path) != artifact["sha256"]:
        raise ValueError(f"SHA-256 mismatch: {path.name}")


def verify_package(path, catalog):
    expected = {f"components/{component}/{a['file']}": a for component, a in artifacts(catalog)}
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)) or set(names) != set(expected) | {"manifest.json"}:
            raise ValueError("Essentials package has missing, duplicate or unexpected members")
        info = archive.getinfo("manifest.json")
        if info.file_size > 1024 * 1024:
            raise ValueError("Essentials manifest is too large")
        manifest = json.loads(archive.read(info))
        if manifest["components"] != catalog["components"] or manifest["package_version"] != catalog["package_version"]:
            raise ValueError("Essentials package does not match this release catalog")
        if manifest.get("catalog_source_sha256") != catalog_source_digest():
            raise ValueError("Essentials package predates the current catalog source; prepare it again")
        version = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"]
        if manifest["compatible_app_versions"] != [version]:
            raise ValueError("Essentials package targets another application version")
        for name, artifact in expected.items():
            if archive.getinfo(name).file_size != artifact["bytes"]:
                raise ValueError(f"Incorrect package size: {name}")
            with archive.open(name) as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != artifact["sha256"]:
                    raise ValueError(f"Incorrect package checksum: {name}")


def checked_sources(catalog):
    result = subprocess.run(["cargo", "run", "--quiet", "-p", "sakatsu-core", "--bin", "essentials-catalog"], cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8")
    live = json.loads(result.stdout)
    stripped = json.loads(json.dumps(live))
    for _, artifact in artifacts(stripped):
        artifact.pop("url")
    if stripped["components"] != catalog["components"]:
        raise ValueError("The release catalog and essentials snapshot have drifted")
    return {(component, a["file"]): a["url"] for component, a in artifacts(live)}


def prepare(source, output, catalog, urls):
    source = source.resolve()
    for component, artifact in artifacts(catalog):
        path = source / component / artifact["file"]
        if not path.exists() and urls is not None:
            path.parent.mkdir(parents=True, exist_ok=True)
            url = urls[component, artifact["file"]]
            # Only catalog pins may use the historical HTTP endpoint; the digest is mandatory.
            request = urllib.request.Request(url, headers={"User-Agent": "Sakatsu-release-builder/1"})
            fd, temporary = tempfile.mkstemp(prefix=".fetch-", dir=path.parent)
            try:
                with os.fdopen(fd, "wb") as target, urllib.request.urlopen(request, timeout=60) as response:
                    remaining = artifact["bytes"] + 1
                    while remaining:
                        chunk = response.read(min(1024 * 1024, remaining))
                        if not chunk:
                            break
                        target.write(chunk)
                        remaining -= len(chunk)
                verify_file(Path(temporary), artifact)
                if path.exists():
                    verify_file(path, artifact)
                else:
                    os.replace(temporary, path)
            finally:
                Path(temporary).unlink(missing_ok=True)
        verify_file(path, artifact)
    if output.exists():
        verify_package(output, catalog)
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    version = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"]
    manifest = dict(catalog, compatible_app_versions=[version], catalog_source_sha256=catalog_source_digest())
    fd, temporary = tempfile.mkstemp(prefix=".essentials-", dir=output.parent)
    os.close(fd)
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_STORED) as archive:
            info = zipfile.ZipInfo("manifest.json")
            archive.writestr(info, json.dumps(manifest, ensure_ascii=False, sort_keys=True).encode("utf-8"))
            for component, artifact in artifacts(catalog):
                info = zipfile.ZipInfo(f"components/{component}/{artifact['file']}")
                with (source / component / artifact["file"]).open("rb") as stream, archive.open(info, "w") as target:
                    shutil.copyfileobj(stream, target)
        verify_package(Path(temporary), catalog)
        # A concurrent build cannot replace a package another build already published.
        try:
            os.link(temporary, output)
        except FileExistsError:
            verify_package(output, catalog)
    except BaseException:
        # Never remove an existing package: it may belong to another build.
        raise
    finally:
        Path(temporary).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=ROOT / ".cache/essentials")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--fetch-missing", action="store_true")
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    catalog = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    notices = ROOT / "src-tauri/resources/essentials/RESOURCE-NOTICES.txt"
    if not notices.is_file():
        raise ValueError("Bundled resource licence notices are missing")
    if args.verify:
        verify_package(args.output, catalog)
    else:
        # Compare the compiled catalog even when all source files are already cached.
        urls = checked_sources(catalog)
        prepare(args.source_root, args.output, catalog, urls if args.fetch_missing else None)
    print(f"Essentials verified: {args.output.stat().st_size:,} bytes, SHA-256 {digest(args.output)}")


if __name__ == "__main__":
    main()
