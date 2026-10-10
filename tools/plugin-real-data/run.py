#!/usr/bin/env python3
"""Compare Sakura preparation on authorized local game data; record digests only.

Builds release probes for unchanged main, the unchanged Sakura reference branch,
and the working tree, then compares every push_string instruction and every
original archive resource. Never writes to the game root. Run with:

  uv run --no-project python tools/plugin-real-data/run.py --game-root /path/to/game
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[2]
# Honor CARGO_TARGET_DIR so a read-only checkout and game root can be used (e.g. in a container).
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
BASE = TARGET / "plugin-real-data"
spec = importlib.util.spec_from_file_location("reference", REPO / "tools/plugin-reference/run.py")
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)
COMMON = ["soft-render-core", "no-audio", "zlib-flate2"]
FUEL_LINE = re.compile(r"plugin (\S+): (\d+) us, (\d+) fuel, (\d+) memory bytes, (\d+) transferred bytes")


def sha_file(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def build_probe(name, source, features, log_dir):
    project = BASE / "probes" / name
    (project / "src").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(Path(__file__).with_name("probe.rs"), project / "src/main.rs")
    rfvp_features = COMMON + (["sakura-moyu", "external-plugins"] if "current" in features else [])
    manifest = (f'[package]\nname = "rfvp-real-{name}"\nversion = "0.0.0"\nedition = "2021"\n'
                '[workspace]\n[dependencies]\nanyhow = "1"\nserde_json = "1"\nsha2 = "0.10"\nlog = "0.4"\n'
                f'rfvp = {{ path = {json.dumps(str(source / "crates/rfvp"))}, default-features = false, '
                f'features = {json.dumps(rfvp_features)} }}\n'
                '[features]\ncurrent = []\nreference-sakura = []\n')
    (project / "Cargo.toml").write_text(manifest)
    shutil.copyfile(source / "Cargo.lock", project / "Cargo.lock")
    # Versions are pinned by the seeded lockfile; set CARGO_NET_OFFLINE=true to forbid fetching.
    command = ["cargo", "build", "--release", "--manifest-path", str(project / "Cargo.toml"),
               "--target-dir", str(TARGET / "plugin-real-data/build" / name)]
    if features:
        command += ["--features", ",".join(features)]
    print(f"Building {name}", flush=True)
    with (log_dir / f"build-{name}.log").open("w") as log:
        subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT, check=True)
    # Separate target dirs: all three probes depend on a package named `rfvp`, and an
    # incremental rebuild in a shared target dir was observed to link the wrong revision.
    exe = TARGET / "plugin-real-data/build" / name / "release" / (f"rfvp-real-{name}" + (".exe" if os.name == "nt" else ""))
    return exe, rfvp_features


def peak_rss(stderr):
    # macOS `time -l` reports bytes; GNU `time -v` reports KiB.
    match = re.search(r"(\d+)\s+maximum resident set size", stderr)
    if match:
        return int(match.group(1))
    match = re.search(r"Maximum resident set size \(kbytes\): (\d+)", stderr)
    return int(match.group(1)) * 1024 if match else None


def execute(exe, args, log_path, timed=False):
    command = [str(exe), *map(str, args)]
    if timed:
        command = ["/usr/bin/time", "-l" if sys.platform == "darwin" else "-v", *command]
    started = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True)
    wall = time.perf_counter() - started
    log_path.write_text(result.stderr)
    if result.returncode != 0:
        return {"status": "error", "error": result.stderr.strip().splitlines()[-1:] if result.stderr else []}, wall, None
    value = json.loads(result.stdout.strip().splitlines()[-1])
    value["status"] = "ok"
    return value, wall, peak_rss(result.stderr) if timed else None


def runtime_reports(value):
    return [dict(zip(["plugin", "elapsed_us", "fuel", "memory_bytes", "transferred_bytes"],
                     [m.group(1), *map(int, m.groups()[1:])]))
            for line in value.get("log", []) if (m := FUEL_LINE.search(line))]


def compare(left, right, label):
    differences = []
    for field in ["strings", "resources"]:
        if len(left[field]) != len(right[field]):
            differences.append(f"{field}: count {len(left[field])} != {len(right[field])}")
            continue
        for index, (a, b) in enumerate(zip(left[field], right[field])):
            if field == "strings":
                a, b = a[:3], b[:3]  # replacement-hit flag is compared separately
            if a != b:
                differences.append(f"{field}[{index}]: {a} != {b}")
                if len(differences) > 20:
                    break
    if differences:
        raise SystemExit(f"FAIL {label}:\n" + "\n".join(differences[:20]))
    print(f"PASS {label}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game-root", type=Path, required=True)
    parser.add_argument("--nls", default="sjis", choices=["sjis", "gbk", "utf8"])
    parser.add_argument("--timing-runs", type=int, default=7)
    args = parser.parse_args()
    root = args.game_root.resolve()
    scripts = sorted(root.glob("*.hcb"))
    if not scripts or list(root.glob("*.bch")):
        raise SystemExit("game root must contain an .hcb script and no .bch (GPU discovery prefers .bch)")
    script_sha = sha_file(scripts[0])
    output = BASE / (time.strftime("run-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    output.mkdir(parents=True)
    # Reference worktrees are created by tools/plugin-reference/run.py in the checkout.
    refs = {"main": REPO / "target/plugin-reference/worktrees/main",
            "branch": REPO / "target/plugin-reference/worktrees/sakura"}
    commits = {"main": reference.worktree(REPO, refs["main"], reference.MAIN),
               "branch": reference.worktree(REPO, refs["branch"], reference.SAKURA)}
    identity = reference.source_identity(REPO)
    probes, builds = {}, {}
    for name, source, features in [("main", refs["main"], []), ("branch", refs["branch"], ["reference-sakura"]),
                                   ("current", REPO, ["current"])]:
        exe, rfvp_features = build_probe(name, source, features, output)
        probes[name] = exe
        builds[name] = {"rfvp_features": rfvp_features, "probe_features": features,
                        "executable_sha256": sha_file(exe)}
    fixed_host = builds["current"]["executable_sha256"]
    # The guest is built after the host executable is fixed; the host is never rebuilt below.
    subprocess.run(["cargo", "build", "-p", "rfvp-plugin-sakura-guest", "--target", "wasm32-unknown-unknown",
                    "--release"], cwd=REPO, check=True, capture_output=True)
    module = TARGET / "wasm32-unknown-unknown/release/rfvp_plugin_sakura_guest.wasm"
    package = output / "sakura-package"
    subprocess.run([sys.executable, str(REPO / "tools/plugin-conformance/package_sakura.py"), "--module", str(module),
                    "--output", str(package), "--script-sha256", script_sha], check=True, capture_output=True)
    listing, _, _ = execute(probes["main"], ["enumerate", root, args.nls], output / "enumerate.log")
    if listing["status"] != "ok":
        raise SystemExit(f"enumeration failed: {listing}")
    list_path = output / "list.json"
    list_path.write_text(json.dumps({k: listing[k] for k in ["string_starts", "paths"]}))
    runs = {"main-disabled": ("main", ["disabled"]), "branch-reference": ("branch", ["reference"]),
            "current-disabled": ("current", ["disabled"]), "current-bundled": ("current", ["bundled"]),
            "current-external": ("current", ["external", package])}
    observed = {}
    for label, (probe, mode) in runs.items():
        if probe == "current":
            assert sha_file(probes["current"]) == fixed_host, "host executable changed"
        print(f"Observing {label}", flush=True)
        value, wall, _ = execute(probes[probe], ["observe", root, args.nls, list_path, *mode], output / f"{label}.log")
        if value["status"] != "ok":
            raise SystemExit(f"FAIL {label}: {value['error']}")
        (output / f"{label}.json").write_text(json.dumps(value))
        observed[label] = value
    compare(observed["main-disabled"], observed["current-disabled"], "plugins disabled == main")
    compare(observed["branch-reference"], observed["current-bundled"], "bundled Sakura == reference branch")
    compare(observed["branch-reference"], observed["current-external"], "external Sakura == reference branch")
    hits = {k: v["replaced_strings"] for k, v in observed.items()}
    assert hits["branch-reference"] == hits["current-bundled"] == hits["current-external"], hits
    for label in ["branch-reference", "current-bundled", "current-external"]:
        flags = [s[3] for s in observed[label]["strings"]]
        assert flags == [s[3] for s in observed["branch-reference"]["strings"]], label
    disabled = observed["main-disabled"]
    patched = observed["branch-reference"]
    changed_strings = sum(a[2] != b[2] for a, b in zip(disabled["strings"], patched["strings"]))
    changed_resources = sum(a != b for a, b in zip(disabled["resources"], patched["resources"]))
    timings = {}
    for label, (probe, mode) in runs.items():
        samples = []
        for index in range(args.timing_runs):
            value, wall, rss = execute(probes[probe], ["prepare", root, args.nls, *mode],
                                       output / f"timing-{label}-{index}.log", timed=True)
            if value["status"] != "ok":
                raise SystemExit(f"FAIL timing {label}: {value['error']}")
            samples.append({"prepare_us": value["prepare_us"], "wall_ms": round(wall * 1000, 1),
                            "peak_rss_bytes": rss, "runtime": runtime_reports(value)})
        timings[label] = {"samples": samples,
                          "median_prepare_ms": statistics.median(s["prepare_us"] for s in samples) / 1000,
                          "median_wall_ms": statistics.median(s["wall_ms"] for s in samples),
                          "max_peak_rss_bytes": max(s["peak_rss_bytes"] or 0 for s in samples)}
    assert sha_file(probes["current"]) == fixed_host, "host executable changed"
    for name, path in refs.items():
        reference.worktree(REPO, path, commits[name])
    if reference.source_identity(REPO) != identity:
        raise SystemExit("implementation source changed during the run")
    inputs = {name: {"bytes": (root / name).stat().st_size, "sha256": sha_file(root / name)}
              for name in ["patch.dat", "patch.bin"]}
    report = {
        "status": "passed", "platform": platform.platform(), "machine": platform.machine(),
        "rustc": reference.run(["rustc", "-V"]), "references": commits,
        "implementation_source_sha256": identity["source_sha256"], "builds": builds,
        "fixed_host_sha256": fixed_host, "script": {"name": scripts[0].name, "sha256": script_sha,
                                                    "bytes": scripts[0].stat().st_size},
        "nls": args.nls, "inputs": inputs,
        "module": {"bytes": module.stat().st_size, "sha256": sha_file(package / "plugin.wasm")},
        "counts": {"instructions": listing["instructions"], "push_string": len(listing["string_starts"]),
                   "replaced_strings": hits["branch-reference"], "changed_string_text": changed_strings,
                   "resource_paths": len(listing["paths"]), "changed_resources": changed_resources,
                   "resource_bytes_hashed_per_run": disabled["resource_bytes"]},
        "sessions": {k: observed[k]["session"] for k in ["current-bundled", "current-external"]},
        "diagnostic_counts": {k: v["diagnostic_count"] for k, v in observed.items()},
        "timings": timings,
        "comparisons": ["main-disabled == current-disabled", "branch-reference == current-bundled",
                        "branch-reference == current-external"],
    }
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    summary = {k: {"median_prepare_ms": v["median_prepare_ms"], "max_peak_rss_mib": round(v["max_peak_rss_bytes"] / 2**20, 1)}
               for k, v in timings.items()}
    print(json.dumps({"counts": report["counts"], "timings": summary}, indent=2))
    print(f"PASS. Report: {output / 'report.json'}")


if __name__ == "__main__":
    main()
