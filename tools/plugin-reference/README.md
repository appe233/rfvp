# Plugin reference comparison

This runner executes the same synthetic behavioral probe against main `ffc7380`, the Sakura branch `8f6458f`, and the current implementation. It uses standalone Rust programs with path dependencies on RFVP; it does not modify reference production code or inject test modules.

Run from the repository root:

```sh
uv run --no-project python tools/plugin-reference/run.py \
  --fixture-root /path/on/a/case-sensitive/filesystem \
  --require-case-sensitive
```

Without `--fixture-root`, fixtures are generated under `target/plugin-reference`. Without `--require-case-sensitive`, the runner can execute on a case-insensitive filesystem, but distinct-case loose-file checks are skipped and that limitation is recorded. A full Change 1 comparison requires a case-sensitive run.

On macOS, a temporary disk image can provide a case-sensitive fixture filesystem:

```sh
mkdir -p target/plugin-reference/mount
hdiutil create -size 64m -type SPARSE -fs HFSX \
  -volname RFVPPluginComparison target/plugin-reference/case-sensitive.sparseimage
hdiutil attach -nobrowse -mountpoint "$PWD/target/plugin-reference/mount" \
  target/plugin-reference/case-sensitive.sparseimage
uv run --no-project python tools/plugin-reference/run.py \
  --fixture-root target/plugin-reference/mount --require-case-sensitive
hdiutil detach "$PWD/target/plugin-reference/mount"
```

Use a new image path if one already exists. The runner does not mount or unmount disks itself. Generated fixtures remain inside the image after detachment; the report and transcripts remain in the host worktree's `target/plugin-reference/run-*/` directory.

## What runs

The runner creates clean detached reference worktrees under `target/plugin-reference/worktrees/`, verifies their full commit IDs, and refuses to use dirty or unexpected references. It builds four probes, with dependency versions seeded from each revision's Cargo.lock and offline dependency resolution:

- Main, with `soft-render-core,no-audio,zlib-flate2`.
- Sakura reference, with the same RFVP features and a probe-only adapter for its static patch APIs.
- Current RFVP without the optional Sakura feature.
- Current RFVP with `sakura-moyu`, using `plugins::prepare_sakura`.

For each of Shift-JIS, GBK, and UTF-8, five fresh processes run byte-identical generated fixtures: main disabled; branch enabled; current disabled without Sakura compiled in; current enabled; current disabled with Sakura compiled in. All revisions use the script NLS for archive entry names, while patch.dat strings are independently encoded as GBK.

The probe records original and effective strings, payload addresses, cursor/next opcode, title, syscall names, original bounds failures, resource bytes/errors, archive lengths, read/seek/EOF results, independent streams, source-folder loose overrides, logical loose overrides, distinct-case loose overrides, and source deletion failures. Each output is also checked against explicit fixture expectations, preventing two equally wrong implementations from passing merely by agreeing.

Disabled outputs must exactly equal main. Enabled outputs must exactly equal the Sakura branch except for one explicitly asserted field on a case-sensitive filesystem: `logical_loose/GRAPH/menu_bg/bytes`. The branch returns mapped archive bytes before checking `graph/menu_bg`; current RFVP must return the same loose bytes as main. No arbitrary differences are ignored.

Missing required files, corrupt zlib, incorrect compressed size, duplicate offsets, and truncated records are independently required to reject in both Sakura implementations. One remaining NUL after stripping a trailing NUL is recorded as an intentional validation difference: the branch accepts it, while the new API rejects it. These are outside valid-input equivalence.

A successful run reports 29 processes and nine direct transcript comparisons. The machine-readable report includes platform/toolchain, full reference commits, implementation source fingerprint, per-file source hashes, probe/runner/executable/lockfile hashes, fixture hashes, transcripts, explicit exceptions, and reference-worktree cleanliness. Any unexpected difference, build failure, changed implementation source during execution, or missing required case-sensitive coverage causes a nonzero exit. Build logs and individual transcripts remain available even if a run fails.

## Scope and cleanup

Only synthetic game data is generated. This suite compares preparation, parser/VM string behavior, and native streaming VFS behavior; it does not exercise presentation, audio playback, launch-option discovery, external Wasm packages, or save-profile compatibility. Real-game and cross-platform release checks remain separate gates.

The reference worktrees and Cargo artifacts are retained for inspection and faster reruns. To remove the reference worktrees, use `git worktree remove` on each path under `target/plugin-reference/worktrees/` before deleting their parent directory. Do not run the comparison concurrently against the same checkout: generated probe projects and build artifacts are shared.

## Cross-revision save compatibility

`legacy_save.py` checks Change 4's save placement against unchanged main in both directions:

```sh
uv run --no-project python tools/plugin-reference/legacy_save.py
```

It reuses the main reference worktree, builds `legacy_save.rs` against main's public API with `soft-render-core,no-audio,zlib-flate2`, and then:

1. Main writes a resumable V1 save containing a serialized `ConstString` and a VFS-backed graph. The ignored working-tree test `save_profile_main_legacy_v1_fixture` restores it in plugin-disabled and Sakura sessions through `VmRunner::load_slot_from_bytes` as an unknown profile.
2. The ignored working-tree test `save_profile_write_profiled_fixture` writes a Sakura-profiled save. Main's header, slot-cache, and thumbnail readers and its V1 suffix decoder must observe the expected title, thumbnail, string, and graph path.

The tests are ignored in ordinary runs because they receive file paths from the runner (`RFVP_LEGACY_V1_SAVE`, `RFVP_PROFILED_SAVE_OUT`). Saves, logs, and `report.json` are written under `target/plugin-reference/legacy-save-*/`. Only synthetic data is used.
