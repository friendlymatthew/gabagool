#!/usr/bin/env -S uv run --script
"""Download WebAssembly spec test suite (.wast files) from the official repo."""

import io
import os
import tarfile
import urllib.request

REPO = "WebAssembly/spec"
BRANCH = "main"
OUT = os.path.join(os.path.dirname(__file__), "gabagool", "tests", "spec")


def main():
    out = os.path.abspath(OUT)

    if os.path.isdir(out) and any(f.endswith(".wast") for f in os.listdir(out)):
        print(f"{out}/ already exists. Remove it first to re-download.")
        return

    os.makedirs(out, exist_ok=True)

    url = f"https://github.com/{REPO}/archive/refs/heads/{BRANCH}.tar.gz"
    print(f"Downloading spec tests from github.com/{REPO} ({BRANCH})...")

    with urllib.request.urlopen(url) as resp:
        data = io.BytesIO(resp.read())

    prefix = f"spec-{BRANCH}/test/core/"
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
