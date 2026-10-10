#!/usr/bin/env python3
"""Create a candidate Sakura package from a built module and explicit script hashes.
This tool does not certify hashes; use only identities verified by the package author.
"""
import argparse
import json
from pathlib import Path
import re
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--module", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--script-sha256", action="append", required=True)
    args = parser.parse_args()
    if not 1 <= len(args.script_sha256) <= 512 or any(not re.fullmatch("[0-9a-f]{64}", h) for h in args.script_sha256):
        parser.error("provide 1–512 lowercase SHA-256 script identities")
    if args.module.stat().st_size > 8 * 1024 * 1024:
        parser.error("module exceeds 8 MiB")
    with args.module.open("rb") as source:
        if source.read(4) != b"\0asm":
            parser.error("module must be a Wasm binary")
    args.output.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(args.module, args.output / "plugin.wasm")
    manifest = dict(schema_version=1, id="rfvp.sakura-moyu-chs", version="0.1.0",
                    api_major=1, api_minor=0, capabilities=["script-string-table-v1", "resource-mapping-v1"],
                    module="plugin.wasm", supported_scripts=sorted(set(args.script_sha256)),
                    inputs=["patch.dat", "patch.bin"], claimed_archives=["patch.bin"])
    (args.output / "plugin.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
