#!/usr/bin/env python3
import io
import sys
import zipfile
from pathlib import Path
from urllib.request import urlopen

VERSION = "3.13.12"
WASI_SDK = "24"
URL = f"https://github.com/brettcannon/cpython-wasi-build/releases/download/v{VERSION}/python-{VERSION}-wasi_sdk-{WASI_SDK}.zip"

script_dir = Path(__file__).resolve().parent

if (script_dir / "python.wasm").exists() and (script_dir / "lib").exists():
    sys.exit(0)

with urlopen(URL) as resp:
    data = resp.read()

with zipfile.ZipFile(io.BytesIO(data)) as zf:
    zf.extractall(script_dir)
