"""Run every eligible Cargo test target without retaining all linked test executables.

Requires an explicit, disposable --target-dir. Existing nonempty directories without our
ownership marker are refused. Dependencies and ordinary binaries are never removed.
"""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time


MARKER = ".sakatsu-ci-target.json"


def owned_target(directory, manifest):
    directory = Path(directory).absolute()
    if directory.is_symlink():
        raise ValueError("target directory must not be a symlink")
    directory = directory.resolve()
    manifest = Path(manifest).resolve()
    if manifest.is_relative_to(directory):
        raise ValueError("target directory must not contain the source checkout")
    identity = {"version": 1, "manifest": str(manifest)}
    marker = directory / MARKER
    if directory.exists() and any(directory.iterdir()):
        if not marker.is_file() or json.loads(marker.read_text()) != identity:
            raise ValueError("refusing a nonempty target directory not owned by this runner")
    else:
        directory.mkdir(parents=True, exist_ok=True)
        marker.write_text(json.dumps(identity), encoding="utf-8")
    return directory


def remove_harnesses(artifacts, target, package_id):
    target = Path(target).resolve()
    removed = []
    for artifact in artifacts:
        if (artifact.get("reason") != "compiler-artifact"
                or artifact.get("package_id") != package_id
                or not artifact.get("profile", {}).get("test")
                or not artifact.get("executable")):
            continue
        executable = Path(artifact["executable"])
        resolved = executable.resolve()
        if not executable.is_absolute() or not resolved.is_relative_to(target) or executable.is_symlink():
            raise ValueError("refusing a test executable outside the owned target directory")
        if executable.is_file():
            executable.unlink()
            removed.append(str(executable))
    return removed


def selected_targets(package, requested):
    enabled = set(requested)
    pending = ["default", *requested]
    while pending:
        feature = pending.pop()
        for child in package["features"].get(feature, []):
            if child in package["features"] and child not in enabled:
                enabled.add(child)
                pending.append(child)
    selected, skipped = [], []
    for target in package["targets"]:
        if not target.get("test") or not set(target["kind"]) & {"lib", "bin", "test"}:
            continue
        missing = set(target.get("required-features", [])) - enabled
        if missing:
            skipped.append({"target": target["name"], "missing_features": sorted(missing)})
        else:
            kind = next(kind for kind in ["lib", "bin", "test"] if kind in target["kind"])
            selected.append({"kind": kind, "name": target["name"]})
    selected.sort(key=lambda item: (["lib", "bin", "test"].index(item["kind"]), item["name"]))
    if any(target.get("doctest") for target in package["targets"]):
        selected.append({"kind": "doc", "name": package["name"]})
    if not selected:
        raise ValueError("Cargo metadata selected no test targets")
    return selected, skipped


def tree_bytes(directory):
    return sum(path.stat().st_size for path in directory.rglob("*") if path.is_file())


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest-path", type=Path, default=Path(__file__).resolve().parent.parent / "Cargo.toml")
    parser.add_argument("--package", default="sakatsu-core")
    parser.add_argument("--features", default="")
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--log-dir", type=Path)
    args = parser.parse_args(argv)
    target = owned_target(args.target_dir, args.manifest_path)
    logs = args.log_dir or target.with_name(target.name + "-logs")
    logs.mkdir(parents=True, exist_ok=True)
    common = ["--manifest-path", str(args.manifest_path.resolve()), "--locked"]
    features = [name for name in args.features.split(",") if name]
    if features:
        common += ["--features", ",".join(features)]
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1", *common]))
    packages = [package for package in metadata["packages"] if package["name"] == args.package]
    if len(packages) != 1:
        raise ValueError("requested package is not uniquely identified by Cargo metadata")
    package = packages[0]
    selected, skipped = selected_targets(package, features)
    start_free = shutil.disk_usage(target).free
    report = {"package": args.package, "selected": selected, "feature_gated": skipped,
              "results": [], "starting_free_bytes": start_free, "minimum_free_bytes": start_free,
              "peak_observed_target_bytes": 0, "success": False}
    stop = threading.Event()

    def sample():
        while not stop.wait(0.25):
            report["minimum_free_bytes"] = min(report["minimum_free_bytes"], shutil.disk_usage(target).free)

    sampler = threading.Thread(target=sample, daemon=True)
    sampler.start()
    try:
        print(f"Selected {len(selected)} targets; {len(skipped)} feature-gated; free disk {start_free:,} bytes.", flush=True)
        for index, item in enumerate(selected):
            selector = ["--lib"] if item["kind"] == "lib" else ["--doc"] if item["kind"] == "doc" else ["--" + item["kind"], item["name"]]
            command = ["cargo", "test", *common, "--package", args.package, "--target-dir", str(target),
                       *selector, "--message-format=json"]
            artifacts = []
            log_path = logs / f"{index:03}-{item['kind']}-{item['name']}.log"
            with log_path.open("w", encoding="utf-8") as log:
                process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                           text=True, encoding="utf-8", errors="replace")
                for line in process.stdout:
                    log.write(line)
                    try:
                        message = json.loads(line)
                    except json.JSONDecodeError:
                        print(line, end="", flush=True)
                        continue
                    if isinstance(message, dict):
                        if message.get("reason") == "compiler-artifact":
                            artifacts.append(message)
                        elif message.get("reason") == "compiler-message":
                            print(message["message"].get("rendered", ""), end="", flush=True)
                code = process.wait()
            report["peak_observed_target_bytes"] = max(report["peak_observed_target_bytes"], tree_bytes(target))
            if code == 0 and item["kind"] != "doc" and not any(
                    artifact.get("package_id") == package["id"]
                    and artifact.get("profile", {}).get("test")
                    and artifact.get("executable") for artifact in artifacts):
                raise ValueError(f"Cargo reported no test executable for {item['name']}")
            removed = remove_harnesses(artifacts, target, package["id"])
            report["results"].append({**item, "exit_code": code, "removed_harnesses": removed})
            print(f"[{index + 1}/{len(selected)}] {item['kind']} {item['name']}: exit {code}; removed {len(removed)} harnesses.", flush=True)
            if code:
                return code
        report["success"] = len(report["results"]) == len(selected)
        return 0
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        stop.set()
        sampler.join()
        report["ending_free_bytes"] = shutil.disk_usage(target).free
        report["minimum_free_bytes"] = min(report["minimum_free_bytes"], report["ending_free_bytes"])
        report["completed"] = len(report["results"])
        report["failed"] = sum(result["exit_code"] != 0 for result in report["results"])
        (logs / "summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"Completed {report['completed']}/{len(selected)} targets; failed {report['failed']}; "
              f"peak observed target bytes {report['peak_observed_target_bytes']:,}; "
              f"minimum free bytes {report['minimum_free_bytes']:,}.", flush=True)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"CI test runner: {error}", file=sys.stderr)
        sys.exit(1)
