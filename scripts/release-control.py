"""Release metadata and gates. Unsigned previews are restricted to private repositories."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
CI_JOBS = {"core (linux)", "desktop (windows)", "ui (checks/build)", "analysis layout (edge/windows)"}
VERSION_JSON = ("package.json", "ui/package.json", "src-tauri/tauri.conf.json")
SHA = re.compile(r"[0-9a-f]{40}")


def version(value):
    if not re.fullmatch(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", value):
        raise ValueError("Expected a stable three-part application version")
    return tuple(map(int, value.split(".")))


def run(*args, cwd=ROOT):
    return subprocess.check_output(args, cwd=cwd, text=True, encoding="utf-8").strip()


def api(repo, path, method="GET", payload=None):
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", repo):
        raise ValueError("Invalid repository")
    command = ["gh", "api", "--method", method, f"repos/{repo}" + (f"/{path}" if path else "")]
    if payload is not None:
        command += ["--input", "-"]
    output = subprocess.check_output(command, input=json.dumps(payload) if payload is not None else None,
                                     text=True, encoding="utf-8")
    return json.loads(output) if output.strip() else None


def state(read):
    cargo = tomllib.loads(read("Cargo.toml"))
    current = cargo["workspace"]["package"]["version"]
    version(current)
    members = cargo["workspace"]["members"]
    names = []
    for member in members:
        if not re.fullmatch(r"[\w/-]+", member) or ".." in Path(member).parts:
            raise ValueError("Workspace member layout needs explicit release support")
        package = tomllib.loads(read(member + "/Cargo.toml"))["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError("Workspace members must inherit the application version")
        names.append(package["name"])
    if len(set(names)) != len(names):
        raise ValueError("Duplicate workspace package")
    for path in VERSION_JSON:
        if json.loads(read(path))["version"] != current:
            raise ValueError("Application versions disagree")
    lock = json.loads(read("ui/package-lock.json"))
    if lock["version"] != current or lock["packages"][""]["version"] != current:
        raise ValueError("npm lockfile version disagrees")
    packages = tomllib.loads(read("Cargo.lock"))["package"]
    for name in names:
        found = [p for p in packages if p["name"] == name]
        if len(found) != 1 or found[0]["version"] != current or "source" in found[0]:
            raise ValueError("Workspace lockfile version disagrees")
    return current, names


def bump_plan(root, target):
    root = Path(root)
    read = lambda p: (root / p).read_text(encoding="utf-8")
    current, names = state(read)
    if version(target) <= version(current):
        raise ValueError("The next release must be newer than the current version")
    updates = {}
    text = read("Cargo.toml")
    section = re.search(r"(?ms)^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)", text)
    if not section:
        raise ValueError("Workspace package section missing")
    body, count = re.subn(r'(?m)^version\s*=\s*"[^"]+"', f'version = "{target}"', section[1])
    if count != 1:
        raise ValueError("Expected one workspace version")
    updates["Cargo.toml"] = text[:section.start(1)] + body + text[section.end(1):]
    text = read("Cargo.lock")
    blocks = re.split(r"(?m)(?=^\[\[package\]\])", text)
    changed = set()
    for i, block in enumerate(blocks):
        if not block.startswith("[[package]]"):
            continue
        package = tomllib.loads(block)["package"][0]
        if package["name"] in names:
            blocks[i], count = re.subn(r'(?m)^version = "[^"]+"', f'version = "{target}"', block, count=1)
            if count != 1:
                raise ValueError("Expected one locked workspace version")
            changed.add(package["name"])
    if changed != set(names):
        raise ValueError("Incomplete workspace lockfile update")
    updates["Cargo.lock"] = "".join(blocks)
    for path in VERSION_JSON + ("ui/package-lock.json",):
        data = json.loads(read(path))
        data["version"] = target
        if path.endswith("package-lock.json"):
            data["packages"][""]["version"] = target
        updates[path] = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    after = lambda p: updates.get(p, read(p))
    if state(after)[0] != target:
        raise ValueError("Version update did not reconcile")
    return {"previous": current, "version": target,
            "updates": [{"path": p, "content": value} for p, value in updates.items()]}


def check_ci(run_data, jobs, sha, repo):
    if (run_data.get("head_sha") != sha or run_data.get("head_branch") != "main"
            or run_data.get("head_repository", {}).get("full_name") != repo
            or run_data.get("event") not in {"push", "workflow_dispatch"}
            or run_data.get("path") != ".github/workflows/ci.yml"):
        raise ValueError("CI evidence does not belong to this repository's main commit")
    if run_data.get("status") != "completed":
        return False
    if run_data.get("conclusion") != "success":
        raise ValueError("The latest CI run did not pass")
    if len(jobs) != len(CI_JOBS) or {j.get("name") for j in jobs} != CI_JOBS:
        raise ValueError("CI job accounting differs from the required inventory")
    if any(j.get("status") != "completed" or j.get("conclusion") != "success"
           or j.get("run_id") != run_data["id"]
           or j.get("run_attempt") != run_data["run_attempt"] for j in jobs):
        raise ValueError("Every required job in this CI attempt must pass")
    return True


def wait_ci(repo, sha, timeout=2700, interval=30):
    deadline = time.monotonic() + timeout
    while True:
        runs = api(repo, f"actions/workflows/ci.yml/runs?head_sha={sha}&per_page=100")["workflow_runs"]
        runs = [r for r in runs if r.get("head_branch") == "main" and r.get("event") in {"push", "workflow_dispatch"}]
        if runs:
            latest = max(runs, key=lambda r: (r["id"], r["run_attempt"]))
            jobs = api(repo, f"actions/runs/{latest['id']}/jobs?filter=latest&per_page=100")["jobs"]
            if check_ci(latest, jobs, sha, repo):
                return {"run": latest, "jobs": jobs}
        if time.monotonic() >= deadline:
            raise ValueError("Timed out waiting for all CI jobs on the release commit")
        print("Waiting for this commit's complete main-branch CI run", file=sys.stderr, flush=True)
        time.sleep(interval)


def output(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(data, indent=2) + "\n"
    if path.exists():
        if path.read_text(encoding="utf-8") != text:
            raise ValueError("Refusing to replace different release evidence")
    else:
        path.write_text(text, encoding="utf-8")


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def load_tool(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def audit_source_text(text):
    # The IPC privacy test intentionally uses this synthetic username. Real build paths
    # are refused; the local publication audit additionally checks private identifiers.
    users = re.findall(r"(?i)[a-z]:[\\/]users[\\/]([^\\/\s\"']+)", text)
    if any(name.lower() != "someone" for name in users) or re.search(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{25,}|github_pat_[A-Za-z0-9_]{25,})\b", text):
        raise ValueError("Source archive contains a user path or credential-shaped value")


def verify_payload(payload, built_app):
    payload = Path(payload)
    runtime = load_tool("prepare-webview")
    spec, files = runtime.specification()
    if not runtime.verify_directory(payload / spec["directory"], spec, files):
        raise ValueError("Installer runtime contents differ from the publisher pin")
    resources = {}
    for name in ("tauri.conf.json", "tauri.windows.conf.json"):
        resources.update(json.loads((ROOT / "src-tauri" / name).read_text(encoding="utf-8"))["bundle"]["resources"])
    checked = []
    for origin, destination in resources.items():
        base = (ROOT / "src-tauri" / origin).resolve()
        if not base.is_relative_to(ROOT):
            raise ValueError("A bundled resource is outside the source tree")
        relative = Path(destination)
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("Invalid resource destination")
        entries = [p for p in base.rglob("*") if p.is_file()] if base.is_dir() else [base]
        for file in entries:
            installed = payload / relative / file.relative_to(base) if base.is_dir() else payload / relative
            if digest(file) != digest(installed):
                raise ValueError("Installer resource bytes disagree")
            checked.append(installed.relative_to(payload).as_posix())
    expected = Path(built_app).read_bytes()
    marker = b"__TAURI_BUNDLE_TYPE_VAR_"
    if expected.count(marker + b"UNK") == 1:
        at = expected.index(marker) + len(marker)
        expected = expected[:at] + b"NSS" + expected[at + 3:]
    actual = (payload / "sakatsu-desktop.exe").read_bytes()
    if hashlib.sha256(expected).digest() != hashlib.sha256(actual).digest():
        raise ValueError("Installer contains a different application")
    for needle in (b"c:\\users\\", b"c:/users/"):
        if needle in actual.lower() or needle.decode().encode("utf-16le") in actual.lower():
            raise ValueError("Application contains an absolute user path; check compiler remapping")
    owned = {p.stem + ".exe" for folder in (ROOT / "core/src/bin", ROOT / "src-tauri/src/tools") for p in folder.glob("*.rs")}
    for manifest in (ROOT / "core/Cargo.toml", ROOT / "src-tauri/Cargo.toml"):
        owned.update(b["name"] + ".exe" for b in tomllib.loads(manifest.read_text(encoding="utf-8")).get("bin", []))
    owned.discard("sakatsu-desktop.exe")
    if any(p.name in owned for p in payload.rglob("*.exe")):
        raise ValueError("Installer contains a development executable")
    essentials = load_tool("prepare-essentials")
    essentials.verify_package(payload / "essentials/essentials.zip",
                              json.loads((ROOT / "scripts/offline-essentials-catalog.json").read_text(encoding="utf-8")))
    return {"passed": True, "runtime_files_verified": len(files), "resources_verified": checked}


def package_preview(installer, payload, built_app, qa_path, sha, destination, verify_only=False):
    current, _ = state(lambda p: (ROOT / p).read_text(encoding="utf-8"))
    if not SHA.fullmatch(sha) or run("git", "rev-parse", "HEAD") != sha:
        raise ValueError("Build source identity disagrees")
    proof = verify_payload(payload, built_app)
    installer = Path(installer)
    qa = json.loads(Path(qa_path).read_text(encoding="utf-8-sig"))
    installer_sha = digest(installer)
    if qa.get("passed") is not True or qa.get("installer_sha256", "").lower() != installer_sha:
        raise ValueError("Successful QA for these exact installer bytes is required")
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    # A new build never replaces previous release files, even before publication.
    name = f"SakatsuReader_{current}_x64-setup.exe"
    target = destination / name
    if target.exists():
        raise ValueError("Release output is not empty")
    import shutil
    shutil.copy2(installer, target)
    source = destination / "source.zip"
    subprocess.run(["git", "archive", "--format=zip", "--output=" + str(source.resolve()), sha], cwd=ROOT, check=True)
    with zipfile.ZipFile(source) as archive:
        for item in archive.infolist():
            if item.is_dir():
                continue
            data = archive.read(item)
            try:
                text = data.decode("utf-8")
            except UnicodeDecodeError:
                continue
            audit_source_text(text)
    config = ROOT / "target/preview-build-config.json"
    shutil.copy2(config, destination / "build-config.json")
    runtime, _ = load_tool("prepare-webview").specification()
    info = {"version": current, "source_revision": sha, "source_tree": run("git", "rev-parse", "HEAD^{tree}"),
            "installer": name, "installer_sha256": installer_sha, "installer_bytes": target.stat().st_size,
            "source_sha256": digest(source), "build_config_sha256": digest(config),
            "authenticode_signed": False, "application_updater_configured": False,
            "verification_only": verify_only, "webview2_version": runtime["version"],
            "essentials_sha256": digest(payload / "essentials/essentials.zip")}
    output(destination / "build-info.json", info)
    output(destination / "verification.json", {**proof, "installer_sha256": installer_sha})
    output(destination / "installer-qa.json", {"passed": True, "installer_sha256": installer_sha,
                                              "steps": qa["steps"], "observations": qa["observations"]})
    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    entries = re.findall(rf"(?ms)^##\s+\[?v?{re.escape(current)}\]?(?:\s|\().*?(?=^##\s|\Z)", changelog)
    if len(entries) != 1:
        raise ValueError("Expected one reviewed changelog entry for this release")
    (destination / "release-notes.md").write_text(entries[0].strip() +
        "\n\nPrivate Windows x64 preview. Unsigned. Includes essentials and app-local WebView2. "
        "Ichiran setup remains a separate download. Application self-updates are not configured.\n", encoding="utf-8")
    (destination / "BUILDING.txt").write_text(
        f"Source commit: {sha}\nSee DEVELOPMENT.md for Windows build prerequisites.\n"
        f"Prepare the pinned runtime in src-tauri/{runtime['directory']}.\n"
        "Copy build-config.json into the source root, then run:\n"
        "npm --prefix ui ci\npython scripts/prepare-essentials.py --fetch-missing\n"
        "cargo tauri build --no-bundle --config build-config.json -- --locked --no-default-features --bin sakatsu-desktop\n"
        "cargo tauri bundle --bundles nsis --config build-config.json\n", encoding="utf-8")
    names = [name, "source.zip", "build-info.json", "build-config.json", "BUILDING.txt"]
    (destination / "SHA256SUMS").write_text("".join(digest(destination / n) + "  " + n + "\n" for n in names), encoding="ascii")
    return info


def resolve_preview(repo, revision, verify_only=False):
    if api(repo, "").get("private") is not True:
        raise ValueError("Unsigned preview automation is restricted to a private repository")
    if revision != "main" and not SHA.fullmatch(revision) and not re.fullmatch(r"v\d+\.\d+\.\d+", revision):
        raise ValueError("Expected main, a version tag or an immutable main-branch commit")
    ref = "refs/remotes/origin/main" if revision == "main" else revision
    sha = run("git", "rev-parse", "--verify", ref + "^{commit}")
    if not SHA.fullmatch(sha):
        raise ValueError("Source did not resolve to an immutable commit")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "refs/remotes/origin/main"], cwd=ROOT, check=True)
    read = lambda p: run("git", "show", sha + ":" + p)
    current, _ = state(read)
    if revision.startswith("v") and revision != "v" + current:
        raise ValueError("Release tag and application version disagree")
    if json.loads(read(".github/release-please/manifest.json"))["."] != current:
        raise ValueError("Release manifest and application version disagree")
    releases = api(repo, "releases?per_page=100")
    found = [r for r in releases if r["tag_name"] == "v" + current]
    if len(found) > 1:
        raise ValueError("Ambiguous release identity")
    if found and not verify_only:
        if not found[0]["draft"]:
            raise ValueError("This version is already published; existing releases are immutable")
        if found[0]["target_commitish"] != sha:
            raise ValueError("The draft belongs to a different source commit")
    return {"sha": sha, "version": current, "verify_only": verify_only}


def publish_draft(repo, directory):
    directory = Path(directory)
    info = json.loads((directory / "build-info.json").read_text(encoding="utf-8-sig"))
    current, sha = info["version"], info["source_revision"]
    version(current)
    if not SHA.fullmatch(sha) or info.get("verification_only") is not False:
        raise ValueError("Invalid publication identity or rehearsal build")
    if api(repo, "").get("private") is not True:
        raise ValueError("Unsigned previews require a private repository")
    if run("git", "rev-parse", "HEAD") != sha or state(lambda p: (ROOT / p).read_text(encoding="utf-8"))[0] != current:
        raise ValueError("Downloaded build identity does not match the checked-out source")
    wait_ci(repo, sha, timeout=0)
    qa = json.loads((directory / "installer-qa.json").read_text(encoding="utf-8-sig"))
    installer = directory / info["installer"]
    if info["installer"] != f"SakatsuReader_{current}_x64-setup.exe" or not installer.is_file():
        raise ValueError("Invalid installer path")
    installer_digest = digest(installer)
    if qa.get("passed") is not True or qa.get("installer_sha256", "").lower() != installer_digest or info["installer_sha256"] != installer_digest:
        raise ValueError("Installer QA does not cover these exact bytes")
    proof = json.loads((directory / "verification.json").read_text(encoding="utf-8"))
    if proof.get("installer_sha256") != installer_digest or proof.get("passed") is not True:
        raise ValueError("Installer payload verification is missing or stale")
    if digest(directory / "source.zip") != info["source_sha256"] or digest(directory / "build-config.json") != info["build_config_sha256"]:
        raise ValueError("Source or build configuration differs from the verified build")
    config = json.loads((directory / "build-config.json").read_text(encoding="utf-8-sig"))
    if (info.get("authenticode_signed") is not False or info.get("application_updater_configured") is not False
            or config["bundle"]["createUpdaterArtifacts"] is not False):
        raise ValueError("This workflow only publishes unsigned, updater-disabled private previews")
    names = [info["installer"], "source.zip", "build-info.json", "build-config.json", "BUILDING.txt", "SHA256SUMS"]
    expected = {}
    for name in names:
        file = directory / name
        expected[name] = (file.stat().st_size, "sha256:" + digest(file))
    checksums = "".join(digest(directory / n) + "  " + n + "\n" for n in names if n != "SHA256SUMS")
    if (directory / "SHA256SUMS").read_text(encoding="ascii") != checksums:
        raise ValueError("Release checksum inventory disagrees")
    tag = "v" + current
    releases = api(repo, "releases?per_page=100")
    found = [r for r in releases if r["tag_name"] == tag]
    if len(found) > 1:
        raise ValueError("Ambiguous release identity")
    if found:
        release = found[0]
        if not release["draft"] or release["target_commitish"] != sha:
            raise ValueError("Refusing to change a published release or a draft for different source")
    else:
        release = api(repo, "releases", "POST", {"tag_name": tag, "target_commitish": sha,
                      "name": f"Sakatsu Reader {current} (private preview)", "draft": True, "prerelease": True,
                      "body": (directory / "release-notes.md").read_text(encoding="utf-8")})
    existing = {a["name"]: a for a in release["assets"]}
    if not set(existing).issubset(expected):
        raise ValueError("Draft has unexpected assets; review it before retrying")
    for name, asset in existing.items():
        if asset["state"] != "uploaded" or (asset["size"], asset.get("digest")) != expected[name]:
            raise ValueError("Draft contains different bytes; rerun the original failed publish job or remove the unpublished draft")
    for name in names:
        asset = existing.get(name)
        if not asset:
            live = api(repo, f"releases/{release['id']}")
            if not live["draft"] or live["target_commitish"] != sha:
                raise ValueError("Draft changed during publication; no more assets were uploaded")
            subprocess.run(["gh", "release", "upload", tag, str(directory / name), "--repo", repo], check=True)
    final = next(r for r in api(repo, "releases?per_page=100") if r["id"] == release["id"])
    actual = {a["name"]: (a["size"], a.get("digest")) for a in final["assets"]}
    if actual != expected or not final["draft"]:
        raise ValueError("Draft asset accounting or hashes disagree")
    body = (directory / "release-notes.md").read_text(encoding="utf-8")
    name = f"Sakatsu Reader {current} (private preview)"
    if final.get("body") != body or final.get("name") != name:
        api(repo, f"releases/{release['id']}", "PATCH", {"body": body, "name": name})
    print(final["html_url"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("bump-plan")
    p.add_argument("--version", required=True)
    p.add_argument("--root", type=Path, default=ROOT)
    p = sub.add_parser("resolve")
    p.add_argument("--repo", required=True)
    p.add_argument("--revision", required=True)
    p.add_argument("--verify-only", action="store_true")
    p.add_argument("--output", required=True)
    p = sub.add_parser("wait-ci")
    p.add_argument("--repo", required=True)
    p.add_argument("--sha", required=True)
    p.add_argument("--output", required=True)
    p = sub.add_parser("publish-draft")
    p.add_argument("--repo", required=True)
    p.add_argument("--directory", required=True)
    p = sub.add_parser("package-preview")
    for name in ("installer", "payload", "built-app", "qa", "sha", "output"):
        p.add_argument("--" + name, required=True)
    p.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    if args.command == "bump-plan":
        print(json.dumps(bump_plan(args.root, args.version)))
    elif args.command == "resolve":
        data = resolve_preview(args.repo, args.revision, args.verify_only)
        output(args.output, data)
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
            for name in ("sha", "version", "verify_only"):
                f.write(f"{name}={str(data[name]).lower()}\n")
    elif args.command == "wait-ci":
        if not SHA.fullmatch(args.sha):
            raise ValueError("Invalid source commit")
        output(args.output, wait_ci(args.repo, args.sha))
    elif args.command == "package-preview":
        print(json.dumps(package_preview(args.installer, Path(args.payload), args.built_app, args.qa,
                                         args.sha, args.output, args.verify_only)))
    else:
        publish_draft(args.repo, args.directory)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f"Release refused: {error}", file=sys.stderr)
        sys.exit(1)
