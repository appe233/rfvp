# RFVP plugin system: preliminary plan

Superseded as the implementation proposal by [RFC 001](PLUGIN-SYSTEM-RFC.md), the [ABI 1.0 candidate](PLUGIN-ABI-V1.md), [prototype results](PLUGIN-PROTOTYPE-RESULTS.md), and [implementation checklist](PLUGIN-IMPLEMENTATION.md). The text below is retained as the historical source-research record; its statements about experiments not yet being performed describe that earlier stage.

Status: proposal based on source inspection of `sakura-moyu-patch-loader` at `8f6458f` and comparison with `main`/freshly fetched `origin/main` at `ffc7380`. No engine changes or runtime/backend experiments have been made. Names and interfaces below are proposed, not existing APIs.

**Comparison with main**

The patch branch has already merged this main revision. The complete tree diff contains seven changed source files, with 727 added lines and 23 removed lines, including tests. Local `main` and `origin/main` resolve to the same revision. The comparison used Git snapshots without switching the working branch.

| Already present in main | Introduced by the Sakura patch branch |
| --- | --- |
| `ConstString` with original HCB address and original-length cursor advancement | The `patched_string` lookup in `push_string` and its regression tests |
| `Parser` with script bytes, metadata, and NLS decoding | The `StringPatchTable` field, attachment methods, and `zlib-flate2`-gated integration |
| Standard FVP archive parsing, seekable streams, and loose-file overrides | `SakuraMoyuPatchOverlay`, its constructor, unique-name matching, and precedence tests |
| Combined loose-file/pack-entry opening in `VfsFile::open_stream_with_len` | Extraction of a raw archive-entry helper that bypasses loose files under the source archive's folder |
| Ordinary CLI → parser → `AppBuilder::with_vfs` startup | The patch CLI flag, required-file checks, and game-specific builder method |
| Desktop GPU `.bch` preference, text/read-history semantics, platform startup differences, save serialization, and direct font loading | No changes to those behaviors |

The seven files are `app.rs`, `main.rs`, `script/context.rs`, `script/mod.rs`, `script/parser.rs`, the new `script/string_patch.rs`, and `subsystem/resources/vfs.rs`. Other inspected host, save, font, worker, and resource-consumer files are identical to main; the workspace and RFVP manifests also have no branch differences.

Use main as the reference for core semantics and plugins-disabled behavior. Use this branch's patch parser, mapping policy, and synthetic fixtures as the reference for Sakura support. The branch-added methods are candidate integration points, not existing main-branch extension APIs that the design must preserve. In particular, a generic string table must not inherit the patch decoder's compression feature gate.

The raw archive-entry helper is worth retaining as a generic internal operation: a mapping to `patch.bin` must open that archive entry, without unexpectedly resolving a loose `patch/<entry>` file. User-facing loose overrides still resolve at the requested logical resource path. Regression tests should compare both normal and mixed-case folder requests with main's existing two-stage loose-file lookup.

When implementation begins, use an isolated branch/worktree from the selected main revision and port the necessary patch behavior into the generic architecture. This is a proposed implementation baseline, not a branch change performed during this research. Validate plugins-disabled behavior against main, and plugin-enabled behavior against the current patch branch; neither comparison alone is sufficient.

**Recommendation**

Introduce a small, versioned extension contract for preparing a game session. Use the Sakura Moyu Chinese patch as the first plugin. Initially, plugins produce string replacements and resource mappings during startup; RFVP owns and uses the resulting data during execution.

Separate that contract from the plugin execution backend. First extract and verify the existing patch as a statically registered implementation. Then prove that an independently built external module can produce the same results without rebuilding RFVP. WebAssembly is a candidate for that second step, not a settled runtime dependency.

This is narrower than a general engine scripting system, but it directly supports the current patch and establishes a path to additional archive formats, resource transformations, and compatibility functions. New plugins can use existing capabilities without engine changes; adding an entirely new capability still requires an API and engine update.

**What the code currently does**

| Area | Evidence | Consequence for the design |
| --- | --- | --- |
| Patch activation | [`main.rs`](../../crates/rfvp/src/main.rs), `CliArgs`, `require_sakura_moyu_chs_patch_files`, `main` | `--sakura-moyu-chs-patch` requires both files, installs strings into the parser, and separately selects a special VFS constructor. Activation is wired into the desktop GPU CLI. |
| String format | [`script/string_patch.rs`](https://github.com/appe233/rfvp/blob/8f6458f234eead7a702606694a0ef30224a3a14b/crates/rfvp/src/script/string_patch.rs), `StringPatchTable` | `patch.dat` has a 16-byte header and a zlib payload containing HCB offsets and GBK strings. Parsing is format-specific and belongs in the plugin. The resulting offset-to-UTF-8 table is generic engine data. |
| String substitution | [`script/context.rs`](../../crates/rfvp/src/script/context.rs), `Context::push_string`; [`script/parser.rs`](../../crates/rfvp/src/script/parser.rs), `patched_string` | Substitution happens at opcode `0x0e`, after original-string decoding. The VM advances by the original encoded length and keeps `Variant::ConstString(text, original_offset)`. |
| Observable string identity | [`syscalls/text.rs`](../../crates/rfvp/src/subsystem/components/syscalls/text.rs), `text_print` | The original offset controls read-history marking and the syscall return value. This affects script control flow, not just presentation. |
| Resource overlay | [`resources/vfs.rs`](../../crates/rfvp/src/subsystem/resources/vfs.rs), `new_with_sakura_moyu_chs_patch`, `build_sakura_moyu_patch_overrides` | `patch.bin` already uses the normal archive reader. The special behavior is matching an entry name to exactly one original archive, removing `patch` from the base archive set, and registering an override. Ambiguous and unmatched entries are warned about and skipped. |
| Resolution and streaming | Same VFS file, `open_stream_with_len`, `open_packed_stream_with_len`, `SubFile` | Loose files win over the patch; the patch wins over original archive entries. Archive slices are seekable and need not be loaded in full. |
| Consumers | [`graph_buff.rs`](../../crates/rfvp/src/subsystem/resources/graph_buff.rs), [`bgm_player.rs`](../../crates/rfvp/src/audio_player/bgm_player.rs), [`se_player.rs`](../../crates/rfvp/src/audio_player/se_player.rs), [`movie.rs`](../../crates/rfvp/src/subsystem/components/syscalls/movie.rs) | Graphics, audio, and movie loading use VFS entry points. One generic overlay layer can cover these consumers without separate decoder hooks. |
| Font loading | [`text_manager.rs`](../../crates/rfvp/src/subsystem/resources/text_manager.rs), `load_font_file`, `init_fontface` | Fonts are scanned/read directly from the filesystem. Resource overlays alone do not provide font installation or font-face aliases. |
| Thread ownership | [`vm_worker/native.rs`](../../crates/rfvp/src/vm_worker/native.rs), `VmWorker::spawn`; VFS stream aliases | The native VM owns the parser on another thread and accesses shared game data under a lock. Normal desktop VFS streams require `Send + Sync`; `rfvp-os` has a different bound. Arbitrary runtime callbacks would require a deliberate threading and reentrancy contract. |
| Save restoration | [`save_state.rs`](../../crates/rfvp/src/subsystem/save_state.rs), `SaveStateSnapshotV1`; [`script/context.rs`](../../crates/rfvp/src/script/context.rs), `ContextSnapshotV1` | Saves persist VM stacks/globals, including string contents and offsets. Restoration reloads scene/audio resources through the current VFS. A changed plugin profile can therefore mix old text with new resources. |
| Syscall extension | [`world.rs`](../../crates/rfvp/src/subsystem/world.rs), `SYSCALL_TBL`, `GameData::do_syscall` | There is a process-global built-in registry whose implementations receive `&mut GameData`. It is not a suitable public plugin ABI. Existing `AudioLoad2`/`GraphLoad2` aliases are possible later compatibility-plugin cases. |

The existing tests cover GBK decoding, header-size mismatches, truncated records, duplicate offsets, trailing NUL removal, preserved string addresses, unambiguous archive mapping, and loose/patch/original precedence. These are useful migration fixtures; this research did not execute them.

Two details should remain explicit during extraction. The current `patch.dat` reader checks sizes but does not validate the first eight header bytes as a signature. It also decompresses before comparing the output size. Establish the actual format and introduce bounded decoding deliberately rather than treating an assumed signature or unlimited allocation as part of the new public contract.

**Startup and platform boundaries**

| Host | Current startup path | Preliminary integration scope |
| --- | --- | --- |
| Desktop GPU CLI | [`main.rs`](../../crates/rfvp/src/main.rs) → [`boot.rs`](../../crates/rfvp/src/boot.rs) → `AppBuilder` | First implementation and external-plugin proof. |
| Desktop library / pump launcher | [`lib.rs`](../../crates/rfvp/src/lib.rs), `run_rfvp`, `rfvp_pump_create` | Reuse the same preparation service; existing exported functions retain their signatures and default to no plugins. Add an options-aware entry point when exposed to launchers. |
| iOS / Android | [`ios_host.rs`](../../crates/rfvp/src/ios_host.rs), [`android_host.rs`](../../crates/rfvp/src/android_host.rs) | Share preparation logic, then add explicit host options. External module execution is a separate platform deliverable. |
| Desktop software renderer | [`soft_host.rs`](../../crates/rfvp/src/soft_host.rs) | Has separate argument parsing, script discovery, and VFS construction. Include after the first CLI path, without introducing GPU dependencies. |
| Browser | [`wasm_entry.rs`](../../crates/rfvp/src/wasm_entry.rs), [`wasm_app_path.rs`](../../crates/rfvp/src/wasm_app_path.rs) | Receives selected-file metadata and uses host-provided range reads. Adapt preparation to file handles/bytes; do not depend on native paths. Browser RFVP being compiled to WASM does not by itself provide a guest-plugin loader. |
| UEFI / `rfvp-os` | [`rfvp_os_host.rs`](../../crates/rfvp/src/rfvp_os_host.rs) | Keep builds working with plugins disabled; consider static registration after desktop validation. |
| `no_std` / console hosts | [`no_std_core.rs`](../../crates/rfvp/src/no_std_core.rs), [`host_api/fs.rs`](../../crates/rfvp/src/host_api/fs.rs), [`vfs_host.rs`](../../crates/rfvp/src/subsystem/resources/vfs_host.rs) | Preserve the independent feature profile. Define portable data types, but do not promise dynamic loading or full patch support in the first release. |

The active `no_std` core uses the real `script::Parser`, `GameData`, and `VmRunner`. Its VFS is a different implementation selected in `resources/mod.rs`: normal host builds load archive bytes into memory, while the `old_school` path currently skips that archive population and uses paged HCB reads. The files under `src/portable/` are not declared as a module in the inspected `lib.rs`; they should not drive this migration.

There are also two compilation surfaces: `main.rs` declares engine modules itself, while `lib.rs` exposes the library modules. New modules and dependencies must be wired into both, or moved behind a shared library entry point in a separately justified change.

`GameData` has both a normal `Default` implementation and an unsafe field-by-field `init_default_in_place` used by `AppBuilder`. Any added session field must be initialized in both paths. Both currently construct `Vfs::default()`, which scans archives on ordinary desktop builds before builder methods replace it. A prepared-game constructor should install the prepared VFS directly, avoiding another scan and a default-initialization panic before plugin diagnostics can be returned; preserve existing public default behavior unless separately changed.

`App::find_hcb` prefers `*.bch` before `*.hcb`. The software host searches for `*.hcb`, and browser startup selects a root-level `.hcb`. Preserve those defaults during extraction and fingerprint the script actually selected. A future script-selection capability must run before parsing. Do not silently unify these behaviors while implementing plugins.

`boot::load_script` can update the base path, while patch-file validation currently happens before it. Plugin preparation should receive the resolved game root explicitly, after script discovery. Avoid adding plugin configuration to the existing process-global environment variables.

**Proposed version-one contract**

A plugin prepares an immutable `PluginPlan` from a `GameContext` and a restricted `PluginHost`. These names describe responsibilities rather than finalized Rust signatures.

| Concept | Proposed contents |
| --- | --- |
| `GameContext` | Selected script identity, content digest, script encoding, parsed metadata, resolved game-file namespace, and host capabilities. |
| `PluginHost` | Read game files through bounded handles/ranges, inspect or mount supported FVP archives, enumerate archive metadata, and emit diagnostics. File access is independent of the active overlay chain. |
| `PluginPlan` | Script-bound UTF-8 replacements, resource mappings to host-owned sources, plugin-owned archive claims, and diagnostics. |
| `PluginSession` | Validated plugin identities/order and committed tables/mounts for one game session. No process-global plugin registry or mutable gameplay state. |

The first resource mapping forms should be a logical path mapped to an entry in a mounted FVP archive, or to bounded immutable bytes. Both can be served by existing host stream machinery. Avoid a generic plugin callback on every resource read in version one.

The Sakura Moyu plugin would:

1. Check the selected script identity and the required `patch.dat`/`patch.bin` files.
2. Decode `patch.dat` into offset-keyed UTF-8 replacements, retaining current decoding semantics.
3. Claim `patch.bin` as its source archive so it is excluded from the original-archive matching set.
4. Compare its entry names with original archive metadata, emitting mappings only for unique matches.
5. Return the plan for validation and atomic installation.

The plugin owns the matching policy. RFVP owns generic string lookup, stream opening, and overlay precedence. This removes the game name and format decoder from parser/VFS behavior.

Use a transaction-like preparation process: gather claims and tentative mounts, construct the original archive view, prepare each plugin in explicit order, validate all output, then install everything before starting the VM. A failed required plugin aborts launch with its ID and the failing operation; it must not leave only half of a patch installed.

For multiple plugins, use the supplied list order, with later entries winning duplicate string offsets/resource paths. Report each conflict with both plugin IDs. Duplicate plugin IDs are errors. Preserve loose-file precedence. Matching against original archive metadata must not depend on mappings installed by earlier plugins.

Validate API compatibility, required capabilities, duplicate records within a plugin, mapping sources, string offsets within the selected script's code area, and output/memory limits. Exact instruction-boundary validation needs a proper instruction walk; a simple range check must not be described as proof that an offset identifies a string.

Ordinary game paths retain existing case/encoding behavior. Desktop pack lookup lowercases the folder but preserves inner entry names; browser and host VFS normalization differ. Do not apply a new global lowercase rule. Represent mounted entries with opaque handles where possible, and test case-sensitive and non-ASCII names.

Keep the replacement hook at `push_string` or a narrowly named script-literal accessor. `Parser::read_cstring` also reads the title and syscall names; adding replacements there would unintentionally expand patch semantics. Preserve original decoding/error behavior, the original byte cursor advance, and `ConstString` identity.

The script NLS remains independent of the translation table's GBK encoding. A plugin should not change the whole game to GBK merely because its translation data uses GBK. The existing 512-byte `TextPrint` limit also remains engine behavior; include a boundary test and diagnostics rather than changing it incidentally.

**Code organization and loading**

Proposed locations:

```text
crates/rfvp-plugin-api/                 portable contract types and versioning
crates/rfvp-plugin-sakura-moyu-chs/      patch decoding and resource matching
crates/rfvp/src/plugins/                preparation, validation, engine adapters
crates/rfvp-plugin-wasm/                optional external backend, if selected
```

Keep dependency direction one-way: the game plugin depends on the API, never on the `rfvp` engine crate. The engine depends on the API and optionally a static plugin implementation. API data should be usable with `alloc`; filesystem/runtime dependencies belong in adapters. An in-process Rust trait is convenient internally, but must not be treated as the external binary ABI.

| Backend | Assessment |
| --- | --- |
| Static Rust registration | Best way to prove extraction and support constrained builds. This alone does not fulfill install-without-rebuild. |
| External WebAssembly module | Preferred experiment for a portable package. Start with startup-only execution, explicit host imports, bounded memory/work, and copied registration data or host-owned handles. Measure runtime size and startup cost before selecting a runtime. |
| Native library with a C ABI | Alternative if desktop-native extension performance becomes necessary. Requires versioned function tables, explicit ownership/freeing, target-specific packages, and panic/error containment. Never expose Rust `String`, trait objects, or engine references across the ABI. |

No RFVP plugin runtime is currently declared in the workspace manifests. `libloading` appears transitively in the lockfile, which does not constitute an existing engine plugin loader. A backend decision still needs a small prototype and current dependency documentation; no library-specific feasibility claim has been verified here.

A first external package can be an ordinary directory containing `plugin.json` and a module. Prefer JSON initially because the standard runtime already depends on `serde_json`; do not force manifest parsing into `no_std`.

Proposed manifest fields: schema version, plugin ID/version, required host API major/minor, required capabilities, entry module, and supported script identities. Distinguish package version from API version. Declare capabilities such as `script-string-table-v1` and `resource-mapping-v1`; reject unsupported requirements before executing a module.

Activation should be explicit, for example:

```sh
rfvp --project-dir ./game --plugin ./plugins/sakura-moyu-chs
```

Allow repeatable `--plugin` options; defer automatic directory scanning, dependency resolution, downloads, and hot reload. Retain `--sakura-moyu-chs-patch` as an alias for the bundled plugin during migration, using the same preparation path and deduplicating the alias against explicit activation.

A strict external package should enumerate supported script hashes. The current implementation does not check those hashes, so collecting verified identities is a release prerequisite for such packages. The legacy alias may retain an explicitly reported unverified compatibility mode during migration; file presence alone is not proof of compatibility.

**Save and lifetime policy**

Version one permits no mutable plugin gameplay state. Startup-produced data and mounted sources remain valid for the session. An initialization-only module can be destroyed after its output has been copied/validated if no guest-owned handles remain; any future callback provider must stay alive until every stream is closed.

Same-profile save/load must retain today's behavior. Cross-profile loading is not automatically safe: serialized strings can survive without passing through `push_string`, while textures/audio are reloaded through the current VFS.

Before a public external-plugin release, add a versioned save compatibility identity covering the selected script, ordered plugin/module identities, and relevant patch content digests. New saves with conflicting known identities should fail before restoring resources, with a clear diagnostic. Older saves have unknown identity and should remain loadable with an explicit diagnostic. Do not claim that old saves can be reliably classified or automatically retranslated.

Design this metadata around the existing `RFVS` footer and strict bincode decoding. Appending arbitrary bytes after that footer or adding fields to `SaveStateSnapshotV1` would break the current decoder. Use a separately designed compatible envelope/version path, retain V1 reading, and test legacy saves. No save-format change is needed for the first extraction milestone.

**Implementation sequence and completion criteria**

| Milestone | Work | Completion criterion |
| --- | --- | --- |
| 1. Generic hooks and static extraction | Separate the immutable string table from the patch decoder; replace the named VFS overlay with generic mappings; move Sakura decoding/matching into its crate; preserve the CLI alias. | Existing synthetic patch tests pass through the plugin path, unpatched behavior is unchanged, and parser/VFS contain no Sakura-specific logic. |
| 2. Shared preparation and configuration | Introduce explicit per-session preparation options; return prepared parser/VFS/session data; add prepared-game construction that avoids the default VFS rescan; wire desktop CLI, library/pump, and software host. | All supported desktop entry points use the same activation/validation logic; legacy exported entry points continue to work with plugins disabled. |
| 3. External loading proof | Prototype the startup-only WASM backend; implement version/capability checks and bounded host imports; package Sakura separately; evaluate the native ABI alternative if the prototype fails its budget. | A fixed RFVP binary loads an independently rebuilt plugin, and a second synthetic plugin produces different strings/resources without an engine rebuild. Record binary size, initialization time, and peak memory. |
| 4. Release compatibility | Finalize package/SDK documentation, diagnostics, known script identities, save-profile metadata, teardown behavior, and regression coverage. | Same-profile saves restore correctly; known mismatches are caught before restoration; old saves retain the documented path; plugins-disabled builds remain unaffected. |
| 5. Additional hosts and capabilities | Adapt browser selected-file storage, mobile launcher options, and static `no_std` registration according to demand and measured cost. | Each newly supported host has explicit capability reporting and platform validation; unsupported backends fail clearly. |

Keep the public scope of each milestone honest: milestones 1–2 improve architecture and bundled patch support; milestone 3 first demonstrates the user's install-without-rebuild goal. They are sequential reviewable changes, not a single broad engine rewrite.

**Verification plan for implementation**

Use synthetic HCB, `patch.dat`, and archive fixtures. Do not commit proprietary scripts, translations, or game assets.

- Establish two reference suites: plugins disabled must preserve main's original string/loose-file/archive behavior; the Sakura plugin enabled must reproduce this branch's patch behavior. Cover the source-archive loose-file bypass and mixed-case folder lookup explicitly.
- Carry over existing decoder, preserved-address, mapping, and precedence tests. Add a translated string longer than the original encoded string and assert the next opcode/cursor and `ConstString` address remain unchanged.
- Exercise missing string fallback, independent source/translation encodings, malformed compressed data with bounded output, and the current text-length boundary.
- Verify unmatched and ambiguous entries, two-plugin conflicts, stable ordering, original-archive matching, claimed archive exclusion, and case/non-ASCII names.
- Exercise the production VFS entry point as well as the current test-only `read_file_at_base` helper. Test seek/read boundaries on a mapped archive entry without buffering the whole archive.
- Verify failed plugin preparation installs neither strings nor resource mappings. Check unsupported API/capability rejection, wrong script identity, invalid handles, oversized output, and a trapped external module.
- Test native worker startup and teardown with prepared data. Confirm no plugin execution occurs per text line or per streamed audio read in version one.
- Exercise save/load with the same profile, a known different profile, and legacy V1 data. Include a persisted `ConstString` and a resource reopened during restore.
- Use an authorized local game installation for a manual smoke test of dialogue, choices, translated UI images, skip/read history, audio, and save/load. Synthetic tests cannot establish full game compatibility.

For Rust changes, run `cargo fmt --all`, focused crate tests, then the repository-required `cargo test --workspace` and `cargo clippy --workspace --all-targets` before a PR. Record pre-existing failures or missing platform dependencies separately.

Feature checks should include default desktop, `--no-default-features --features soft-render-desktop`, `--no-default-features --features no_std --lib`, and `--no-default-features --features no_std,old_school --lib`. Use the existing WASM and UEFI build scripts when those hosts are touched; execute target builds on an appropriate toolchain rather than reporting unbuilt configurations as supported. Test both CLI and library surfaces. Add checks that plugin features do not pull a dynamic runtime into `no_std`.

**Deferred capabilities and unresolved decisions**

Custom encrypted/compressed streaming providers require an open/read-at/length/close contract, independent stream state, explicit error-versus-not-handled results, and threading rules. The existing `RfvpFile::read_at` abstraction is a useful model, but desktop and host VFS implementations still need separate adapters. Give plugins a base-resource API that bypasses their own overlay to prevent recursion. Implement this after the startup-only proof demonstrates a concrete need.

Font bytes and font aliases need dedicated registration points because fonts bypass the VFS. Script selection or preprocessing needs an earlier boot stage. New syscalls need per-session dispatch and typed values without exposing `GameData`; they must account for yielding and persistence. Rendering/audio backend replacement and arbitrary opcode interception are outside the initial scope.

The remaining decisions are the external runtime and supported first-release targets, memory/startup budgets, verified game identities, and the backward-compatible save metadata representation. Resolve these with the external-module prototype and fixtures before freezing API version 1.
