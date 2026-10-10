# Changes 2–3: transactional preparation and external packages

Branch `feat/plugin-data`, continuing the Change 1/2 changes based on `ffc7380`. This completes Change 2's implementation and the external execution/package portion of Change 3. ABI 1.0 is still a candidate; real-game/cross-platform release verification and save-profile enforcement are not complete.

## Implemented

- `rfvp-plugin-sdk` defines strict manifest/context/plan wire types, count-bounded array deserializers, bounded JSON serialization, shared plan validation, and guest import helpers. Unknown/duplicate fields, invalid types, unsupported capabilities/API, wrong script hashes, bad relative paths, invalid source references, and duplicate canonical plan keys are errors.
- `preparation/session.rs` preflights the entire activation list before any guest execution. It collects duplicate IDs and lexical claims, opens declared inputs, detects aliased claims using opened-file identities, hashes streams, assigns 1..N source handles in input order, and builds the original archive view after removing the union of claims. Both archive entry lists and logical original paths are sorted. Module/input/metadata failures precede execution.
- Successful plans remain private to the preparation transaction. Host-owned mappings remap per-plugin source IDs to session source IDs. Later activation wins with both plugin IDs in conflict diagnostics; aggregate submitted plan bytes are capped at 64 MiB. Only after every plugin succeeds are the merged tables attached to the parser/VFS and returned with session identity. A later trap never changes an earlier, already running/prepared session.
- `--plugin` is functional with the explicit `external-plugins` feature across shared GPU/library/pump/software preparation. `sakura-moyu` remains independent. An explicitly activated `rfvp.sakura-moyu-chs` package suppresses only the bundled alias, retains its requested position, and still requires a supported script hash. Otherwise the bundled alias is appended after the explicit list. Duplicate explicit IDs are rejected.
- `rfvp-plugin-host` provides typed plugin/stage errors, canonical implementation identity, confined filesystem opens, stable source handles, and an optional interpreter. The Wasmi adapter pins version 2.0.0 with validation and strict compilation limits, four bounded imports, required typed exports, version-query import rejection, start/WASI rejection, one wasm32 memory, one bounded table, fuel and copy quotas, exact reads, and one tentative submission. Guest state is destroyed after output validation.
- Production sources are opened relative to directory capabilities using `cap-std`, checked as regular files, and retained rather than reopened by pathname. Source equality handles symlink/hardlink/case aliases. Independently positioned slices share a short-lived file seek/read lock, never the GameData lock. Hashing uses a 64-KiB buffer and guest reads transfer at most 1 MiB at a time. Large archives are not buffered wholesale. A 100-MiB sparse source is exercised without increasing the 64-MiB guest-copy limit.
- Plugin-enabled archive preparation bounds raw metadata, record/name counts, and decoded names before retaining metadata collections. Archive entry ranges are checked against file length. Original and mapped archives keep the prepared file handle alive, so pathname replacement does not replace the source. In-place file edits/truncation remain unsupported and read errors propagate.
- The portable API now distinguishes per-plan limits from session-merged table limits; a valid combination can exceed 100,000 records while each guest remains below the per-array limit. The 64-MiB aggregate submitted-plan quota remains authoritative.
- `rfvp-plugin-sakura-guest` runs the actual GBK/zlib decoder and unique-name matcher through the same context/plan contract as the bundled adapter. `rfvp-plugin-conformance` provides independently compiled normal/alternate synthetic guests. Neither receives a filesystem API or WASI.
- `tools/plugin-conformance/run.py` generates only synthetic game/patch data and packages. `package_sakura.py` creates an external Sakura package from a built module plus explicit author-verified script hashes, without copying game assets. Build/API/error guidance is in `tools/plugin-conformance/README.md`.

## Candidate policy additions and behavioral differences

The ABI signatures and wire fields are unchanged. Additional finite host policy is now explicit: 256 inputs/claims and 512 supported-script hashes per manifest; 1,024 archive metadata parses; 100,000 archive entry records and 100,000 filename-table records across preparation; 8 MiB aggregate metadata accounting. Accounting includes raw headers/tables, decoded retained names, and original logical paths, and can reject before the separate serialized-context limit. JSON escaping is counted during bounded serialization. Conflict-detail diagnostics are capped at 1 MiB, followed by a suppressed-count summary.

Plugin-disabled VFS scanning is unchanged. When plugins are enabled, malformed/unopenable discovered `.bin` archives or exceeded metadata budgets fail preparation instead of being silently skipped. This is an intentional stricter preflight. The static Change 1 compatibility helper `plugins::prepare_sakura` remains for the reference-comparison harness; desktop startup uses the new transaction and stable-source service.

Static implementation identity still includes build-generated source inputs and the workspace lockfile; unrelated lockfile changes conservatively change it. External identity includes the exact executed module digest and canonical normalized manifest semantics. Input hashes describe bytes observed during preparation, not a tamper-proof snapshot. Guest memory/fuel do not impose a host RSS cap or a real-time deadline.

## Fixed-host evidence

Command:

```sh
uv run --no-project python tools/plugin-conformance/run.py
```

Observed on macOS 15.8 / arm64. One headless RFVP executable was built before the independent guest rebuild and verified unchanged before/after every run:

```text
b6088b03619eea226e870a2a75c1ea33c9f7b40120ff82470fc3bd6c9df848b9
```

Six process runs passed: disabled, bundled Sakura, external Sakura, normal conformance, independently rebuilt alternate conformance, and the union-of-claims case where a later plugin claims an otherwise ambiguous entry.

Bundled/external Sakura both produced `你好`, retained ConstString address 6 and cursor 9, and sought/read the same translated resource bytes. The alternate guest changed the output to `synthetic [alternate plugin]` with a different implementation digest and without changing RFVP. The first plugin's matching excluded a later plugin's claimed archive.

Module sizes: Sakura 435,914 bytes; conformance normal 248,644 bytes; alternate 248,803 bytes. The recorded single debug-host preparation samples were approximately 110 ms for external Sakura and 59–61 ms for conformance. These include preparation/compilation for tiny synthetic input and are not release performance claims, representative archive benchmarks, or RSS measurements. Machine-readable generated evidence is in `target/plugin-conformance/results.json`.

The headless probe calls the production RFVP preparation/parser/VFS code, but does not create a game window, play audio, restore saves, or establish authorized real-game equivalence.

## Verification

All focused tests below pass; existing compiler warnings remain in the engine. No proprietary game data was used or added.

| Check | Result |
| --- | --- |
| API + SDK + Sakura + host tests (`cargo test -p rfvp-plugin-host --features wasmi-runtime -p rfvp-plugin-sdk -p rfvp-plugin-api -p rfvp-plugin-sakura`) | 27 passed: 2 API, 3 SDK, 5 Sakura, 17 host |
| Preparation tests with `soft-render-core,no-audio,sakura-moyu,external-plugins` | 15 passed; also passed three consecutive parallel-test runs |
| Preparation tests with bundled-only `soft-render-core,no-audio,sakura-moyu` | 10 passed |
| Preparation tests with backend-only `soft-render-core,no-audio,external-plugins` | 11 passed |
| Preparation tests with `soft-render-core,no-audio,desktop-preparation` | 9 passed, including unavailable-backend and unavailable-bundled rejection |
| GPU library FFI options test with `gpu-render,no-audio,sakura-moyu,external-plugins` | Passed without creating a window |
| Full RFVP core library tests with Sakura/external enabled | 42 passed; 8 existing parser/save-item/texture fixture tests fail with absent-file errors |
| `cargo check -p rfvp --features sakura-moyu,external-plugins --bins --lib` | Passed |
| Software desktop bins/library check with `soft-render-desktop,sakura-moyu,external-plugins` | Passed |
| Browser library check with `wasm` / `wasm32-unknown-unknown` | Passed |
| Actual guest release builds for `wasm32-unknown-unknown` | Passed |
| Clippy across six plugin crates, all targets, `-D warnings` | Passed |
| Focused RFVP library/tests Clippy with Sakura/external enabled | Passed with existing warnings |
| `cargo test --workspace` | Still blocked by unchanged console dependencies: stable-channel allocator/unwinding feature errors and unwinding intrinsic mismatch |
| `cargo clippy --workspace --all-targets` | Still blocked by unchanged WMV/MPEG decoder lints |
| `no_std` and `no_std,old_school` library checks | Same three baseline errors: missing Path, panic handler, and unwinding without std |
| `cargo fmt --all`; `git diff --check` | Run with unrelated baseline formatting preserved; passed |

Tests include strict/duplicate fields and numeric types, array limits, canonical identity ordering, version/capability/script mismatch, malformed plans, unknown imports, forbidden module features, early host calls, missing/double submit, trap-after-submit, fuel/memory exhaustion, exact EOF and zero-length boundaries, invalid source handles, high 64-bit offsets, cumulative copy exhaustion, symlink escapes and concurrent symlink replacement, stable independent file slices, deterministic conflict order, alias deduplication, overlapping/aliased claims, huge malformed archive headers, session aggregate bytes, merged counts above one plan, bounded conflict diagnostics, and worker-owned resource/session cleanup.

Dependency-tree checks show no Wasmi in default GPU/software desktop, `no_std`, `no_std,old_school`, software-core, browser, or UEFI profiles. Software-core/desktop and UEFI do not acquire wgpu. `rfvp-plugin-host` is absent from non-desktop target profiles. Only an explicitly enabled desktop `external-plugins` profile includes the interpreter. UEFI tree inspection is not a UEFI build or hardware run.

## Remaining gates

- Change 4 save/profile metadata and early load preflight are now implemented; see [Change 4 results](CHANGE-4-RESULTS.md). External loading remains experimental and not release-ready until the real-game and platform gates pass.
- No real Sakura hashes are verified or invented. Authorized real-game dialogue/choice/UI/read-skip/audio/save-load testing is outstanding.
- Representative zlib/GBK/input-count workloads, archive-hashing time, initialization latency distributions and peak RSS remain unmeasured. Revisit candidate policy if authorized data exceeds it.
- Windows/Linux builds and native execution, GPU/pump/software presentation smoke tests, and relevant platform packaging remain release work. No platform build script was changed.
- Formal schema/support publication and ABI 1.0 freeze remain dependent on those gates.
