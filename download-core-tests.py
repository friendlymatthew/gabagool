#!/usr/bin/env -S uv run --script
"""Download WebAssembly spec test suite (.wast files) from the official repo."""

import io
import os
import tarfile
import urllib.request

REPO = "WebAssembly/spec"
REVISION = "9fb3a59c0c8bc4481cb87cf9e652d510998d6798"
OUT = os.path.join(os.path.dirname(__file__), "tests", "spec")


def main():
    out = os.path.abspath(OUT)

    if os.path.isdir(out) and any(f.endswith(".wast") for f in os.listdir(out)):
        print(f"{out}/ already exists. Remove it first to re-download.")
        return

    os.makedirs(out, exist_ok=True)

    url = f"https://github.com/{REPO}/archive/{REVISION}.tar.gz"
    print(f"Downloading spec tests from github.com/{REPO} ({REVISION})...")

    with urllib.request.urlopen(url) as resp:
        data = io.BytesIO(resp.read())

    prefix = f"spec-{REVISION}/test/core/"
    count = 0

    with tarfile.open(fileobj=data, mode="r:gz") as tar:
        for member in tar.getmembers():
            if member.name.startswith(prefix) and member.name.endswith(".wast"):
                member.name = os.path.basename(member.name)
                tar.extract(member, path=out, filter="data")
                count += 1

    print(f"Downloaded {count} .wast files to tests/spec/")


if __name__ == "__main__":
    main()
