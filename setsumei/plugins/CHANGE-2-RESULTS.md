# Change 2 progress: shared desktop startup and bundled activation

> Historical startup-milestone report. The remaining Change 2 work and external backend are now implemented; see [Changes 2–3 results](CHANGE-3-RESULTS.md). Statements below about unavailable external execution and remaining manifest/composition work describe this earlier milestone, not the current implementation. Save-profile and real-game/platform release gates remain open.

Branch `feat/plugin-data`, continuing the Change 1 implementation on `ffc7380`. This completes the startup portion of Change 2. Manifest preflight and multi-plugin composition are still required; this is not external plugin support.

## Implemented behavior

- `preparation::{LaunchOptions, PreparedGame, PluginSession}` provides a shared desktop preparation service. Preparation selects and parses the script, resolves its parent directory, builds the VFS, and stages the bundled adapter before returning the parser/VFS/session together. It does not start a window or VM. Legacy global resource/save roots are activated only after preparation succeeds.
- GPU CLI and desktop library/pump keep `.bch` before `.hcb` discovery. The software desktop host keeps `.hcb`-only discovery. Both retain the existing glob ordering. Script NLS is unchanged by plugins.
- `GameData::boxed_with_vfs` and `init_with_vfs_in_place` consume the prepared VFS without an extra `Vfs::default()` scan. Both default initialization paths initialize the optional session field consistently. Legacy/mobile/browser builders remain available.
- Both desktop CLIs accept repeated `--plugin PACKAGE_DIRECTORY` in order and `--sakura-moyu-chs-patch`. Explicit external activation currently fails with a backend-unavailable error before any adapter executes. An unavailable bundled feature also fails rather than silently launching without the patch.
- The bundled Sakura alias uses the shared preparation service with the `sakura-moyu` feature. Successful activation emits an unverified-script diagnostic containing the exact selected script hash. Required-input failures return no prepared game.
- Session data records the exact parsed script bytes' SHA-256, source NLS, plugin ID/version, implementation identity, and ordered `patch.dat`/`patch.bin` hashes. `patch.dat` is hashed from the same bounded bytes passed to the decoder; `patch.bin` is hashed through a 64-KiB buffer. The archive source uses handle 2, corresponding to manifest-style input order.
- Sakura's portable crate generates a reproducible source-content digest at build time from its source, build script, manifests, API source, and workspace lockfile. The engine combines this with the static adapter source and the candidate's canonical normalized implementation semantics. The bundled migration alias has an empty verified-script list. Lockfile changes conservatively change the identity even when they concern another workspace dependency.
- Prepared tables and identity move into the existing native worker ownership model. Tests exercise a sync message, read a mapped archive stream while the worker holds the world, and verify the world/session are released after the channel closes. No guest callback or runtime is retained.

## Desktop launcher API

The existing `rfvp_run_entry` and `rfvp_pump_create` remain, delegating to plugin-disabled preparation. New desktop-only functions are:

```c
int rfvp_run_entry_v1(const char *game_root_utf8, const char *nls_utf8,
                      const unsigned char *options_json, size_t options_len);
void *rfvp_pump_create_v1(const char *game_root_utf8, const char *nls_utf8,
                          const unsigned char *options_json, size_t options_len);
```

Root and NLS are NUL-terminated UTF-8. Options are length-delimited UTF-8 JSON, at most 64 KiB, with no trailing NUL. Null pointers, invalid JSON, duplicate/unknown fields, empty package paths, and more than 16 requested activations fail. The version is carried by the entry-point name. Both fields default to disabled/empty:

```json
{"plugins": [], "sakura_moyu_chs_patch": true}
```

Package paths are intended to be relative to the caller's working directory, independent of the game root. The run function returns 0 on success, 1 on launch failure, and 2 on invalid options/arguments; the pump function returns NULL on failure. Diagnostics use the existing logger. The macOS launcher header includes these declarations and corrects the old `rfvp_run_entry` declaration to match its existing Rust `i32` return value. Mobile headers are unchanged.

For bundled activation:

```sh
cargo run -p rfvp --features sakura-moyu -- --project-dir /path/to/game --sakura-moyu-chs-patch
```

## Verification

Executed on macOS/aarch64 using synthetic data only. No visual game-play or real-game smoke test is claimed.

| Command/check | Result |
| --- | --- |
| `cargo test -p rfvp-plugin-api -p rfvp-plugin-sakura` | 7 passed |
| `cargo test -p rfvp --lib --no-default-features --features soft-render-core,no-audio,sakura-moyu preparation` | 7 passed: six preparation tests plus the prior Sakura integration test |
| Same preparation filter with `soft-render-core,no-audio,desktop-preparation` | 6 passed, including unavailable-Sakura rejection |
| All RFVP library tests with `soft-render-core,no-audio,sakura-moyu` | 34 passed; 8 existing fixture-dependent tests failed with missing-file errors: parser, save-item reader, and six texture tests |
| `cargo test -p rfvp --lib --no-default-features --features gpu-render,no-audio,sakura-moyu desktop_options` | FFI pointer/length/JSON rejection test passed without creating a window |
| `cargo test -p rfvp --no-default-features --features gpu-render,no-audio,sakura-moyu --lib --bin rfvp launch_options` | GPU CLI order/missing-argument test passed |
| `cargo test -p rfvp --lib --no-default-features --features soft-render-desktop,sakura-moyu launch_options` | Software CLI order/missing-argument test passed |
| `cargo check -p rfvp --features sakura-moyu --bins` | Passed with existing warnings |
| `cargo check -p rfvp --no-default-features --features soft-render-desktop,sakura-moyu --bins --lib` | Passed with existing warnings |
| `cargo check -p rfvp --no-default-features --features wasm --target wasm32-unknown-unknown --lib` | Passed with existing warnings |
| `cargo clippy -p rfvp --no-default-features --features soft-render-core,no-audio,sakura-moyu --lib --tests` | Passed with existing warnings |
| `cargo test --workspace` | Blocked by unchanged console dependencies: stable-channel feature errors in `linked_list_allocator`/`unwinding`, plus `unwinding` intrinsic type mismatch |
| `cargo clippy --workspace --all-targets` | Blocked by unchanged WMV/MPEG decoder lints, including equal/erasing operations and raw-pointer FFI |
| `cargo check -p rfvp --no-default-features --features no_std --lib` and `no_std,old_school` | Same three documented baseline errors: missing `Path` in `text_manager.rs`, missing panic handler, unwinding without std |
| `clang -x c -fsyntax-only platform/macos/RFVPLauncher/Headers/rfvp.h` | Passed; only the standalone-header pragma warning |
| `cargo fmt --all` / `git diff --check` | Run; unrelated baseline formatting preserved / passed |

## Remaining work

- Implement strict bounded manifests, duplicate-ID/claim preflight, union-of-claims metadata construction, and deterministic input contexts for arbitrary packages before execution.
- Bound archive metadata before the existing parser allocates it. The bundled adapter still inherits Change 1's path-based archive opening/reopening; stable sources and race-resistant production opening remain required.
- Stage/merge multiple plans with conflict diagnostics and aggregate quotas, and deduplicate the bundled alias against explicit activation of the same plugin. The current external-request error prevents accidental partial bundled activation while this is incomplete.
- Add the Wasmi backend and actual external Sakura package in Change 3. There is no external interpreter dependency in this change.
- Persist and enforce profile identity during save/load in Change 4. Session identity alone does not make saves profile-safe; no RFPM metadata is written yet.
- Run Windows/Linux builds, platform packaging, presentation smoke tests for all desktop entry paths, and authorized real-game checks. No packaging script was changed or run.
