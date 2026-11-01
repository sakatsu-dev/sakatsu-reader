import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location("essentials", Path(__file__).with_name("prepare-essentials.py"))
essentials = importlib.util.module_from_spec(spec)
spec.loader.exec_module(essentials)


class EssentialsTests(unittest.TestCase):
    def test_package_validation_and_non_overwrite(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "sources/dictionaries"
            source.mkdir(parents=True)
            data = b"pinned fixture bytes"
            (source / "fixture.zip").write_bytes(data)
            artifact = {"file": "fixture.zip", "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
            catalog = {"package_version": "fixture", "components": [{"id": "dictionaries", "licences": ["fixture"], "artifacts": [artifact]}]}
            output = root / "essentials.zip"
            essentials.prepare(root / "sources", output, catalog, None)
            expected = output.read_bytes()
            essentials.prepare(root / "sources", output, catalog, None)
            self.assertEqual(output.read_bytes(), expected)
            essentials.verify_package(output, catalog)
            with patch.object(essentials, "catalog_source_digest", return_value="changed"):
                with self.assertRaisesRegex(ValueError, "catalog source"):
                    essentials.verify_package(output, catalog)
            changed = json.loads(json.dumps(catalog))
            changed["package_version"] = "different"
            with self.assertRaisesRegex(ValueError, "release catalog"):
                essentials.prepare(root / "sources", output, changed, None)
            self.assertEqual(output.read_bytes(), expected)
            (source / "fixture.zip").write_bytes(b"x" * len(data))
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                essentials.prepare(root / "sources", root / "invalid.zip", catalog, None)
            self.assertFalse((root / "invalid.zip").exists())

    def test_missing_input_does_not_access_network(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(essentials.urllib.request, "urlopen", side_effect=AssertionError("network")):
            catalog = {"components": [{"id": "a", "artifacts": [{"file": "x", "bytes": 1, "sha256": "0" * 64}]}]}
            with self.assertRaises(ValueError):
                essentials.prepare(Path(temporary), Path(temporary) / "out", catalog, None)

    def test_package_refuses_extra_members(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "bad.zip"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("../outside", b"bad")
            with self.assertRaisesRegex(ValueError, "unexpected"):
                essentials.verify_package(path, {"components": []})


if __name__ == "__main__":
    unittest.main()
