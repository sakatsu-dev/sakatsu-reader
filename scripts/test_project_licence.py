"""Keep first-party licensing consistent and present in the installed application."""

import hashlib
import json
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPDX = "GPL-3.0-only"


class ProjectLicenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(
            ["cargo", "metadata", "--offline", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT, capture_output=True, check=True,
        )
        cls.metadata = json.loads(result.stdout)
        cls.config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))

    def test_cargo_and_javascript_packages_declare_the_same_licence(self):
        packages = self.metadata["packages"]
        self.assertEqual({p["name"] for p in packages}, {"sakatsu-core", "sakatsu-desktop"})
        for package in packages:
            with self.subTest(package=package["name"]):
                self.assertEqual(package["license"], SPDX)
        for name in ("package.json", "ui/package.json"):
            with self.subTest(manifest=name):
                package = json.loads((ROOT / name).read_text(encoding="utf-8"))
                self.assertEqual(package["license"], SPDX)
        lock = json.loads((ROOT / "ui/package-lock.json").read_text(encoding="utf-8"))
        self.assertEqual(lock["packages"][""]["license"], SPDX)

    def test_the_complete_unmodified_gnu_licence_is_present(self):
        # Ignore checkout line-ending conversion, never changes to the legal text.
        text = (ROOT / "LICENSE").read_bytes().replace(b"\r\n", b"\n")
        self.assertEqual(
            hashlib.sha256(text).hexdigest(),
            "3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986",
        )
        self.assertFalse((ROOT / "LICENSE-MIT").exists())
        self.assertFalse((ROOT / "LICENSE-APACHE").exists())

    def test_the_installer_displays_and_installs_the_application_licence(self):
        bundle = self.config["bundle"]
        displayed = (ROOT / "src-tauri" / bundle["licenseFile"]).resolve()
        self.assertEqual(displayed, ROOT / "LICENSE")
        self.assertTrue(displayed.is_file())
        resources = bundle["resources"]
        installed = [source for source, target in resources.items() if target == "LICENSE"]
        self.assertEqual(len(installed), 1)
        self.assertEqual((ROOT / "src-tauri" / installed[0]).resolve(), displayed)
        self.assertIn("Sakatsu contributors", bundle["copyright"])
        self.assertEqual(resources["THIRD-PARTY-NOTICES.txt"], "THIRD-PARTY-NOTICES.txt")

    def test_public_guides_link_to_the_current_licence(self):
        for name in ("README.md", "LICENSING.md"):
            with self.subTest(guide=name):
                text = (ROOT / name).read_text(encoding="utf-8")
                self.assertIn(SPDX, text)
                self.assertIn("(./LICENSE)", text)
                self.assertNotIn("./LICENSE-MIT", text)
                self.assertNotIn("./LICENSE-APACHE", text)


if __name__ == "__main__":
    unittest.main()
