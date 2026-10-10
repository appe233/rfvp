#!/usr/bin/env python3
"""Run with uv run --no-project python experiments/plugin-v1/run.py.
Builds two independent guests and measures the unchanged standalone host.
All generated files stay under this experiment's ignored target/ directory.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "target" / "probe-results"


def command(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")


def make_pack(entries):
    names = bytearray()
    table = bytearray()
    payload = bytearray()
    offset = 8 + len(entries) * 12 + sum(len(name.encode()) + 1 for name, _ in entries)
    for name, data in entries:
        table.extend(struct.pack("<III", len(names), offset + len(payload), len(data)))
        names.extend(name.encode() + b"\0")
        payload.extend(data)
    return struct.pack("<II", len(entries), len(names)) + table + names + payload


def fixture(name, count):
    root = OUT / name
    root.mkdir(parents=True, exist_ok=True)
    strings = [{"offset": 6 + i * 8, "text": f"你好，合成测试对话 {i}"} for i in range(count)]
    write_json(root / "translations.json", strings)
    # Real FVP-shaped archive, synthetic bytes; the probe consumes supplied metadata.
    (root / "patch.bin").write_bytes(make_pack([
        ("menu", b"patched-menu"), ("ambiguous", b"skip"), ("unmatched", b"skip")]))
    script = b"synthetic HCB identity:" + str(count).encode()
    (root / "script.hcb").write_bytes(script)
    sources = []
    for source_id, filename, entries in [(1, "translations.json", []), (2, "patch.bin", ["menu", "ambiguous", "unmatched"])]:
        data = (root / filename).read_bytes()
        sources.append({"id": source_id, "path": filename, "len": len(data), "sha256": sha(data), "entries": entries})
    context = {"script_sha256": sha(script), "code_start": 4, "code_end": 8 + count * 8, "sources": sources,
               "originals": [{"path": "graph/menu", "entry": "menu"},
                             {"path": "graph/ambiguous", "entry": "ambiguous"},
                             {"path": "graph_vis/ambiguous", "entry": "ambiguous"}]}
    write_json(root / "context.json", context)
    return root, context


def measure(args, samples=11):
    elapsed = []
    reports = []
    # One warm-up; results describe warm filesystem runs, not cold launch.
    proc = subprocess.run(args, capture_output=True, text=True)
    if proc.returncode:
        raise RuntimeError(f"{' '.join(args)} failed:\n{proc.stderr}")
    for _ in range(samples):
        start = time.perf_counter_ns()
        proc = subprocess.run(args, check=True, capture_output=True, text=True)
        elapsed.append((time.perf_counter_ns() - start) / 1_000_000)
        reports.append(json.loads(proc.stdout))
    rss = None
    if platform.system() == "Darwin":
        proc = subprocess.run(["/usr/bin/time", "-l", *args], check=True, capture_output=True, text=True)
        match = re.search(r"(\d+)\s+maximum resident set size", proc.stderr)
        if match:
            rss = int(match.group(1))
    elif platform.system() == "Linux" and Path("/usr/bin/time").exists():
        proc = subprocess.run(["/usr/bin/time", "-v", *args], check=True, capture_output=True, text=True)
        match = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", proc.stderr)
        if match:
            rss = int(match.group(1)) * 1024
    return {"samples": samples, "process_ms_median": statistics.median(elapsed),
            "process_ms_max": max(elapsed), "peak_rss_bytes": rss}, reports


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    command("cargo", "build", "--release", "-p", "rfvp-plugin-probe", "--bins")
    suffix = ".exe" if os.name == "nt" else ""
    host = ROOT / "target" / "release" / f"probe{suffix}"
    baseline = ROOT / "target" / "release" / f"baseline{suffix}"
    host_before = sha(host.read_bytes())
    fixtures = [fixture("small", 2), fixture("large", 10_000)]
    hashes = [context["script_sha256"] for _, context in fixtures]
    packages = []
    for variant in ["normal", "alternate"]:
        args = ["cargo", "build", "--release", "-p", "rfvp-plugin-example", "--target", "wasm32-unknown-unknown"]
        if variant == "alternate":
            args += ["--features", "alternate"]
        command(*args)
        package = OUT / variant
        package.mkdir(exist_ok=True)
        shutil.copyfile(ROOT / "target/wasm32-unknown-unknown/release/rfvp_plugin_example.wasm", package / "plugin.wasm")
        write_json(package / "plugin.json", {"schema_version": 1, "id": f"example.{variant}", "version": "0.1.0",
                   "api_major": 1, "api_minor": 0, "capabilities": ["script-string-table-v1", "resource-mapping-v1"],
                   "module": "plugin.wasm", "supported_scripts": hashes, "inputs": ["translations.json", "patch.bin"], "claimed_archives": ["patch.bin"]})
        packages.append(package)
    assert sha(host.read_bytes()) == host_before, "host changed while building guests"
    results = {"platform": platform.platform(), "machine": platform.machine(),
               "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
               "host_sha256": host_before, "host_bytes": host.stat().st_size,
               "baseline_bytes": baseline.stat().st_size, "cases": []}
    results["loader_size_delta_bytes"] = results["host_bytes"] - results["baseline_bytes"]
    for root, context in fixtures:
        stats, _ = measure([str(baseline), str(root)])
        results["cases"].append({"fixture": root.name, "variant": "baseline", **stats})
        for package in packages:
            stats, reports = measure([str(host), str(package), str(root)])
            report = reports[-1]
            count = 2 if root.name == "small" else 10_000
            assert len(report["plan"]["strings"]) == count
            first = report["plan"]["strings"][0]
            assert first["offset"] == 6
            expected = "你好，合成测试对话 0" + (" [alternate plugin]" if package.name == "alternate" else "")
            assert first["text"] == expected
            assert report["plan"]["resources"] == [{"path": "graph/menu", "source_id": 2, "entry": "menu"}]
            results["cases"].append({"fixture": root.name, "variant": package.name, **stats,
                "prepare_us_median": statistics.median(r["elapsed_us"] for r in reports),
                **{key: report[key] for key in ["module_sha256", "module_bytes", "linear_memory_bytes", "fuel_consumed", "input_bytes_read"]}})
    assert sha(host.read_bytes()) == host_before
    write_json(OUT / "results.json", results)
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
