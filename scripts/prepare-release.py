"""Prepare release inputs and metadata. Never publishes, signs, or submits anything."""
import argparse
import base64
import hashlib
import ipaddress
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent


def https_url(value):
    if any(char.isspace() for char in value):
        raise ValueError("Release URLs must encode whitespace")
    url = urlsplit(value)
    if url.scheme != "https" or not url.hostname or url.username or url.password or url.fragment or url.query:
        raise ValueError("A public HTTPS URL without credentials, query, or fragment is required")
    host = url.hostname.lower()
    if host == "localhost" or host.endswith((".localhost", ".invalid", ".test", ".example")) or host in {"example.com", "example.org"}:
        raise ValueError("Placeholder and loopback hosts are not release destinations")
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return value
    if not address.is_global:
        raise ValueError("A public host is required")
    return value


def public_key(value):
    decoded = base64.b64decode(value.strip(), validate=True).decode("ascii")
    lines = decoded.splitlines()
    if len(lines) != 2 or not lines[0].startswith("untrusted comment:"):
        raise ValueError("Expected a Tauri public key, not a private key or placeholder")
    key = base64.b64decode(lines[1], validate=True)
    if len(key) != 42 or key[:2] != b"Ed":
        raise ValueError("Invalid Minisign public key")
    return value.strip()


def versions(root=ROOT):
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    values = [cargo]
    for name in ["package.json", "ui/package.json", "src-tauri/tauri.conf.json"]:
        values.append(json.loads((root / name).read_text(encoding="utf-8"))["version"])
    if len(set(values)) != 1:
        raise ValueError("Cargo, npm and Tauri versions disagree")
    if not re.fullmatch(r"\d+\.\d+\.\d+", cargo):
        raise ValueError("Only stable three-part releases are supported")
    return cargo


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(value, indent=2, ensure_ascii=False) + "\n"
    # Evidence is immutable: an identical rerun is harmless; a different one needs a new path.
    if path.exists():
        if path.read_text(encoding="utf-8") != text:
            raise ValueError(f"Refusing to overwrite different release evidence: {path.name}")
        return
    with path.open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(text)


def config(endpoint, key):
    return {"bundle": {"createUpdaterArtifacts": True}, "plugins": {"updater": {
        "pubkey": public_key(key), "endpoints": [https_url(endpoint)],
        "requireSignedVersion": True, "windows": {"installMode": "passive"},
    }}}


def trusted_revision(ref, git=None):
    if git is None:
        git = lambda *args: subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()
    if ref != "main" and not re.fullmatch(r"v\d+\.\d+\.\d+", ref):
        raise ValueError("Release source must be main or a stable version tag")
    source = "refs/remotes/origin/main" if ref == "main" else "refs/tags/" + ref
    sha = git("rev-parse", "--verify", source + "^{commit}")
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Git did not resolve an immutable commit")
    # A tag on an unrelated branch or a fork is not eligible for signing.
    git("merge-base", "--is-ancestor", sha, "refs/remotes/origin/main")
    if ref != "main":
        cargo = tomllib.loads(git("show", sha + ":Cargo.toml"))["workspace"]["package"]["version"]
        if ref != "v" + cargo:
            raise ValueError("Release tag does not match Cargo version")
    return sha


def manifest(installer, url, revision, version):
    installer = Path(installer)
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("An immutable source revision is required")
    signature = Path(str(installer) + ".sig").read_text(encoding="utf-8").strip()
    base64.b64decode(signature, validate=True)
    return {"source_revision": revision, "version": version, "architecture": "x86_64",
            "installer": installer.name, "sha256": hashlib.sha256(installer.read_bytes()).hexdigest(),
            "bytes": installer.stat().st_size, "url": https_url(url), "signature": signature}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    cfg = commands.add_parser("config")
    cfg.add_argument("--endpoint", required=True)
    cfg.add_argument("--public-key-file", type=Path, required=True)
    cfg.add_argument("--output", type=Path, required=True)
    trust = commands.add_parser("revision")
    trust.add_argument("--ref", required=True)
    out = commands.add_parser("manifest")
    out.add_argument("--installer", type=Path, required=True)
    out.add_argument("--url", required=True)
    out.add_argument("--revision", required=True)
    out.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "config":
        versions()
        # Generated config contains machine-specific build inputs and belongs only in build output.
        output = args.output.resolve()
        if not output.is_relative_to((ROOT / "target").resolve()):
            raise ValueError("Generated release configuration must remain below target/")
        write_json(output, config(args.endpoint, args.public_key_file.read_text(encoding="utf-8")))
    elif args.command == "revision":
        print(trusted_revision(args.ref))
    else:
        version = versions()
        result = manifest(args.installer, args.url, args.revision, version)
        source = args.output.parent / "source.zip"
        if not source.is_file():
            raise ValueError("The corresponding source archive is required before release metadata")
        result["source_sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
        result["toolchains"] = {tool: subprocess.check_output([tool, "--version"], text=True).strip() for tool in ["rustc", "node"]}
        proof = json.loads((args.output.parent / "verification.json").read_text(encoding="utf-8-sig"))
        if proof.get("installer_sha256") != result["sha256"] or any(proof.get(key) is not True for key in ["updater_signature", "authenticode_installer", "authenticode_executable"]):
            raise ValueError("Successful verification evidence for these exact installer bytes is required")
        result["verification"] = proof
        result["build_config_sha256"] = hashlib.sha256((args.output.parent / "build-config.json").read_bytes()).hexdigest()
        write_json(args.output, result)
        write_json(args.output.parent / "latest.json", {"version": version, "platforms": {"windows-x86_64": {"url": result["url"], "signature": result["signature"]}}})


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Release preparation refused: {error}", file=sys.stderr)
        sys.exit(1)
