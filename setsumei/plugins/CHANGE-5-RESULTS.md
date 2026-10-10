# Change 5 and remaining Change 3 gates: real data, platforms, schemas

Branch `feat/plugin-data`.

The base was fast-forwarded from `ffc7380` to upstream main `f0055c3`. The new upstream commits change only these:

- a `no_std` `Path` import;
- the UEFI `wcslen` shim;
- a CI `no_std` job;
- the README.

None overlaps the plugin changes. Reference comparisons still use main `ffc7380` and the Sakura branch `8f6458f`; neither upstream change affects desktop behavior. ABI 1.0 remains a candidate.

Authorized local game data was used read-only, through an APFS clone outside the repository. No game data, text, or assets are committed. Recorded values are SHA-256 digests, sizes, counts, and timings.

## Real-game equivalence (Change 3)

Run with `tools/plugin-real-data/run.py`; see `tools/plugin-real-data/README.md`.

| Item | Value |
| --- | --- |
| Script | `Sakura.hcb`, 5,002,852 bytes, SHA-256 `946877dd0ed8fbf318ba5c73d20afd46b9dcdc200aa7e8edc610c437cc1c789b`, NLS `sjis` |
| Inputs | `patch.dat` 1,281,129 bytes `32ebb9903df645fba8981965de729dcf61d71c94d5ede9377e4ac59a99f490ce`; `patch.bin` 31,634,830 bytes `04518ff4563ea54dc2f50b3c78c8b3d058b2b2c2d42ac4d35911cb4ed2d6096c` |
| Script sweep | 966,284 instructions, of which 61,164 are `push_string`; the sweep ends exactly at the code end |
| Strings | All 57,053 `patch.dat` records hit a `push_string` payload; 55,193 effective texts change |
| Resources | 18,166 logical paths, 7.70 GB hashed per observation |
| Mappings | 70 entries mapped and 1 unmatched. The 71 `patch/*` paths become unavailable, as in the reference |

Five independent processes run on each platform: main disabled, reference branch, current disabled, current bundled alias, and current external package. On every platform:

- main and the current tree with plugins disabled agree on every `push_string` instruction and every archive resource: address, cursor, text digest, length, and content digest;
- the reference branch, the bundled alias, and the external Wasm package agree on all of the same, plus the replacement-hit flags.

The real game root has no loose resource overrides, so the documented loose-file precedence correction is not exercised by this data.

Verified script identity: the hash above is recorded as `rfvp_plugin_sakura::VERIFIED_SCRIPT_SHA256`. The bundled alias's manifest now lists it, so that script no longer gets the unverified-identity warning. Other scripts still get the documented warning under the alias's migration mode. External packages still require an explicit `--script-sha256`; `package_sakura.py` does not add hashes implicitly.

Fixed host: each run builds the release host probe first, then builds the actual Sakura guest, packages it, and runs it. The host digest is checked before every external observation and again at the end of the run.

## Fuel budget revision

The production external run failed with `all fuel consumed by WebAssembly` under the 1,000,000,000 budget. Measured needs and limits:

| Resource | Sakura used | Candidate limit |
| --- | ---: | ---: |
| Fuel | 3,687,674,188 | **10,000,000,000** (was 1,000,000,000) |
| Linear memory | 34,013,184 bytes | 64 MiB |
| Context bytes | 853,105 | 8 MiB |
| Context + source + plan copies | 6,819,440 bytes | 64 MiB |
| Plan bytes | ~4.69 MB | 16 MiB |
| String records / resource records | 57,053 / 70 | 100,000 each |
| Original archive entries | 18,166 | 100,000 |

Fuel use was identical on macOS arm64, Linux arm64, and Linux x86_64.

A temporary per-stage instrumentation (since reverted) attributed the cost:

| Stage | Fuel |
| --- | ---: |
| Context JSON parse | 0.23 billion |
| zlib inflate | 0.42 billion |
| `encoding_rs` GBK decode of 1.09 M characters | 2.3 billion |
| Matching, conversion, and plan serialization | 0.26 billion |

The GBK decode costs about 1,800 fuel per character but only about 0.2 s of wall time. Wasmi charges whole blocks up front, which overstates that code. Fuel is therefore a deterministic quota, not a time estimate.

New policy: `FUEL = 10_000_000_000` (2.7× headroom over Sakura). An ignored test, `full_fuel_budget_bounds_a_tight_loop`, measures the worst case: a spinning guest takes 19.1 s on an Apple M2 Pro to exhaust the budget, against about 1.9 s under the old budget. That is the startup cost of a hostile or buggy explicitly selected plugin. No other ABI limit, wire field, or import changed. The revision is recorded in the ABI candidate.

## Performance and size

Preparation time is measured from launch-option handling to a committed parser and VFS. It includes script hashing, input hashing, archive metadata, and guest execution. Medians are of 7 runs on macOS and 5 on Linux; RSS is the process peak.

| Mode | macOS 15.8 arm64 (M2 Pro) | Linux arm64 container | Linux x86_64 container (Rosetta) |
| --- | --- | --- | --- |
| main, disabled | 39 ms / 12.7 MiB | 20 ms / 11.0 MiB | 32 ms / 12.9 MiB |
| reference branch | 57 ms / 25.2 MiB | 37 ms / 20.1 MiB | 61 ms / 22.2 MiB |
| current, disabled | 52 ms / 13.1 MiB | 33 ms / 11.5 MiB | 55 ms / 14.0 MiB |
| current, bundled | 171 ms / 37.0 MiB | 168 ms / 34.7 MiB | 260 ms / 37.7 MiB |
| current, external | 1,239 ms / 78.9 MiB | 1,249 ms / 59.6 MiB | 1,641 ms / 65.4 MiB |

Notes on these figures:

- Guest execution accounts for about 1.10 s of the macOS external time (1.11 s on Linux arm64, 1.43 s on x86_64 under emulation).
- The disabled-mode overhead of about 13 ms comes from computing the session's SHA-256 script identity.
- The x86_64 numbers ran under emulation and are not native performance.

Module size: 435,914 bytes (macOS build). The module is not bit-reproducible across build hosts: Linux builds produce 435,852 bytes (arm64) and 435,844 bytes (x86_64). Remapping path prefixes and stripping symbols did not make the bytes identical; crate disambiguators and codegen still differ. The package is therefore the distributed artifact: rebuilding it elsewhere changes `implementation_sha256` and, deliberately, the save profile. Reproducible guest builds are future work.

Desktop binary sizes on macOS arm64 (release, GPU CLI):

| Build | Bytes | Difference |
| --- | ---: | ---: |
| main `ffc7380`, default | 43,868,976 | |
| current, default | 44,235,888 | +366,912 over main |
| current, `sakura-moyu` | 44,340,928 | +105,040 over default |
| current, `sakura-moyu,external-plugins` | 46,636,496 | +2,400,608 over default (2.29 MiB) |

The external backend therefore exceeds the experiment's 2 MiB loader-delta regression target by about 0.29 MiB. It is opt-in and absent from default builds.

## Gameplay smoke test (macOS GPU CLI, external package)

A person ran this on macOS 15.8 / arm64 with:

```sh
../release/rfvp --project-dir game --plugin pkg-sakura
```

The RFVP release build had `sakura-moyu,external-plugins`. The package was the actual Sakura guest built after the host.

| Check | Result |
| --- | --- |
| Translated title graphics (`樱花萌放` logo) | Pass; also captured by window screenshot |
| Dialogue in Chinese | Pass; glyph rendering defects found and fixed in the engine (below) |
| Choices | Pass; same font defects |
| Translated UI graphics | Pass |
| Read/skip | Pass |
| BGM, voice, SE | Pass |
| Save, quit, relaunch, load | Pass |
| Load the plugin save without the plugin | Rejected as designed: `load: rejected save slot 1; the running session was left unchanged: … activated plugins differ (save: rfvp.sakura-moyu-chs@0.1.0; session: no plugins)`; the game kept running |

All 15 save files written in these sessions carry RFPM metadata that validates against `setsumei/plugins/schemas/save-metadata.schema.json`:

- 13 have the external Sakura profile; their `implementation_sha256` begins `316a8cb8…` and matches the probe session.
- 2 have the empty profile and come from the plugin-free relaunch.

Not covered here: Linux/Windows presentation, the library/pump and software-render presentation paths with real data, and movies with subtitles.

### Font rendering: engine fix

The translation's non-ASCII text uses 3,307 unique characters. The gameplay test with `--system-font` showed both blank glyphs and smaller glyphs in dialogue. There were two engine problems, both pre-existing on main and independent of the plugin:

1. **Blank glyphs.** The embedded `MSGOTHIC.TTF` maps 4,798 CJK ideographs to glyphs without outlines; 263 of them occur in this translation, including 养, 兽 and 关. `Font::has_glyph` checked only the cmap entry, so fallback never happened.
2. **Undersized fallback glyphs.** `ab_glyph::PxScale` scales by ascent−descent, not by the em square. MS Gothic's ascent−descent is 0.996 em, while Source Han and Noto Sans SC measure 1.437–1.448 em and Songti 1.400 em. Fallback ideographs such as 东 and 啊 were therefore drawn at about 70% size.

Fix: commit `71076b4` ("fix cjk font fallback for empty and mis-sized glyphs") on branch `fix/cjk-font-fallback`, based on main `f0055c3`. It is cherry-picked here as `2e6014a`, and it changes only `font.rs` and `text_manager.rs`:

- A cmap entry without an outline counts as missing, except for whitespace and control characters.
- Fallback glyphs are rasterized at the primary face's em size. When rescaled, they share the primary face's baseline, as GDI font linking does. Faces within 1% of the primary's ratio keep the historical size and baseline exactly.
- `--system-font` candidates are collected first and then loaded in a deterministic Gothic-first priority order. Within a family, light and thin files load last. The order is Microsoft YaHei, SimHei, DengXian, PingFang, Heiti, Hiragino Sans GB, Noto Sans SC/CJK, Source Han Sans, and only then the serif faces. Previously directory order applied: on the test Mac, Source Han Serif was first and drew Mincho-style glyphs inside Gothic text.

Verification:

- **Unit tests:** two new tests cover empty-glyph fallback with the embedded faces and the em-size factor. There are also two ignored manual tests: one compares installed-font ink heights with the primary face, and one renders a line through the real text-slot path.
- **Installed fonts:** Songti, Source Han Serif VF, Noto Sans SC VF, and Hiragino Sans GB rasterize fallback ideographs at 44–47 px against MS Gothic's 43 px at the same size. Before the fix they were about 30 px.
- **Rendered line:** the real text-slot path rendered the screenshot's line before and after the fix. Before: blanks for 养, 兽 and 关 and small 东/啊. After, with system fonts: every character is drawn at matching size and baseline in Heiti.
- **Japanese regression:** 5,000 sampled original Japanese script lines rendered pixel-identically before and after, both with and without `--system-font`.
- **Builds:** default, software-core, software-desktop, `no_std` and `no_std,old_school` on `x86_64-unknown-uefi`, and `wasm` all compile. The library tests show the same 10 fixture failures as main, plus 2 new passing tests. With the plugin features enabled, 61 pass and the same 8 fixture failures remain.

Remaining limitation: 911 of the translation's characters exist in none of the four embedded MS faces, and they appear in 42,760 of the 57,053 lines. Without `--system-font` or a game `font/` directory, they still render as the primary face's `.notdef` glyph. A translated package therefore needs `--system-font` or a shipped font. Font registration by plugins remains deferred by RFC 001. Live confirmation: a person played the fixed release GUI build (`2e6014a`, external package, `--system-font`) on macOS. It loaded Heiti Medium first, and Chinese dialogue rendered correctly. The log has no panic or load rejection; it shows the same syscall error classes described below.

The same session logged many `motion` syscall "Invalid duration"/"Invalid id" errors, `graph_load: invalid id : Nil`, and `history_set` errors with Nil arguments. The session that the person closed accidentally also logged one `unknown opcode: 0x4e @ 0x000000, thread: 15`. These come from syscalls that the plugin work does not change. They have not been compared with a plugin-free playthrough of the same scenes.

## Platform and profile validation (Change 5)

| Check | macOS 15.8 arm64 | Linux arm64 (Debian bookworm container) | Linux x86_64 (same, Rosetta) |
| --- | --- | --- | --- |
| GPU CLI release build with `sakura-moyu,external-plugins` | Pass | Pass | Pass |
| Default release build | Pass | Pass | Pass |
| GPU library check with plugins | Pass | Pass | Pass |
| Software desktop bins and library check | Pass | Pass | Pass |
| Plugin crate tests (2 API, 3 SDK, 5 Sakura, 17 host plus 1 ignored) | Pass | Pass | Pass |
| Preparation and save tests, `soft-render-core,no-audio,sakura-moyu,external-plugins` | 32 pass | Pass | Pass |
| GPU-render preparation, save, and FFI options tests | 33 pass | Pass | Pass |
| Software launch-option test | Pass | Pass | Pass |
| Synthetic fixed-host conformance (6 runs) | Pass | Pass | Pass |
| Real-data equivalence | Pass | Pass | Pass |
| Gameplay smoke test | Pass (person, GPU CLI) | Not run (no display) | Not run |

Linux evidence comes from `tools/plugin-platform/linux-docker.sh`. Every container mounts the checkout and the game read-only and records `dpkg --print-architecture` and the ELF machine of the built host: arm64/`00b7` and amd64/`003e`. That check exists because the first x86_64 attempt silently reused a cached base image of the wrong architecture; the script now always passes `--platform`.

The real-data runner gives each probe its own target directory. A shared directory was observed to link the current `rfvp` into the reference-branch probe after an incremental rebuild in the container.

Windows: validated in CI by `.github/workflows/plugin-validation.yml` on the fork (`appe233/rfvp`, run 38038795118, commit `bb407c2`). Both runners passed:

- `windows-2022`: Windows Server 2022, x86_64-pc-windows-msvc.
- `windows-11-arm`: Windows 11 ARM64, aarch64-pc-windows-msvc, running natively.

| Check (each runner) | Result |
| --- | --- |
| GPU CLI release build with `sakura-moyu,external-plugins` | Pass |
| GPU library check (pump/entry FFI) | Pass |
| Software desktop bins and library check | Pass |
| Plugin crate tests | 2 API, 3 SDK, 5 Sakura, 15 host plus 1 ignored. The two Unix-only symlink tests are compiled out |
| Preparation and save tests, software core | 31 pass. The Unix symlink-alias test is compiled out |
| GPU-render preparation, save, and FFI options tests | 32 pass |
| Software launch-option test | Pass |
| Synthetic fixed-host conformance (6 runs) | Pass. Strings, addresses (6), cursors (9), and mapped resources match; the alternate guest changes output without rebuilding the host |
| Schema check (26 checks) | Pass |

The first CI run failed only in conformance. On Windows, Python decoded the probe's UTF-8 JSON with the ANSI code page. The tools now use UTF-8 explicitly. The repository's general "Build Artifacts" workflow also passed its default Windows x64 and arm64 builds on this branch; it also passed Linux, macOS, iOS, FreeBSD, wasm, UEFI, and `no_std`. Only Android APK signing failed, because the fork lacks the signing secrets; upstream main passes that step.

Not covered on Windows: authorized real-data equivalence and gameplay, because CI has no game data and no person playing.

Other profiles on macOS:

| Profile | Result |
| --- | --- |
| `soft-render-core,no-audio` + `sakura-moyu` / `external-plugins` / `desktop-preparation` / none | 26 / 26 / 23 / 3 pass |
| Full RFVP core library tests with Sakura/external | 59 pass; 8 fail, all needing absent fixtures (`test_parser`, `test_read_save_item`, 6 texture tests) |
| Upstream CI `no_std` check (`--target x86_64-unknown-uefi`) and `no_std,old_school` | Pass |
| Host `no_std` and `no_std,old_school` | Same 3 errors as unchanged main (panic handler, unwinding, `Path`) |
| UEFI app `x86_64-unknown-uefi` release build | Pass; its tree has no Wasmi, `rfvp-plugin-host`, `cap-std`, or wgpu |
| `wasm` on `wasm32-unknown-unknown` | Check passes; `cargo tree` shows no Wasmi |
| Default, software desktop, software core, `no_std`, and `sakura-moyu` trees | No Wasmi; only `external-plugins` includes it |
| `cargo test --workspace` | Blocked, as before, by console dependencies (`#![feature]` on stable; `unwinding` mismatch) |
| `cargo clippy --workspace --all-targets` | Blocked, as before, by WMV/MPEG decoder lints |
| Clippy, `-D warnings`, on the six plugin crates (host, all targets; guests, wasm32) | Pass |
| `rustfmt` on new and changed plugin files | Clean |

Notes:

- The UEFI build temporarily rewrote its stale lockfile. The lockfile was already stale on main; it was restored afterwards.
- `build_wasm.sh` was not run: `wasm-bindgen` is not installed. No platform script was changed.

## Draft publication material

- `setsumei/plugins/schemas/`: JSON Schema 2020-12 drafts for the manifest, context, plan, and save metadata. Each documents the byte limits and cross-field rules that JSON Schema cannot express.
- `tools/plugin-conformance/check_schemas.py`: validates the schemas against generated synthetic packages, a plan, the ABI context example, 20 structural negative cases, and the RFPM block of each profiled save given, with one negative case per save. It passed with 15 saves: 56 checks.
- The conformance README now records the verified environments, the verified script identity, and the fuel revision. It no longer claims that save-profile enforcement is missing.

## Remaining gates

- Windows real-data equivalence and gameplay (CI covers builds, tests, and synthetic conformance).
- Presentation checks for the library/pump and software paths with real data.
- Reproducible guest builds, if they become a release requirement.
- Review of the 2.29 MiB backend size and the 19 s worst-case fuel exhaustion.
- Final publication, and the ABI 1.0 freeze with the fuel deviation recorded.
