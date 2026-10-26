import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("prepare_release", Path(__file__).with_name("prepare-release.py"))
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_public_endpoint_policy(self):
        for value in ["http://github.com/a", "https://localhost/a", "https://127.0.0.1/a", "https://10.0.0.1/a", "https://user:secret@github.com/a", "https://example.com/a", "https://github.com/a?token=secret", "https://github.com/a b"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                release.https_url(value)
        self.assertEqual(release.https_url("https://github.com/owner/repo/releases/latest/download/latest.json"), "https://github.com/owner/repo/releases/latest/download/latest.json")

    def test_real_public_key_required(self):
        key = base64.b64encode(b"untrusted comment: fixture\n" + base64.b64encode(b"Ed" + b"a" * 40) + b"\n").decode()
        result = release.config("https://github.com/owner/repo/feed", key)
        self.assertTrue(result["plugins"]["updater"]["requireSignedVersion"])
        for bad in ["placeholder", base64.b64encode(b"private key").decode()]:
            with self.assertRaises(ValueError):
                release.public_key(bad)

    def test_evidence_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "record.json"
            release.write_json(path, {"hash": "first"})
            release.write_json(path, {"hash": "first"})
            with self.assertRaises(ValueError):
                release.write_json(path, {"hash": "second"})
            self.assertEqual(json.loads(path.read_text())["hash"], "first")

    def test_only_main_or_version_tag_can_be_signed(self):
        for ref in ["feature/test", "--help", "refs/pull/2/head", "a" * 40, "fork/main"]:
            with self.assertRaises(ValueError):
                release.trusted_revision(ref, lambda *args: self.fail("Git must not run"))
        calls = []
        def git(*args):
            calls.append(args)
            return "a" * 40 if args[0] == "rev-parse" else ""
        self.assertEqual(release.trusted_revision("main", git), "a" * 40)
        self.assertIn(("merge-base", "--is-ancestor", "a" * 40, "refs/remotes/origin/main"), calls)

    def test_bundle_contains_only_catalog_companions(self):
        import re
        root = release.ROOT
        expected = set(re.findall(r'script: Some\("([^"]+)"\)', (root / "core/src/setup/catalog.rs").read_text(encoding="utf8")))
        for name in ["src-tauri/tauri.conf.json", "src-tauri/tauri.windows.conf.json"]:
            resources = json.loads((root / name).read_text(encoding="utf8"))["bundle"]["resources"]
            scripts = {Path(p).name for p in resources if p.endswith(".ps1")}
            self.assertEqual(scripts, expected)
            self.assertFalse(any("*" in p for p in resources))

    def test_developer_entrypoints_cannot_be_discovered_by_bundler(self):
        self.assertEqual(list((release.ROOT / "src-tauri/src/bin").glob("*.rs")), [])

    def test_current_version_agreement(self):
        self.assertRegex(release.versions(), r"^\d+\.\d+\.\d+$")


if __name__ == "__main__":
    unittest.main()
