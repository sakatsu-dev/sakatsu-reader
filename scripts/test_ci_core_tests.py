"""Exercise the CI runner against real, dependency-free Cargo targets."""

import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("ci-core-tests.py")
spec = importlib.util.spec_from_file_location("ci_core_tests", SCRIPT)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_ownership_refuses_existing_or_source_containing_directories(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "source/Cargo.toml"
            manifest.parent.mkdir()
            manifest.write_text("")
            with self.assertRaisesRegex(ValueError, "contain the source"):
                runner.owned_target(root, manifest)
            unknown = root / "unknown"
            unknown.mkdir()
            (unknown / "keep").write_text("existing data")
            with self.assertRaisesRegex(ValueError, "not owned"):
                runner.owned_target(unknown, manifest)
            target = runner.owned_target(root / "owned", manifest)
            self.assertEqual(runner.owned_target(target, manifest), target)
            with self.assertRaisesRegex(ValueError, "not owned"):
                runner.owned_target(target, root / "other.toml")

    def test_cleanup_preserves_ordinary_binaries_and_refuses_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "target"
            target.mkdir()
            normal, harness, outside = target / "normal", target / "test", root / "keep"
            for file in [normal, harness, outside]:
                file.write_text("data")
            def artifact(file, test):
                return {"reason": "compiler-artifact", "package_id": "fixture",
                        "profile": {"test": test}, "executable": str(file)}
            runner.remove_harnesses([artifact(normal, False), artifact(harness, True)], target, "fixture")
            self.assertTrue(normal.exists())
            self.assertFalse(harness.exists())
            with self.assertRaisesRegex(ValueError, "outside"):
                runner.remove_harnesses([artifact(outside, True)], target, "fixture")
            self.assertTrue(outside.exists())

    def test_required_features_follow_default_feature_dependencies(self):
        package = {"name": "fixture", "features": {"default": ["outer"], "outer": ["inner"], "inner": [], "optional": []},
                   "targets": [{"name": "included", "kind": ["bin"], "test": True, "required-features": ["inner"]},
                               {"name": "gated", "kind": ["bin"], "test": True, "required-features": ["optional"]}]}
        targets, skipped = runner.selected_targets(package, [])
        self.assertEqual([target["name"] for target in targets], ["included"])
        self.assertEqual(skipped, [{"target": "gated", "missing_features": ["optional"]}])
        targets, skipped = runner.selected_targets(package, ["optional"])
        self.assertEqual({target["name"] for target in targets}, {"included", "gated"})
        self.assertEqual(skipped, [])

    def real_fixture(self, root):
        (root / "src").mkdir()
        (root / "tests").mkdir()
        (root / "Cargo.toml").write_text('''[package]
name = "ci-runner-fixture"
version = "0.1.0"
edition = "2021"
[features]
extra = []
[[bin]]
name = "extra"
path = "src/extra.rs"
required-features = ["extra"]
''')
        (root / "src/lib.rs").write_text('''/// A doctest is a separate part of Cargo coverage.
/// ```
/// assert_eq!(ci_runner_fixture::value(), 7);
/// ```
pub fn value() -> u8 { 7 }
#[test] fn library_case() { assert_eq!(value(), 7); }
#[test] #[ignore = "explicit ignored control"] fn ignored_case() { panic!("must stay ignored"); }
''')
        (root / "src/main.rs").write_text('fn main() { println!("fixture"); }\n#[test] fn binary_case() { assert!(true); }\n')
        (root / "src/extra.rs").write_text('fn main() {}\n#[test] fn feature_case() { assert!(true); }\n')
        (root / "tests/first.rs").write_text('''#[test] fn integration_case() { assert_eq!(ci_runner_fixture::value(), 7); }
#[test] fn normal_binary_is_available() {
    assert!(std::path::Path::new(env!("CARGO_BIN_EXE_ci-runner-fixture")).is_file());
}
''')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, check=True, capture_output=True)

    def invoke(self, args, root):
        environment = os.environ.copy()
        environment.pop("CARGO_TARGET_DIR", None)
        environment["CARGO_INCREMENTAL"] = "0"
        return subprocess.run(args, cwd=root, env=environment, capture_output=True, text=True,
                              encoding="utf-8", errors="replace")

    def test_real_coverage_matches_cargo_and_failure_is_not_green(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            source = base / "source"
            source.mkdir()
            self.real_fixture(source)
            for features in [[], ["extra"]]:
                tag = "feature" if features else "default"
                flags = ["--features", "extra"] if features else []
                baseline = self.invoke(["cargo", "test", "--locked", "--target-dir", str(base / (tag + "-ordinary")), *flags], source)
                self.assertEqual(baseline.returncode, 0, baseline.stdout + baseline.stderr)
                target, logs = base / (tag + "-bounded"), base / (tag + "-logs")
                bounded = self.invoke([sys.executable, str(SCRIPT), "--package", "ci-runner-fixture",
                                       "--manifest-path", str(source / "Cargo.toml"), "--target-dir", str(target),
                                       "--log-dir", str(logs), *flags], source)
                self.assertEqual(bounded.returncode, 0, bounded.stdout + bounded.stderr)
                cases = lambda text: set(re.findall(r"^test (.+?) \.\.\. (ok|ignored)", text, re.MULTILINE))
                self.assertEqual(cases(baseline.stdout), cases(bounded.stdout))
                self.assertTrue(cases(bounded.stdout))
                report = json.loads((logs / "summary.json").read_text())
                self.assertTrue(report["success"])
                self.assertEqual(report["completed"], len(report["selected"]))
                self.assertEqual(len(report["feature_gated"]), 0 if features else 1)
                self.assertTrue(any(row["kind"] == "doc" for row in report["results"]))
                removed = [path for row in report["results"] for path in row["removed_harnesses"]]
                self.assertTrue(removed)
                self.assertTrue(all(not Path(path).exists() for path in removed))
                binary = target / "debug" / ("ci-runner-fixture.exe" if os.name == "nt" else "ci-runner-fixture")
                self.assertTrue(binary.exists())
            (source / "tests/first.rs").write_text('#[test] fn deliberately_failing() { panic!("failure control"); }')
            failed = self.invoke([sys.executable, str(SCRIPT), "--package", "ci-runner-fixture",
                                  "--manifest-path", str(source / "Cargo.toml"), "--target-dir", str(base / "failure-target"),
                                  "--log-dir", str(base / "failure-logs")], source)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn("deliberately_failing", failed.stdout)
            report = json.loads((base / "failure-logs/summary.json").read_text())
            self.assertFalse(report["success"])
            self.assertEqual(report["failed"], 1)
            self.assertLess(report["completed"], len(report["selected"]))


if __name__ == "__main__":
    unittest.main()
