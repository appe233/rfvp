#!/usr/bin/env python3
"""Cross-revision save compatibility in both directions.

1. Unchanged main writes a resumable V1 save; the working tree restores it.
2. The working tree writes a profiled save; main's readers and V1 decoder read it.

Run with uv; only Python's standard library is required. The main reference
worktree is shared with run.py; generated files stay under target/plugin-reference.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run import COMMON_FEATURES, MAIN, require, sha, source_identity, worktree  # noqa: E402

TEST_FEATURES = "soft-render-core,no-audio,sakura-moyu,external-plugins"
TEST_NAME = "preparation::tests::save_profile::save_profile_main_legacy_v1_fixture"
WRITE_TEST = "preparation::tests::save_profile::save_profile_write_profiled_fixture"


def main():
    repo = Path(__file__).resolve().parents[2]
    base = repo / "target/plugin-reference"
    output = base / (time.strftime("legacy-save-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    output.mkdir(parents=True)
    reference = base / "worktrees/main"
    commit = worktree(repo, reference, MAIN)

    project = base / "probes/legacy-save-main"
    (project / "src").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(Path(__file__).with_name("legacy_save.rs"), project / "src/main.rs")
    (project / "Cargo.toml").write_text(
        '[package]\nname = "rfvp-reference-legacy-save"\nversion = "0.0.0"\nedition = "2021"\n'
        '[workspace]\n[dependencies]\nanyhow = "1"\n'
        f'rfvp = {{ path = {json.dumps(str(reference / "crates/rfvp"))}, default-features = false, '
        f'features = {json.dumps(COMMON_FEATURES)} }}\n')
    shutil.copyfile(reference / "Cargo.lock", project / "Cargo.lock")
    print(f"Building main legacy-save probe (log: {output / 'build.log'})", flush=True)
    with (output / "build.log").open("w") as log:
        subprocess.run(["cargo", "build", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                        "--target-dir", str(repo / "target")], cwd=repo, stdout=log,
                       stderr=subprocess.STDOUT, check=True)
    exe = repo / "target/debug" / ("rfvp-reference-legacy-save" + (".exe" if os.name == "nt" else ""))

    empty = output / "empty-base"
    empty.mkdir()
    save = output / "legacy-v1.sav"
    subprocess.run([str(exe), "write", str(save), str(empty)], check=True, capture_output=True)
    data = save.read_bytes()
    require(data.endswith(b"RFVS"), "main did not write an RFVS state")
    require(b"RFPM" not in data, "main save unexpectedly contains a profile block")

    def cargo_test(name, env, log_name):
        command = ["cargo", "test", "-p", "rfvp", "--no-default-features", "--features", TEST_FEATURES,
                   "--lib", "--", "--ignored", "--exact", name]
        print(f"Running {name} (log: {output / log_name})", flush=True)
        with (output / log_name).open("w") as log:
            result = subprocess.run(command, cwd=repo, env=dict(os.environ, **env), stdout=log,
                                    stderr=subprocess.STDOUT)
        text = (output / log_name).read_text(encoding="utf-8")
        require(result.returncode == 0, f"{name} failed; see {output / log_name}")
        require("1 passed" in text, f"{name} did not run")

    cargo_test(TEST_NAME, {"RFVP_LEGACY_V1_SAVE": str(save)}, "restore-test.log")

    profiled = output / "profiled.sav"
    cargo_test(WRITE_TEST, {"RFVP_PROFILED_SAVE_OUT": str(profiled)}, "write-test.log")
    new_data = profiled.read_bytes()
    require(b"RFPM" in new_data and new_data.endswith(b"RFVS"), "working tree did not write a profiled save")
    observed = subprocess.run([str(exe), "read", str(profiled)], check=True, capture_output=True,
                              encoding="utf-8").stdout
    (output / "main-read.txt").write_text(observed, encoding="utf-8")
    expected = ("title=saved title\nitem_has_title=true\nthumb=[0, 0, 0, 0]\n"
                "const_string=\u4f60\u597d@6\ngraph5=graph/menu\n")
    require(observed == expected, f"main observed unexpected values:\n{observed}")

    identity = source_identity(repo)
    report = {
        "platform": {"system": platform.system(), "release": platform.release(), "machine": platform.machine()},
        "rustc": subprocess.check_output(["rustc", "--version"], encoding="utf-8").strip(),
        "main_commit": commit,
        "implementation_base_commit": identity["base_commit"],
        "implementation_source_sha256": identity["source_sha256"],
        "probe_sha256": sha(Path(__file__).with_name("legacy_save.rs").read_bytes()),
        "executable_sha256": sha(exe.read_bytes()),
        "main_save_bytes": len(data),
        "main_save_sha256": hashlib.sha256(data).hexdigest(),
        "profiled_save_bytes": len(new_data),
        "profiled_save_sha256": hashlib.sha256(new_data).hexdigest(),
        "main_observed_profiled_save": observed.splitlines(),
        "tests": [TEST_NAME, WRITE_TEST],
        "test_features": TEST_FEATURES,
        "result": "passed",
    }
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
