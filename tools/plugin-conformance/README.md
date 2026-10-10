# External preparation package conformance

This exercises RFVP's actual shared preparation service, parser string instruction, and streaming VFS. It is not a rendering/audio or real-game test. ABI 1.0 remains a candidate; external loading is experimental. New resumable saves record the plugin profile, and loads reject known mismatches before any state is restored (see `setsumei/plugins/CHANGE-4-RESULTS.md`). For authorized real-game equivalence and Linux checks, see `tools/plugin-real-data/README.md`.

## Run the synthetic verification

Prerequisites: the repository's Rust toolchain, `wasm32-unknown-unknown`, and Python through `uv`. Run from the repository root:

```sh
uv run --no-project python tools/plugin-conformance/run.py
```

The runner writes only generated synthetic data and artifacts under `target/plugin-conformance`. It builds one headless RFVP executable, records its SHA-256, builds the actual Sakura decoder/matcher as a standalone Wasm guest, and runs six cases:

1. Plugin-disabled original strings/resources.
2. Bundled Sakura activation.
3. External Sakura activation with the same synthetic inputs.
4. A normal independently compiled conformance module.
5. An independently rebuilt alternate conformance module, without rebuilding RFVP.
6. Sakura activated before another plugin that claims a matching archive entry: all claims must already be removed from Sakura's original-archive view.

It asserts original payload address/cursor semantics and seeks/reads mapped resource bytes, compares bundled and external Sakura outputs, checks changed implementation identities, and verifies the host executable digest stays unchanged. It emits `results.json` and generated package directories containing `plugin.json` and `plugin.wasm`. Only a synthetic script hash is supplied; this is not a verified real-game package.

## Build a desktop host

External loading is opt-in and does not imply the bundled adapter:

```sh
cargo run -p rfvp --features external-plugins -- \
  --project-dir /path/to/game --plugin /path/to/package

cargo run -p rfvp --no-default-features \
  --features soft-render-desktop,external-plugins -- \
  --project-dir /path/to/game --plugin /path/to/package
```

Repeat `--plugin` in the desired order. Later plans override earlier string/resource keys with diagnostics. Duplicate plugin IDs and overlapping archive claims are errors. A failed launch does not activate an earlier plugin's partial plan. Paths supplied to `--plugin` are relative to the caller's working directory, independently of the game root. GPU startup continues to prefer `.bch`; software startup continues to select `.hcb` only.

`--sakura-moyu-chs-patch` requires the `sakura-moyu` feature, except when an explicit package with ID `rfvp.sakura-moyu-chs` is already selected. In that case the explicit package wins and stays in its requested position; its script-hash restriction is not relaxed. Without that explicit package the bundled alias is appended after the explicit list and warns that its selected script identity is unverified. Repeated explicit IDs are still errors.

The desktop C APIs `rfvp_run_entry_v1` and `rfvp_pump_create_v1` accept bounded, length-delimited JSON:

```json
{"plugins":["/path/to/package"],"sakura_moyu_chs_patch":false}
```

Old entry points continue to select an empty plugin list. See `platform/macos/RFVPLauncher/Headers/rfvp.h` for signatures; no mobile/browser activation is added.

## Build the actual Sakura guest

```sh
cargo build -p rfvp-plugin-sakura-guest --target wasm32-unknown-unknown --release
uv run --no-project python tools/plugin-conformance/package_sakura.py \
  --module target/wasm32-unknown-unknown/release/rfvp_plugin_sakura_guest.wasm \
  --output target/my-sakura-package \
  --script-sha256 YOUR_VERIFIED_64_LOWERCASE_HEX_HASH
```

The placeholder is intentionally invalid. The script identity verified so far on authorized data is `946877dd0ed8fbf318ba5c73d20afd46b9dcdc200aa7e8edc610c437cc1c789b` (`Sakura.hcb`, 5,002,852 bytes; see `setsumei/plugins/CHANGE-5-RESULTS.md`). It is also recorded as `rfvp_plugin_sakura::VERIFIED_SCRIPT_SHA256`, which suppresses the bundled alias's unverified-identity warning for that script. Supply hashes the package author has actually verified with authorized data; the helper does not certify compatibility or include game assets. Repeat `--script-sha256` for additional verified scripts. Output must be a new directory. External packages cannot opt into the bundled alias's unverified mode.

The guest uses `rfvp-plugin-sdk::guest::{context, read_source, submit_plan}` and the same portable `rfvp-plugin-sakura::prepare` decoder/matcher as the bundled implementation. Build without WASI. The synthetic `rfvp-plugin-conformance` guest reads a bounded plan from `plan.json`; `--features alternate` changes its output so module replacement is observable.

## Validation and host policy

The complete candidate contract is specified in `setsumei/plugins/PLUGIN-ABI-V1.md`; this implementation uses `rfvp-plugin-sdk` as its shared typed wire contract. The production boundary is pinned to Wasmi 2.0.0 with validation, strict compilation limits, fuel, no starts/WASI, one wasm32 memory, and at most one table. Runtime state is destroyed after a successful, copied and validated plan.

Additional finite host-side policy: 256 inputs/claims and 512 supported-script hashes per manifest; at most 1,024 archive metadata parses, 100,000 archive entry records and 100,000 filename-table records across a session; 8 MiB aggregate metadata accounting; 1 MiB conflict-detail diagnostics followed by a suppressed-count summary. Metadata accounting includes raw headers/tables, decoded retained names, and logical original paths, so its budget can fail before the serialized 8 MiB context limit. Empty filename records consume counts. These are candidate implementation limits, not new plugin capabilities.

Declared paths are opened relative to directory capabilities and retained as regular-file handles, rather than canonicalized and later reopened. Aliased claims are detected by file identity. Original/plugin streams use independently positioned views over the retained source. Replacing a pathname does not replace an already prepared source; truncating or changing the same open file remains unsupported and subsequent read errors are not hidden by original-resource fallback. Streaming hashing uses a 64-KiB buffer; large archives are not copied into host vectors or Wasm.

When plugins are disabled the existing VFS discovery path is unchanged. Plugin-enabled preparation fails on malformed/unopenable discovered `.bin` archives or metadata budgets instead of silently skipping them; this stricter preflight is deliberate and diagnosed. Filesystem latency/hashing and host RSS are not bounded by guest fuel or the linear-memory quota.

Errors expose a plugin/package label plus a typed stage: `Manifest`, `Inputs`, `Metadata`, `Module`, `Execute`, `Plan`, or `Compose`. Fix invalid declarations/claims before retrying; a script mismatch requires a compatible package, not a force flag. Fuel, memory, metadata, and copy-limit failures are launch errors, never invitations to silently start the unpatched game.

Verified environments: macOS 15.8 arm64 (synthetic conformance, authorized real-data equivalence, and a GPU CLI gameplay smoke test) and Linux in Debian bookworm containers (builds, focused tests, conformance, and real-data equivalence; no window or audio). Windows is not yet verified. ABI 1.0 is not frozen. The fuel budget was raised to 10,000,000,000 after real Sakura data needed about 3.69 billion; see `setsumei/plugins/CHANGE-5-RESULTS.md`.
