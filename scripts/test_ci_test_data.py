import hashlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("ci_test_data", Path(__file__).with_name("ci-test-data.py"))
data = importlib.util.module_from_spec(spec)
spec.loader.exec_module(data)


class Response(io.BytesIO):
    def geturl(self):
        return "https://example.invalid/pinned.zip"


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.payload = b"a pinned test archive"
        self.specification = {"file": "fixture.zip", "url": "https://example.invalid/pinned.zip",
                              "bytes": len(self.payload), "sha256": hashlib.sha256(self.payload).hexdigest()}

    def test_missing_requires_consent_and_verified_cache_needs_no_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            def no_network(*args, **kwargs):
                self.fail("cache validation must not contact the network")
            with self.assertRaisesRegex(ValueError, "absent"):
                data.prepare(directory, self.specification, opener=no_network)
            archive = data.prepare(directory, self.specification, True, lambda *a, **k: Response(self.payload))
            self.assertEqual(archive.read_bytes(), self.payload)
            self.assertEqual(data.prepare(directory, self.specification, True, no_network), archive)

    def test_corruption_and_oversize_never_publish_or_erase_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for payload in [self.payload + b"too much", b"x" * len(self.payload)]:
                with self.assertRaises(ValueError):
                    data.prepare(directory, self.specification, True, lambda *a, **k: Response(payload))
                self.assertEqual(list(directory.iterdir()), [])
            existing = directory / "fixture.zip"
            existing.write_bytes(b"invalid cached evidence")
            with self.assertRaises(ValueError):
                data.prepare(directory, self.specification, True)
            self.assertEqual(existing.read_bytes(), b"invalid cached evidence")

    def test_paths_and_insecure_urls_are_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            for change in [{"file": "../escape.zip"}, {"file": "..\\escape.zip"}, {"url": "http://example.invalid/data"}]:
                with self.assertRaisesRegex(ValueError, "metadata"):
                    data.prepare(Path(temporary), {**self.specification, **change}, True)


if __name__ == "__main__":
    unittest.main()
