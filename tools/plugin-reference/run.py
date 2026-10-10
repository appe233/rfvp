#!/usr/bin/env python3
"""Execute one behavioral probe against unchanged references and the working tree.

Run with uv; only Python's standard library is required. Generated binaries,
fixtures, logs and detached worktrees stay under target/plugin-reference.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import time
import zlib

MAIN = "ffc7380"
SAKURA = "8f6458f"
COMMON_FEATURES = ["soft-render-core", "no-audio", "zlib-flate2"]


def run(args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def worktree(repo, path, revision):
    commit = run(["git", "rev-parse", revision], repo)
    if not path.exists():
        subprocess.run(["git", "worktree", "add", "--detach", str(path), commit], cwd=repo, check=True)
    if run(["git", "rev-parse", "HEAD"], path) != commit:
        raise RuntimeError(f"Unexpected revision in {path}")
    if run(["git", "status", "--porcelain", "--untracked-files=all"], path):
        raise RuntimeError(f"Reference worktree is not clean: {path}")
    return commit


def source_identity(repo):
    files = sorted(set(run(["git", "ls-files", "--cached", "--others", "--exclude-standard"], repo).splitlines()))
    hashes = {name: sha((repo / name).read_bytes()) for name in files
              if (name.startswith("crates/") or name in ("Cargo.toml", "Cargo.lock"))
              and (repo / name).is_file()}
    return {"base_commit": run(["git", "rev-parse", "HEAD"], repo),
            "source_sha256": sha(json.dumps(hashes, sort_keys=True).encode()),
            "files": hashes}


def pack(path, entries, encoding):
    names, offsets = bytearray(), []
    for name, _ in entries:
        offsets.append(len(names))
        names.extend(name.encode(encoding) + b"\0")
    offset = 8 + 12 * len(entries) + len(names)
    table = bytearray()
    for (_, data), name_offset in zip(entries, offsets):
        table.extend(struct.pack("<III", name_offset, offset, len(data)))
        offset += len(data)
    path.write_bytes(struct.pack("<II", len(entries), len(names)) + table + names
                     + b"".join(data for _, data in entries))


def patch(records):
    payload = struct.pack("<I", len(records))
    for address, text in records:
        payload += struct.pack("<IH", address, len(text)) + text
    compressed = zlib.compress(payload)
    return bytes.fromhex("00000000cf4e4263") + struct.pack("<II", len(payload), len(compressed)) + compressed


def fixture(root, nls, variant="valid"):
    root.mkdir(parents=True)
    encoding = {"sjis": "shift_jis", "gbk": "gbk", "utf8": "utf-8"}[nls]
    starts, addresses, cursors = [], [], []
    code = bytearray()
    originals = ["あ", "empty-original", "fallback"]
    for text in originals:
        starts.append(4 + len(code))
        addresses.append(6 + len(code))
        payload = text.encode(encoding) + b"\0"
        code.extend(bytes([0x0e, len(payload)]) + payload)
        cursors.append(4 + len(code))
        code.append(0x42)
    title = "題名".encode(encoding) + b"\0"
    syscall = b"TextPrint\0"
    metadata = (struct.pack("<IHHBBB", 4, 0, 0, 0, 0, len(title)) + title
                + struct.pack("<HBB", 1, 1, len(syscall)) + syscall + struct.pack("<H", 0))
    (root / "script.hcb").write_bytes(struct.pack("<I", 4 + len(code)) + code + metadata)
    records = [(addresses[0], "更长的中文翻译".encode("gbk") + b"\0"), (addresses[1], b"\0")]
    dat = patch(records)
    if variant == "duplicate":
        dat = patch([records[0], records[0]])
    elif variant == "remaining-nul":
        dat = patch([(addresses[0], b"text\0\0")])
    elif variant == "bad-zlib":
        dat = dat[:16] + b"\xff" + dat[17:]
    elif variant == "bad-size":
        dat = dat[:12] + struct.pack("<I", 999) + dat[16:]
    elif variant == "truncated-record":
        payload = struct.pack("<II", 1, addresses[0])
        compressed = zlib.compress(payload)
        dat = bytes(8) + struct.pack("<II", len(payload), len(compressed)) + compressed
    if variant != "missing-dat":
        (root / "patch.dat").write_bytes(dat)
    pack(root / "graph.bin", [("menu_bg", b"normal-menu"), ("plain", b"normal-plain"),
                              ("ambiguous", b"graph-ambiguous"), ("Menu名", b"normal-case")], encoding)
    pack(root / "graph_vis.bin", [("title_logo1", b"normal-logo"), ("ambiguous", b"vis-ambiguous")], encoding)
    if variant != "missing-bin":
        pack(root / "patch.bin", [("menu_bg", b"patched-menu"), ("title_logo1", b"patched-logo"),
                                  ("ambiguous", b"patched-ambiguous"), ("unmatched", b"patched-unmatched"),
                                  ("Menu名", b"patched-case")], encoding)
    (root / "case.json").write_text(json.dumps({"instruction_starts": starts}))
    return {"originals": originals, "addresses": addresses, "cursors": cursors, "code_end": 4 + len(code)}


def differences(left, right, path=""):
    if isinstance(left, dict) and isinstance(right, dict):
        result = []
        for key in sorted(left.keys() | right.keys()):
            if key not in left or key not in right:
                result.append(path + "/" + key)
            else:
                result.extend(differences(left[key], right[key], path + "/" + key))
        return result
    return [] if left == right else [path]


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def ok_bytes(value):
    require(value.get("status") == "ok", f"Expected successful read: {value}")
    return bytes(value["bytes"])


def validate(transcript, expected, enabled, reference, case_sensitive):
    require(transcript["preparation"] == "ok", "Valid fixture failed preparation")
    texts = ["更长的中文翻译", "", "fallback"] if enabled else expected["originals"]
    for i, string in enumerate(transcript["strings"]):
        require(string == {"text": texts[i], "original": expected["originals"][i],
                           "address": expected["addresses"][i], "cursor": expected["cursors"][i],
                           "next_opcode": 0x42}, f"Unexpected string result: {string}")
    require(len(transcript["strings"]) == 3, "Missing string observations")
    require(transcript["metadata"]["title"] == "題名", "Title changed")
    require(transcript["metadata"]["syscalls"] == [{"id": 0, "args": 1, "name": "TextPrint"}], "Syscall changed")
    require(transcript["metadata"]["code_end"] == expected["code_end"], "Code range changed")
    require(transcript["bounds"] == {"rejected": True, "cursor": 6}, "Original bounds check changed")
    menu = b"patched-menu" if enabled else b"normal-menu"
    require(ok_bytes(transcript["baseline"]["graph/menu_bg"]) == menu, "Wrong archive selection")
    require(ok_bytes(transcript["source_loose"]["graph/menu_bg"]) == menu, "Source loose file intercepted mapping")
    require(ok_bytes(transcript["baseline"]["graph/plain"]) == b"normal-plain", "Original fallback failed")
    require(ok_bytes(transcript["baseline"]["graph/ambiguous"]) == b"graph-ambiguous", "Ambiguous name mapped")
    require(ok_bytes(transcript["baseline"]["graph_vis/ambiguous"]) == b"vis-ambiguous", "Ambiguous name mapped")
    require(transcript["baseline"]["graph/unmatched"] == {"status": "error"}, "Unmatched name mapped")
    require(transcript["baseline"]["graph/menu名"] == {"status": "error"}, "Entry case was folded")
    require(ok_bytes(transcript["baseline"]["graph/Menu名"]) == (b"patched-case" if enabled else b"normal-case"), "Unicode entry mismatch")
    require(ok_bytes(transcript["logical_loose"]["graph/menu_bg"]) == b"logical-loose", "Requested loose file lost")
    mixed = menu if case_sensitive and enabled and reference else b"logical-loose"
    require(ok_bytes(transcript["logical_loose"]["GRAPH/menu_bg"]) == mixed, "Mixed-case lookup mismatch")
    if case_sensitive:
        require(ok_bytes(transcript["distinct_case"]["GRAPH/menu_bg"]) == b"requested-case-loose", "Requested-case precedence failed")
        require(ok_bytes(transcript["distinct_case"]["graph/menu_bg"]) == b"logical-loose", "Distinct loose paths collapsed")
    else:
        require(transcript["distinct_case"] is None, "Incorrect case-sensitivity detection")
    for key, data in [("mapped_or_original", menu), ("original", b"normal-plain")]:
        require(transcript["streams"][key] == {"length": len(data), "prefix": list(data[:4]),
            "tail": list(data[-4:]), "end_pos": len(data)-4, "eof_read": 0,
            "past_end": len(data), "before_start": 0, "independent_first": list(data[:4]),
            "independent_second": list(data[2:6])}, f"Stream semantics changed: {key}")
    if enabled:
        require(transcript["source_failure"]["mapped"] == {"status": "error"}, "Mapped failure fell back")
    else:
        require(ok_bytes(transcript["source_failure"]["mapped"]) == b"normal-menu", "Original source lost")
    require(ok_bytes(transcript["source_failure"]["original"]) == b"normal-plain", "Unrelated source lost")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture-root", type=Path, help="Fixture parent, preferably a case-sensitive volume")
    parser.add_argument("--require-case-sensitive", action="store_true", help="Fail instead of skipping distinct-case paths")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    base = repo / "target/plugin-reference"
    base.mkdir(parents=True, exist_ok=True)
    output = base / (time.strftime("run-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    output.mkdir()
    fixture_parent = (args.fixture_root or output / "fixtures").resolve() / output.name
    fixture_parent.mkdir(parents=True)
    marker = fixture_parent / "case-probe"
    marker.write_text("case")
    case_sensitive = not (fixture_parent / "CASE-PROBE").exists()
    marker.unlink()
    require(case_sensitive or not args.require_case_sensitive, "Fixture filesystem is case-insensitive")
    references = {"main": base / "worktrees/main", "branch": base / "worktrees/sakura"}
    commits = {"main": worktree(repo, references["main"], MAIN),
               "branch": worktree(repo, references["branch"], SAKURA)}
    identity = source_identity(repo)
    probe = Path(__file__).with_name("probe.rs")
    profiles = {"main": (references["main"], None), "branch": (references["branch"], "reference-sakura"),
                "new-disabled": (repo, None), "new-enabled": (repo, "new-sakura")}
    executables, build_records = {}, {}
    for name, (source, feature) in profiles.items():
        project = base / "probes" / name
        (project / "src").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(probe, project / "src/main.rs")
        features = "[features]\nreference-sakura = []\n"
        if source == repo:
            features += 'new-sakura = ["rfvp/sakura-moyu"]\n'
        else:
            features += 'new-sakura = []\n'
        manifest = (f'[package]\nname = "rfvp-reference-{name}"\nversion = "0.0.0"\nedition = "2021"\n'
                    '[workspace]\n[dependencies]\nanyhow = "1"\nserde_json = "1"\n'
                    f'rfvp = {{ path = {json.dumps(str(source / "crates/rfvp"))}, default-features = false, '
                    f'features = {json.dumps(COMMON_FEATURES)} }}\n' + features)
        (project / "Cargo.toml").write_text(manifest)
        # Seed dependency versions from the source revision, not latest registry versions.
        shutil.copyfile(source / "Cargo.lock", project / "Cargo.lock")
        command = ["cargo", "build", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                   "--target-dir", str(repo / "target")]
        if feature:
            command += ["--features", feature]
        print(f"Building {name} (log: {output / (name + '-build.log')})", flush=True)
        with (output / (name + "-build.log")).open("w") as log:
            subprocess.run(command, cwd=repo, stdout=log, stderr=subprocess.STDOUT, check=True)
        exe = repo / "target/debug" / ("rfvp-reference-" + name + (".exe" if os.name == "nt" else ""))
        executables[name] = exe
        build_records[name] = {"features": COMMON_FEATURES + (["sakura-moyu"] if feature == "new-sakura" else []),
                               "adapter_feature": feature, "lock_sha256": sha((project / "Cargo.lock").read_bytes()),
                               "executable_sha256": sha(exe.read_bytes())}
    transcripts, comparisons, exceptions, validations = {}, [], [], []

    def observe(profile, nls, enabled, variant="valid"):
        key = f"{profile}-{nls}-{'enabled' if enabled else 'disabled'}-{variant}"
        root = fixture_parent / key
        expected = fixture(root, nls, variant)
        # Record fixture bytes before the shared probe mutates loose/source files.
        fixtures = {p.name: sha(p.read_bytes()) for p in sorted(root.iterdir()) if p.is_file()}
        command = [str(executables[profile]), str(root), "enabled" if enabled else "disabled", nls]
        result = subprocess.run(command, capture_output=True, text=True, check=True)
        (output / (key + ".stderr.log")).write_text(result.stderr)
        value = json.loads(result.stdout)
        (output / (key + ".json")).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
        transcripts[key] = {"fixture_sha256": fixtures, "transcript_sha256": sha(result.stdout.encode()), "result": value}
        return key, value, expected

    for nls in ["sjis", "gbk", "utf8"]:
        observations = {}
        for profile, enabled in [("main", False), ("branch", True), ("new-disabled", False),
                                 ("new-enabled", True), ("new-enabled", False)]:
            key, value, expected = observe(profile, nls, enabled)
            validate(value, expected, enabled, profile == "branch", case_sensitive)
            observations[(profile, enabled)] = (key, value)
        for profile in ["new-disabled", "new-enabled"]:
            left_key, left = observations[("main", False)]
            right_key, right = observations[(profile, False)]
            require(not differences(left, right), f"Disabled mismatch {nls}: {differences(left, right)}")
            require(transcripts[left_key]["fixture_sha256"] == transcripts[right_key]["fixture_sha256"], "Fixtures differ")
            comparisons.append({"left": left_key, "right": right_key, "result": "identical"})
        left_key, left = observations[("branch", True)]
        right_key, right = observations[("new-enabled", True)]
        expected_diff = ["/logical_loose/GRAPH/menu_bg/bytes"] if case_sensitive else []
        actual_diff = differences(left, right)
        require(actual_diff == expected_diff, f"Enabled mismatch {nls}: {actual_diff}")
        require(transcripts[left_key]["fixture_sha256"] == transcripts[right_key]["fixture_sha256"], "Fixtures differ")
        main_value = observations[("main", False)][1]
        require(right["logical_loose"]["GRAPH/menu_bg"] == main_value["logical_loose"]["GRAPH/menu_bg"], "Correction does not match main")
        comparisons.append({"left": left_key, "right": right_key, "result": "identical_except_documented_precedence", "differences": actual_diff})
        if actual_diff:
            exceptions.append({"nls": nls, "path": actual_diff[0], "branch": "patched-menu", "new_and_main": "logical-loose"})
        print(f"PASS {nls}: disabled equality, enabled equality, explicit precedence policy", flush=True)
    for variant in ["missing-dat", "missing-bin", "bad-zlib", "bad-size", "duplicate", "truncated-record"]:
        for profile in ["branch", "new-enabled"]:
            key, value, _ = observe(profile, "sjis", True, variant)
            require(value == {"preparation": "error"}, f"Expected rejection: {key}")
            validations.append({"case": key, "result": "rejected"})
    # This is intentionally stricter validation, not a valid-input equivalence case.
    _, old, _ = observe("branch", "sjis", True, "remaining-nul")
    _, new, _ = observe("new-enabled", "sjis", True, "remaining-nul")
    require(old["strings"][0]["text"] == "text\0", "Reference trailing-NUL behavior changed")
    require(new == {"preparation": "error"}, "New adapter must reject remaining NUL")
    validations.append({"case": "remaining-nul", "branch": "accepted text with NUL", "new": "rejected", "intentional": True})
    for name, path in references.items():
        worktree(repo, path, commits[name])
    require(source_identity(repo) == identity, "Implementation source changed while comparisons ran")
    report = {"status": "passed", "platform": platform.platform(), "machine": platform.machine(),
              "rustc": run(["rustc", "-Vv"]), "python": sys.version, "references": commits,
              "implementation": identity, "probe_sha256": sha(probe.read_bytes()),
              "runner_sha256": sha(Path(__file__).read_bytes()), "builds": build_records,
              "case_sensitive": case_sensitive, "comparisons": comparisons,
              "documented_exceptions": exceptions, "validation_cases": validations,
              "transcripts": transcripts, "reference_worktrees_clean": True}
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"PASS: {len(transcripts)} process runs, {len(comparisons)} comparisons; case-sensitive={case_sensitive}\nReport: {output / 'report.json'}", flush=True)


if __name__ == "__main__":
    main()
