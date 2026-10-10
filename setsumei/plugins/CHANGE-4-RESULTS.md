# Change 4: save/profile compatibility

Branch `feat/plugin-data`, continuing Changes 1–3 on `ffc7380`. This implements save-profile metadata and early load preflight from [RFC 001](PLUGIN-SYSTEM-RFC.md) and the ABI candidate's "Session identity and save metadata". ABI 1.0 is still a candidate. Real-game, performance, and platform release gates remain open.

## Implemented

- `preparation/profile.rs` implements profile identity schema 1. `profile_digest` follows the ABI byte encoding: `RFVP-PROFILE-1\0`, the script hash, NLS, plugin count, and each plugin's ID, version, implementation hash, and ordered input path/hash pairs. Metadata JSON contains `schema_version`, `profile_sha256`, `script_sha256`, `nls`, and `plugins`. Decoding rejects unknown, duplicate, and missing fields, wrong numeric types, unsupported schema versions or NLS values, invalid or uppercase hashes, duplicate or empty plugin IDs, more than 16 plugins, metadata over 64 KiB, and recomputed-digest mismatches. Typed profiles are then compared exactly.
- Every prepared desktop session must encode within the 64-KiB metadata limit. A profile that would exceed it fails preparation, so it cannot surface later as a save failure during gameplay. Ordinary sessions are small; the synthetic Sakura save's metadata is about 0.5 KiB.
- `save_state.rs` writes `metadata JSON | u32 metadata_len | RFPM` immediately before the unchanged bincode `SaveStateSnapshotV1` and `u32 state_len | RFVS` footer. `ResumableSaveState::capture` encodes the profile before consuming the pending VM snapshot. `SaveCreate(3)`/`local_saved` and the immediate `finalize_save_write` path both use `append_resumable_state`. Plugin-disabled desktop sessions record `"plugins":[]`. If no metadata is supplied, the writer emits exactly the historical bytes.
- Load handling now begins with `preflight_load`. It locates the V1 payload, reads the optional RFPM block, strictly decodes the snapshot, and checks profile compatibility before touching any state. `VmRunner::load_slot_from_bytes` performs header loading and `SaveStateSnapshotV1::apply` only after preflight succeeds. Any rejection is logged as `load: rejected save slot N; the running session was left unchanged: <reason>`. Corrupt RFVS state, malformed metadata, and known mismatches therefore never fall back to header-only or legacy restoration. The old behavior applied the header and then logged a state-decode error.
- Compatibility outcomes:
  - Matching profile: the state is restored silently.
  - Different script, NLS, plugin order or set, plugin version, implementation, input list, or input bytes: the load is rejected with the first differing field and the saved plugin list.
  - A plugin-free save loaded into a plugin session, or the reverse: rejected.
  - An RFVS save without RFPM: restored with an `unknown plugin profile` warning.
  - A header-only save without RFVS: unchanged historical header-only load.
- Hosts that cannot activate plugins write no metadata and do not compare profiles. This includes mobile, browser, `rfvp-os`, and software-core-only builds without the desktop preparation configuration. They still reject malformed RFPM framing, but otherwise act like older RFVP releases. A desktop world without a session identity rejects saves requiring plugins and warns for plugin-free profiles. Production desktop preparation always supplies an identity.
- The historical `try_decode_state_chunk_v1` remains and still ignores the profile block. Header, slot-cache, and thumbnail readers parse from the start of the file and are unchanged.

## Limits and residual behavior

- Metadata removal or editing is not a security boundary. The digest detects damaged or inconsistent metadata; it is not a signature.
- Old RFVP releases ignore RFPM and restore any profile. This was verified against unchanged main.
- Unknown-profile saves can mix serialized old strings with resources reopened through the current VFS. Tests demonstrate this hazard: serialized `ConstString`s are restored unchanged, never retranslated.
- A legacy RFVS save whose last thumbnail pixel happens to be the bytes `R F P M` would be treated as having malformed metadata and rejected. That pixel has alpha 77. RFVP thumbnails are framebuffer readbacks, so this is not expected in practice, but it remains a theoretical false positive.
- Preflight guarantees compatibility before mutation. It does not make `SaveStateSnapshotV1::apply` transactional. A resource-read failure after a compatible preflight keeps the pre-existing partial-restore and logging behavior.
- Rejection is reported through the existing log channel. There is no new in-game dialog.

## Verification

All checks used only synthetic data and ran on macOS 15.8 / arm64.

| Check | Result |
| --- | --- |
| Profile unit tests (6): ABI byte-encoding transcription for empty and plugin profiles, plugin/input order sensitivity, exact JSON schema, round trip, every mismatch class, unknown/unidentified sessions, 13 malformed variants including duplicate JSON keys and forged checksums, bounded encoding | Pass |
| Save framing tests (3): legacy-identical bytes without metadata; header/slot-cache/thumbnail readers from memory and file; historical V1 decoder on profiled and unprofiled files; zero-length and payload-at-start edges; oversize/truncated RFPM; truncated, overlong, and trailing-byte RFVS; invalid marker rejected by preflight but ignored by the historical decoder | Pass |
| Engine save/load tests (6 + 1 external): actual preparation, `ResumableSaveState::capture`, `finalize_local_savedata_prepare`, and `VmRunner::load_slot_from_bytes`. Coverage includes same-profile restore of a serialized `ConstString` (`你好` at address 6); graph 5 reopened through the loading session's mapped VFS; legacy V1 restore with an unknown profile; forged checksum, bad length, and malformed JSON; plugin-free metadata; changed selected script (`.bch`); changed NLS; missing plugin; plugin-free/plugin swap; changed `patch.dat` bytes; reordered external plugins; dropped external plugin; same-manifest module rebuild; changed external input; and recovery after restoring inputs. Every rejection asserts the error and an unchanged VM, title, slot, and graph fingerprint. | Pass |
| `soft-render-core,no-audio,sakura-moyu,external-plugins`: preparation + save tests | 31 passed, 2 ignored (cross-revision); three consecutive runs |
| Bundled-only, external-only, `desktop-preparation`, and `soft-render-core,no-audio` (no preparation) profiles | 25, 26, 23, and 3 passed respectively |
| `gpu-render,no-audio,sakura-moyu,external-plugins` preparation/save/FFI-options tests | 32 passed |
| Full RFVP core library tests with Sakura/external | 58 passed; same 8 pre-existing absent-fixture failures (`test_parser`, `test_read_save_item`, six texture tests) |
| Plugin crate tests | 27 passed: 2 API, 3 SDK, 5 Sakura, and 17 host |
| `cargo check -p rfvp` for default bins/lib, Sakura/external bins/lib, software desktop, `soft-render-core,no-audio`, `rfvp-os` (host check), and `wasm` on `wasm32-unknown-unknown` | Pass; no new warnings in touched files |
| `no_std` and `no_std,old_school` | Same three baseline errors |
| Clippy on touched RFVP files (core library/tests; GPU library/bins with Sakura/external) | No findings in new or changed code |
| `rustfmt` on new files and `preparation.rs`; `git diff --check` | Clean. Baseline formatting in `app.rs`, `save_manager.rs`, and `vm_runner.rs` was preserved. |

### Cross-revision legacy fixture

```sh
uv run --no-project python tools/plugin-reference/legacy_save.py
```

The runner builds `tools/plugin-reference/legacy_save.rs` against an unchanged `ffc7380` worktree using only main's public API. Checks in both directions passed:

1. Main wrote a resumable save: 1,300,066 bytes, SHA-256 `dafdc398ec6a5ed05ed8ca26c511a4a0f481953e4a3705192bf60746cd38cdf0`, with no RFPM. The save contains a `ConstString("ab", 6)` and a VFS-backed graph 5. The working tree restored it in plugin-disabled and Sakura sessions with an unknown-profile warning. It restored the title, kept the serialized string, and reopened graph 5 through each session's VFS: original pixels without plugins and translated pixels with Sakura.
2. The working tree wrote a Sakura-profiled save. Main's `SaveManager`, `SaveItem::load_from_mem`, thumbnail reader, and `try_decode_state_chunk_v1` read the title, thumbnail, `你好@6`, and `graph/menu` unchanged. This confirms that old hosts ignore the metadata.

Generated saves and the report remain under `target/plugin-reference/legacy-save-*/` and are not committed. Because the profiled save header contains the current time, its file hash changes on every run.

## Remaining gates

- Authorized real-game save/load across dialogue, choices, read/skip, and audio remains outstanding, as do verified Sakura script hashes and representative performance/RSS measurements from Change 3.
- Windows/Linux execution, presentation-path smoke tests, and ABI publication remain Change 5 work. Mobile/browser profile enforcement depends on a future decision to give those hosts session identities.
