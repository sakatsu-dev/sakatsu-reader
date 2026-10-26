"""Fetch the pinned, test-only Japanese encoder. Never invoked by the application.

python scripts/fetch-reading-encoder.py --download
Apache-2.0: https://huggingface.co/tohoku-nlp/bert-base-japanese-v3
Only the safetensors weights and tokenizer metadata are fetched (~448 MB).
"""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
REPO = "tohoku-nlp/bert-base-japanese-v3"
REVISION = "01256ee45494fea11d9fc3d0eb2cee88d5340266"
FILES = {
    "model.safetensors": (447362824, "sha256", "fb55ed4648ebeba4b4cbf69d1b01a089a2c2b3ce68cad375b6afdf80f9e38649"),
    "config.json": (472, "git", "65a2f332234197516eacda2fd4e50f542d7ed71d"),
    "tokenizer_config.json": (251, "git", "dfbcc4690aaab4b44570ee05eb7fd09a10e5a7db"),
    "vocab.txt": (231026, "git", "421500f13b8d3fc8e962a4a9ecedf0d55ca14c89"),
    "README.md": (2659, "git", "630f3eca8d375fe0242a0b5c6d2686e2241a780f"),
}

def verify(path, expected):
    size, kind, digest = expected
    if not path.is_file() or path.stat().st_size != size:
        return False
    h = hashlib.sha256() if kind == "sha256" else hashlib.sha1(f"blob {size}\0".encode())
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest() == digest

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--download", action="store_true")
    args = parser.parse_args()
    dest = ROOT / "scratch/context-next/encoder"
    dest.mkdir(parents=True, exist_ok=True)
    for name, expected in FILES.items():
        path = dest / name
        if verify(path, expected):
            print(f"verified cached {name}", flush=True)
            continue
        if not args.download:
            raise SystemExit(f"Missing/unverified {name}; pass --download explicitly")
        temporary = path.with_suffix(path.suffix + ".part")
        with urllib.request.urlopen(f"https://huggingface.co/{REPO}/resolve/{REVISION}/{name}", timeout=60) as source, temporary.open("wb") as out:
            count = 0
            while chunk := source.read(1024 * 1024):
                count += len(chunk)
                if count > expected[0]:
                    raise ValueError("Transfer exceeded pinned file size")
                out.write(chunk)
        if not verify(temporary, expected):
            raise ValueError(f"Pinned size/hash mismatch: {name}")
        temporary.replace(path)
        print(f"verified {name}: {count} bytes", flush=True)
    (dest / "provenance.json").write_text(json.dumps({"repo":REPO,"revision":REVISION,"licence":"Apache-2.0","files":FILES}, indent=2), encoding="utf-8")

if __name__ == "__main__":
    main()
