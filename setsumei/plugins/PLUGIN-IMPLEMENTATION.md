# RFVP plugin implementation checklist

This checklist implements [RFC 001](PLUGIN-SYSTEM-RFC.md) and its [ABI candidate](PLUGIN-ABI-V1.md). Checked items are completed research/prototype or implementation work as described below. Unchecked items are required future changes, not claims about the current engine.

**Completed design evidence**

- [x] Compare main `ffc7380` with the Sakura branch `8f6458f` and identify the seven changed files.
- [x] Trace active parser, VFS, native workers, save state, and desktop/browser/host startup paths.
- [x] Define desktop version-1 scope and explicit deferred capabilities.
- [x] Build an isolated interpreter host and independently compiled guest.
- [x] Load normal and alternate guests with a verified unchanged host executable.
- [x] Exercise bounded imports and discard-on-failure in ten tests.
- [x] Measure small and 10,000-record cases and record the failed initial fuel budget.
- [x] Specify manifest, input claims, execution ABI, tables, limits, ordering, and profile metadata.

**Change 1: generic data and Sakura extraction**

Affected areas: new API and Sakura crates; `script/string_patch.rs`, `script/parser.rs`, `script/context.rs`, `script/mod.rs`, `resources/vfs.rs`; binary and library module declarations.

- [x] Start implementation in an isolated worktree based on the chosen main revision; retain the patch branch as a reference.
- [x] Add immutable generic string replacements independent of `zlib-flate2` and usable with `alloc`.
- [x] Preserve string payload addresses, original cursor advancement, original decoding, and missing-entry fallback.
- [x] Move `patch.dat` decoding and unique archive-name matching to the Sakura crate, depending only on the API.
- [x] Bound decompression, record allocation, and UTF-8 output; retain known format semantics without guessing header magic.
- [x] Add generic raw archive-entry mappings and preserve main's two loose-file lookup opportunities.
- [x] Carry over synthetic tests and add cursor, case-sensitive path, source-archive loose-file bypass, and streaming seek/read coverage.
- [x] Demonstrate plugins-disabled behavior against main and Sakura-enabled behavior against the branch.

Implementation is on branch `feat/plugin-data`, originally based on `ffc7380`. See [Change 1 results](CHANGE-1-RESULTS.md) for code, test commands, and baseline failures. Seven portable-crate tests and nine engine tests pass; the generic engine tests also pass without the Sakura feature or `zlib-flate2`. Comparative execution of both reference revisions passed: 29 process runs and nine transcript comparisons across Shift-JIS, GBK, and UTF-8, including distinct-case paths on a case-sensitive volume. Disabled outputs exactly match main; enabled outputs match the Sakura branch except for the RFC's explicit loose-file precedence correction. See [the comparison](CHANGE-1-COMPARISON.md) and `tools/plugin-reference/README.md` for evidence and reproduction. Real-game equivalence remains unverified. The optional `sakura-moyu` feature exposes a static preparation adapter; CLI activation remains Change 2.

Completion: no Sakura-specific parser/VFS behavior; no renderer/VM semantic changes; same reference outputs. Static extraction is not yet external plugin support.

**Change 2: shared preparation and session configuration**

Affected areas: new RFVP preparation module; `main.rs`, `lib.rs`, `app.rs`, `soft_host.rs`, `world.rs`; launcher headers only when options-aware APIs are introduced.

- [x] Introduce explicit launch options, prepared game, and per-session plugin identity.
- [x] Resolve selected script/root first; preserve existing `.bch`/`.hcb` discovery defaults.
- [x] Validate all manifests, duplicate IDs and claims before execution; bound counts and metadata construction.
- [x] Open declared inputs, hash through streams, and generate deterministic source handles and metadata.
- [x] Stage all plans, report key conflicts, and commit only after every enabled plugin succeeds.
- [x] Construct prepared game data without an extra default VFS scan; update both default and unsafe field-by-field initialization paths where needed.
- [x] Add repeatable `--plugin` and retain the Sakura alias through the same preparation logic. External execution is opt-in with `external-plugins`; an explicitly selected Sakura package deduplicates the bundled alias without relaxing its script-hash restriction.
- [x] Add versioned options-aware desktop library/pump creation, retaining old entry points with empty plugin lists.
- [x] Wire software desktop startup without introducing GPU/runtime dependencies into unrelated profiles.
- [x] Verify native worker transfer and shutdown with host-owned data and no guest callbacks.

Change 2 implementation is complete. [Change 2 results](CHANGE-2-RESULTS.md) record the initial startup work; [Change 3 results](CHANGE-3-RESULTS.md) record the completed manifest/input preflight, bounded original metadata, union-of-claims view, transactional composition, stable streamed sources, external backend, and alias/package deduplication. GPU CLI, desktop library/pump, and software startup share `LaunchOptions`, `PreparedGame`, and `PluginSession` without an extra default VFS scan. Focused preparation tests pass with bundled/external, bundled-only, external-only, and backend-disabled profiles. Presentation and real-game release checks remain in Changes 3/5; save-profile enforcement remains Change 4.

Completion: CLI/library/software host share configuration and failure behavior; no partial activation; legacy APIs still work.

**Change 3: external backend and actual Sakura package**

Affected areas: optional Wasmi adapter, SDK/example packaging, plugin build instructions and Cargo feature wiring.

- [x] Move the proven execution boundary into a production adapter with typed errors and plugin/stage diagnostics.
- [x] Implement the complete candidate execution ABI, including quotas and declarations; retain module validation and reject starts/WASI. Additional finite host metadata/count policies are documented; save metadata remains Change 4.
- [x] Open production file sources without path-check/open races; avoid whole-archive buffering. Confined opens and stable handles are implemented; concurrent symlink replacement is tested on macOS. Other-platform execution remains a release gate.
- [x] Compile the actual Sakura decoder/matcher to Wasm using the same contract as its static adapter.
- [x] Produce an external package and a synthetic conformance package; distribute no game data. Reproducible tooling emits generated packages under `target/plugin-conformance`; no verified real-game script list is claimed.
- [x] Record verified supported-script hashes from authorized data; keep only the bundled migration alias's documented unverified mode. `Sakura.hcb` `946877dd…789b` is recorded as `VERIFIED_SCRIPT_SHA256`; the alias warns only for unlisted scripts, and external packages still list hashes explicitly.
- [x] Confirm a fixed RFVP executable accepts an independently rebuilt plugin and produces correct in-game strings/resources. Headless real-data runs on macOS arm64, Linux arm64, and Linux x86_64 match the reference branch for all 61,164 string instructions and 18,166 resources. A person also verified the GPU CLI in game on macOS. Blank and undersized fallback glyphs were an engine bug, not a plugin bug; they are fixed by `71076b4` on `fix/cjk-font-fallback`, which is cherry-picked into the plugin branch. Translated games still need `--system-font` or a shipped font.
- [x] Measure zlib/GBK work, representative input counts, archive hashing time, module size, initialization latency, and peak RSS. External preparation takes 1.24 s and peaks at 79 MiB RSS on an M2 Pro; the bundled alias takes 171 ms. Full figures are in `CHANGE-5-RESULTS.md`.
- [x] Revisit candidate quotas if real data fails; document any ABI change before publication. Real data needed 3.69 billion fuel, so the candidate was revised from 1 billion to 10 billion. The worst-case spin takes about 19 s. No other limit changed.

The backend/package implementation and synthetic fixed-host verification are recorded in [Change 3 results](CHANGE-3-RESULTS.md). The actual Sakura guest matches bundled synthetic outputs; normal/alternate guests change output without rebuilding the host. Build/package instructions are in `tools/plugin-conformance/README.md`. Real-data equivalence, the verified script hash, measurements, and the fuel revision are recorded in [Change 5 results](CHANGE-5-RESULTS.md). External loading remains experimental until the Change 5 gates pass.

Completion: the no-engine-rebuild goal works inside RFVP, not only inside the standalone experiment.

**Change 4: save/profile compatibility**

Affected areas: `save_state.rs`, both save-write paths in `save_manager.rs`, early load preflight in `vm_runner.rs`, session identity producer.

- [x] Implement canonical implementation/profile digest encoding and validate metadata checksums.
- [x] Add the bounded `RFPM` metadata block before the unchanged V1 state payload/footer.
- [x] Record empty plugin profiles in new plugin-disabled resumable saves as well.
- [x] Check compatibility before any legacy globals, scene/audio, or VM mutations occur.
- [x] Reject known mismatches and malformed metadata without a legacy-restore fallback.
- [x] Retain unknown-profile handling for old saves with a clear diagnostic.
- [x] Verify legacy header/thumbnail readers and the old V1 suffix decoder still locate their data.
- [x] Test same-profile round-trip, reordered plugins, changed module/input bytes, missing plugin, changed script/NLS, malformed lengths, and legacy V1 fixtures.
- [x] Include a serialized `ConstString` and resource reopening during restoration.

Change 4 is implemented; see [Change 4 results](CHANGE-4-RESULTS.md). `preparation/profile.rs` encodes, strictly decodes, and checksums schema-1 profiles, and preparation rejects sessions whose metadata would exceed 64 KiB. Both save-write paths insert `RFPM` metadata before the unchanged V1 payload and footer. Plugin-disabled desktop saves record `"plugins":[]`. `VmRunner::load_slot_from_bytes` fully decodes and checks the save before applying the header or state. Rejections leave VM, title, slot, and graph state unchanged and never fall back to a legacy restore. Old RFVS saves restore with an unknown-profile warning; header-only saves are unchanged. Hosts without desktop preparation, including mobile, browser, and `rfvp-os`, write no metadata and behave like old hosts. Sixteen new tests and two cross-revision runner tests pass. `tools/plugin-reference/legacy_save.py` restores a save written by unchanged main and confirms that main reads a new profiled save. Apply remains non-transactional after a compatible preflight, matching existing behavior.

Completion: failed preflight leaves the live session unchanged; known profiles are enforced by new hosts; legacy saves remain readable. Old hosts ignoring metadata is documented.

**Change 5: validation and ABI publication**

- [ ] Run Windows, Linux, and macOS desktop builds and external-package smoke tests; record which architectures were actually tested. macOS arm64 and Linux arm64/x86_64 (containers, x86_64 under Rosetta) pass. Windows is untested.
- [ ] Validate GPU CLI, library/pump, and software desktop paths independently. All three build, and their option and preparation tests pass on macOS and Linux. Only the GPU CLI has been verified in game with real data.
- [x] Run `cargo fmt --all`, focused plugin tests, `cargo test --workspace`, and `cargo clippy --workspace --all-targets`; identify pre-existing failures separately. The focused tests and plugin clippy pass. Workspace test and clippy are blocked by the same pre-existing console-dependency and decoder failures, and the core library has 8 fixture failures.
- [x] Check `no_std`, `no_std,old_school`, software desktop, and plugins-disabled dependency profiles. After the fast-forward to `f0055c3`, upstream's CI `no_std` check passes. Wasmi appears only with `external-plugins`.
- [x] Run relevant WASM/UEFI/platform scripts where touched; ensure they do not acquire the external runtime accidentally. The UEFI x86_64 release build passes and the `wasm` check passes; neither tree contains a plugin runtime. No platform script was changed, and `build_wasm.sh` was not run because `wasm-bindgen` is absent.
- [x] Perform authorized local Sakura smoke tests for dialogue, choices, translated UI, read/skip behavior, audio, and save/load; record results without committing game data. All passed on the macOS GPU CLI with the external package, and a plugin save was correctly rejected without the plugin. The font rendering defects found in this test are fixed in the engine (`71076b4`), and the fix was confirmed in game.
- [ ] Publish schemas, SDK/example source, exact supported capabilities/limits, migration behavior, and error guidance. Draft JSON Schemas and `check_schemas.py` exist; they are not published until the freeze.
- [ ] Freeze ABI 1.0 only after the preceding gates pass; record deviations from the candidate explicitly.

Changes 1–4 and Change 3's real-data gates are complete; see [Change 5 results](CHANGE-5-RESULTS.md); the branch is now based on upstream `f0055c3`. These remain open:

- Windows validation;
- real-data presentation on the library/pump and software paths;
- reviews of the 2.29 MiB backend size and the 19 s worst-case fuel exhaustion;
- publication.

ABI 1.0 must not be frozen yet. Dynamic resource callbacks, fonts, syscall extensions, mobile/browser activation, and static console adapters require separate follow-up RFCs or versioned capability proposals.
