#!/usr/bin/env python3
"""Build real guests; verify them through one fixed RFVP preparation executable.
Only synthetic scripts/archives are generated under target/plugin-conformance.
Run with: uv run --no-project python tools/plugin-conformance/run.py
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import zlib

ROOT = Path(__file__).resolve().parents[2]
# Honor CARGO_TARGET_DIR so a read-only source checkout can be validated (e.g. in a container).
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
OUT = TARGET / "plugin-conformance"


def run(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def sha(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def pack(path, data):
    path.write_bytes(struct.pack("<5I", 1, 5, 0, 25, len(data)) + b"menu\0" + data)


def package(path, module, script_hash, plugin_id, inputs, claims):
    path.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(module, path / "plugin.wasm")
    manifest = dict(schema_version=1, id=plugin_id, version="0.1.0", api_major=1,
                    api_minor=0, capabilities=["script-string-table-v1", "resource-mapping-v1"],
                    module="plugin.wasm", supported_scripts=[script_hash], inputs=inputs, claimed_archives=claims)
    (path / "plugin.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    game = OUT / "game"
    game.mkdir(exist_ok=True)
    script = bytes([10, 0, 0, 0, 0x0e, 3, 97, 98, 0, 0x42, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 72, 0, 0, 0, 0, 0])
    (game / "game.hcb").write_bytes(script)
    pack(game / "graph.bin", b"original")
    pack(game / "patch.bin", b"translated")
    text = "你好".encode("gbk")
    # Decoder format: u32 count followed by offset/u16 byte length/text records.
    payload = struct.pack("<IIH", 1, 6, len(text)) + text
    compressed = zlib.compress(payload)
    (game / "patch.dat").write_bytes(bytes(8) + struct.pack("<II", len(payload), len(compressed)) + compressed)
    plan = dict(strings=[dict(offset=6, text="synthetic")],
                resources=[dict(path="graph/menu", source_id=2, entry="menu")])
    (game / "plan.json").write_text(json.dumps(plan))
    run(["cargo", "build", "-p", "rfvp", "--example", "plugin_prepare_probe", "--no-default-features",
         "--features", "soft-render-core,no-audio,sakura-moyu,external-plugins"])
    host = TARGET / "debug/examples" / ("plugin_prepare_probe" + (".exe" if os.name == "nt" else ""))
    initial_host = sha(host)
    guests = TARGET / "wasm32-unknown-unknown/release"
    run(["cargo", "build", "-p", "rfvp-plugin-sakura-guest", "-p", "rfvp-plugin-conformance",
         "--target", "wasm32-unknown-unknown", "--release"])
    script_hash = hashlib.sha256(script).hexdigest()
    sakura = OUT / "sakura"
    normal = OUT / "normal"
    alternate = OUT / "alternate"
    package(sakura, guests / "rfvp_plugin_sakura_guest.wasm", script_hash, "rfvp.sakura-moyu-chs", ["patch.dat", "patch.bin"], ["patch.bin"])
    package(normal, guests / "rfvp_plugin_conformance.wasm", script_hash, "example.conformance", ["plan.json", "patch.bin"], ["patch.bin"])
    results = {}

    def probe(label, *options):
        assert sha(host) == initial_host, "host executable changed"
        output = run([str(host), str(game), *map(str, options)], capture_output=True, text=True)
        value = json.loads(output.stdout.strip().splitlines()[-1])
        results[label] = value
        return value

    disabled = probe("disabled")
    bundled = probe("bundled", "--bundled")
    external = probe("external-sakura", sakura)
    assert disabled["text"] == "ab"
    for value in [bundled, external]:
        assert value["text"] == "你好"
        assert value["address"] == 6 and value["cursor"] == 9
        assert value["resource_tail"] == list(b"ranslated")
    for key in ["text", "address", "cursor", "resource_tail", "resource_len"]:
        assert bundled[key] == external[key], key
    first = probe("normal", normal)
    assert first["text"] == "synthetic"
    # Rebuild only the guest after the RFVP executable is fixed.
    run(["cargo", "build", "-p", "rfvp-plugin-conformance", "--features", "alternate",
         "--target", "wasm32-unknown-unknown", "--release"])
    package(alternate, guests / "rfvp_plugin_conformance.wasm", script_hash, "example.conformance", ["plan.json", "patch.bin"], ["patch.bin"])
    second = probe("alternate", alternate)
    assert second["text"] == "synthetic [alternate plugin]"
    assert first["session"]["plugins"][0]["implementation_sha256"] != second["session"]["plugins"][0]["implementation_sha256"]
    assert sha(normal / "plugin.wasm") != sha(alternate / "plugin.wasm")
    # The FIRST plugin must see claims from LATER plugins removed already.
    claim_only = OUT / "claim-only"
    shutil.copyfile(game / "patch.bin", game / "other.bin")
    package(claim_only, normal / "plugin.wasm", script_hash, "example.claim-only", ["plan.json", "other.bin"], ["other.bin"])
    try:
        (game / "plan.json").write_text(json.dumps(dict(strings=[], resources=[])))
        union = probe("union-of-claims", sakura, claim_only)
        assert union["text"] == "你好"
        assert union["resource_tail"] == list(b"ranslated"), "later claim polluted original matching view"
        assert len(union["session"]["plugins"]) == 2
    finally:
        (game / "plan.json").write_text(json.dumps(plan))
        (game / "other.bin").unlink()
    assert sha(host) == initial_host
    report = dict(host_sha256=initial_host, platform=__import__("platform").platform(),
                  module_bytes={p.name: (p / "plugin.wasm").stat().st_size for p in [sakura, normal, alternate]},
                  runs=results)
    (OUT / "results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(dict(status="passed", host_sha256=initial_host, runs=len(results), report=str(OUT / "results.json"))))


if __name__ == "__main__":
    main()
