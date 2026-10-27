"""Verify the pinned dictionary corpus; download only with explicit --download.

Files stay in an explicit CI/cache directory, never the application profile or repository.
The original archive, including its attribution, is retained unchanged.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
from urllib.parse import urlsplit
from urllib.request import urlopen


def verify(path, specification):
    if path.stat().st_size != specification["bytes"]:
        raise ValueError("dictionary fixture has the wrong byte length")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    if digest.hexdigest() != specification["sha256"]:
        raise ValueError("dictionary fixture has the wrong SHA-256")


def prepare(destination, specification, download=False, opener=urlopen):
    name = specification["file"]
    if (not isinstance(name, str) or Path(name).name != name or "\\" in name
            or urlsplit(specification["url"]).scheme != "https"
            or not isinstance(specification["bytes"], int) or specification["bytes"] <= 0
            or not re.fullmatch(r"[0-9a-f]{64}", specification["sha256"])):
        raise ValueError("invalid pinned dictionary fixture metadata")
    destination = Path(destination)
    target = destination / name
    if target.exists():
        verify(target, specification)
        return target
    if not download:
        raise ValueError("dictionary fixture is absent; pass --download to fetch the pinned archive")
    destination.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination, suffix=".partial", delete=False) as output:
            temporary = Path(output.name)
            with opener(specification["url"], timeout=60) as response:
                if urlsplit(response.geturl()).scheme != "https":
                    raise ValueError("dictionary fixture redirected to a non-HTTPS URL")
                size = 0
                while chunk := response.read(1024 * 1024):
                    size += len(chunk)
                    if size > specification["bytes"]:
                        raise ValueError("dictionary fixture exceeded its pinned size")
                    output.write(chunk)
        verify(temporary, specification)
        os.replace(temporary, target)
        return target
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--download", action="store_true")
    args = parser.parse_args()
    specification = json.loads(Path(__file__).with_name("ci-test-data.json").read_text(encoding="utf-8"))
    try:
        prepare(args.destination, specification, args.download)
    except (OSError, ValueError) as error:
        parser.exit(1, f"CI dictionary fixture: {error}\n")
    print(f"Verified {specification['file']}: {specification['bytes']} bytes, SHA-256 {specification['sha256']}")
    print(f"{specification['attribution']} ({specification['licence']})")


if __name__ == "__main__":
    main()
