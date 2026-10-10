# Change 1: generic preparation data and Sakura extraction

Branch `feat/plugin-data`, based on main `ffc7380`. The reference branch remains `sakura-moyu-patch-loader` at `8f6458f`.

This implements the generic data and static extraction milestone. It does not load external plugins or add launch options. The Rust API remains unstable.

## Implemented

- `rfvp-plugin-api` is dependency-free and uses `no_std + alloc`. It owns immutable validated string replacements, resource mappings, original-entry metadata, and preparation plans. It rejects duplicate offsets/canonical paths, invalid paths, NUL text, out-of-range offsets, and excessive counts/output sizes. Resource entry names remain opaque and case-sensitive.
- `rfvp-plugin-sakura` depends on the API plus `encoding_rs` and `miniz_oxide`, with no RFVP engine dependency. It decodes GBK, strips one trailing NUL, rejects malformed/duplicate records, and matches only unique original archive names. It reports unmatched/ambiguous names through a borrowed diagnostic callback. Header bytes 0–7 remain uninterpreted.
- Sakura limits are 16 MiB for the complete compressed input and decompressed payload, 100,000 records, 64 KiB per decoded replacement, and 16 MiB aggregate decoded text. The decompressor cannot grow beyond the bounded declared size. Counts are validated before record allocation; UTF-8 expansion is checked before retaining text.
- Parser replacements are independent of `zlib-flate2`. The original string is decoded and bounds-checked first. `ConstString` retains the original payload address, and the VM advances by the original encoded length. Missing mappings preserve the original string; an empty replacement is effective. Shared tables use `Arc` normally and `Rc` for the existing `old_school` profile.
- VFS mappings validate their archive destinations before installation, retain seekable archive slices, and open mapped entries directly without source-folder loose overrides or recursive resolution. Both native loose-file lookup opportunities precede mappings. Only the logical archive folder is ASCII-lowercased.
- The optional RFVP feature `sakura-moyu` exposes `plugins::prepare_sakura(parser, vfs, resolved_root)`. Both required inputs must open successfully. The source archive is removed from the original metadata view; parser and VFS are returned together after validation. Binary and library module declarations are wired. No CLI behavior is added in this milestone.

The reference branch allowed a mapping to intercept a mixed-case folder request before the second loose-file lookup. This implementation deliberately preserves main's precedence in that case, as specified by the RFC.

## Validation

Executed on macOS/aarch64 with the installed stable Rust toolchain. Only synthetic data was used; no game files were added.

| Command | Result |
| --- | --- |
| `cargo test -p rfvp-plugin-api -p rfvp-plugin-sakura` | 7 passed |
| `cargo test -p rfvp --lib --no-default-features --features soft-render-core,no-audio,sakura-moyu plugin` | 5 passed, including static Sakura preparation through VM and VFS |
| Same profile, filter `string_patch_tests` | 4 passed |
| Same two RFVP test commands without `sakura-moyu` | 4 VFS and 4 string tests passed |
| `cargo check -p rfvp-plugin-api -p rfvp-plugin-sakura --target wasm32-unknown-unknown` | Passed; both crates compile as `no_std + alloc` |
| `cargo clippy -p rfvp-plugin-api -p rfvp-plugin-sakura --all-targets -- -D warnings` | Passed |
| `cargo check -p rfvp --no-default-features --features soft-render-desktop,sakura-moyu --lib` | Passed with existing warnings |
| `cargo check -p rfvp --features sakura-moyu --bins` | Passed with existing warnings |
| `cargo check -p rfvp --no-default-features --features wasm --target wasm32-unknown-unknown --lib` | Passed with existing warnings |
| `cargo clippy -p rfvp --no-default-features --features soft-render-core,no-audio,sakura-moyu --lib --tests` | Passed with existing warnings |
| `cargo test --workspace` | Blocked: existing console dependency `linked_list_allocator` enables unstable allocator features on stable Rust |
| `cargo clippy --workspace --all-targets` | Blocked: existing deny-by-default lints in unchanged WMV/MPEG decoders, including erasing operations and raw-pointer FFI |
| `cargo check -p rfvp --no-default-features --features no_std --lib` and `no_std,old_school` | Blocked: missing `Path` in `text_manager.rs`, missing panic handler, and unwinding without std. The same three errors were reproduced on unchanged main with `no_std`. |
| `cargo fmt --all` | Run; unrelated baseline formatting churn removed. New/edited implementation files formatted. |
| `uv run --no-project python tools/plugin-reference/run.py --fixture-root target/plugin-reference/mount --require-case-sensitive` | Passed: 29 process runs, nine reference comparisons across Shift-JIS/GBK/UTF-8; see [comparison results](CHANGE-1-COMPARISON.md) |
| `git diff --check` | Passed |

The no-`zlib-flate2` test profile exposed a missing test-only `std::io::Read` import in existing texture tests; that one-line fix is included.

Tests cover long replacement text, unchanged cursor/next opcode/address/NLS/title, bounds failure before replacement, empty replacement, missing entry, GBK expansion limits, malformed headers/streams/records, duplicate offsets, ambiguous and unmatched archive names, Unicode and case-sensitive entry keys, source loose-file bypass, new logical resource paths, seek/read/EOF boundaries, invalid destinations, and mapped-source failure without original fallback. The distinct-case loose-directory assertion executes only on case-sensitive filesystems; portable folder-normalization checks always execute.

The standalone [reference comparison](CHANGE-1-COMPARISON.md) additionally exercised distinct-case paths on a temporary case-sensitive HFSX volume. Main and the Sakura branch were executed through a shared probe against byte-identical synthetic fixtures; no production code was modified in either reference worktree.

## Remaining gates

- Perform an authorized real-game smoke test. Synthetic comparative execution against both reference revisions is complete; game-level equivalence remains unverified.
- Introduce shared launch options, prepared-game construction, session identity, conflict composition, and CLI/library/software-host activation in Change 2. `--plugin` and `--sakura-moyu-chs-patch` are not exposed yet.
- Bound original archive metadata construction before the existing archive parser allocates it, and replace path-based archive reopening with stable production sources and race-resistant opening. The static adapter currently inherits RFVP's archive parser and streaming file behavior; it is not the production external-plugin input service.
- Add external Wasm execution, manifests/script-hash binding, independently packaged guests, and profile-aware saves in later changes. The Wasm compile check above does not produce an ABI guest package.
- Validate Windows/Linux and UEFI/platform packaging. No platform scripts were changed or run for this milestone.
