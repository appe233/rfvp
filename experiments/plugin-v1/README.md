# RFVP plugin ABI experiment

Standalone research workspace for [RFC 001](../../setsumei/plugins/PLUGIN-SYSTEM-RFC.md). It does not depend on RFVP or change the root workspace. Wasmi 2.0.0 executes a separately compiled Rust/Wasm guest that reads synthetic translations and registers strings plus archive mappings.

**Run**

Requirements: Rust 1.95 was tested, the `wasm32-unknown-unknown` target, and Python through `uv`. If needed, install the target with `rustup target add wasm32-unknown-unknown`.

From the repository root:

```sh
cargo test --manifest-path experiments/plugin-v1/Cargo.toml -p rfvp-plugin-probe
cargo clippy --manifest-path experiments/plugin-v1/Cargo.toml -p rfvp-plugin-probe --all-targets -- -D warnings
cargo clippy --manifest-path experiments/plugin-v1/Cargo.toml -p rfvp-plugin-example --target wasm32-unknown-unknown -- -D warnings
cargo fmt --manifest-path experiments/plugin-v1/Cargo.toml --all --check
uv run --no-project python experiments/plugin-v1/run.py
```

Use explicit package/target selections: the guest is a wasm32 module, not a native program. The runner builds release binaries and two guest variants, creates synthetic fixtures, verifies the host executable stays unchanged, and measures eleven samples per case after warm-up. It records RSS using the system `time` tool on macOS/Linux; on other hosts RSS remains null. Windows/Linux execution has not yet been validated.

The runner assumes Cargo's default target directory for this isolated workspace; unset `CARGO_TARGET_DIR` when reproducing. Dependencies are pinned by this workspace's own `Cargo.lock`; the root lockfile is unchanged.

Generated packages, translation/pack fixtures, compiled modules, and machine-readable measurements are under ignored `experiments/plugin-v1/target/probe-results/`. Nothing requires proprietary data. The small pack has a standard FVP-shaped header/index but synthetic resource bytes; the script identity fixture is not executable HCB.

After running, a package can be loaded directly:

```sh
experiments/plugin-v1/target/release/probe \
  experiments/plugin-v1/target/probe-results/normal \
  experiments/plugin-v1/target/probe-results/small
```

The JSON report contains the validated plan, module digest, preparation duration, final linear-memory size, fuel consumption, and total transfer bytes. `input_bytes_read` includes context copies and submitted output as well as source reads; it is the aggregate copy budget counter. The baseline binary reads fixture JSON/source bytes without linking the probe library/runtime.

**What is implemented**

- A bounded manifest/context loader and SHA-256 source validation.
- Explicit input declarations and archive claims.
- Four host imports, version negotiation, no start sections or WASI, fuel and memory limits.
- Copied tentative output that is returned only after successful guest completion and validation.
- Synthetic guest matching that accepts unique archive names and skips ambiguous/unmatched names.
- Independently rebuilt normal/alternate guests against a fixed executable.
- Ten tests of accepted output, failed execution, buffer/handle validation, and registration semantics.

**What remains outside this experiment**

The engine must produce trusted context metadata from the actual script/archive files. This probe trusts supplied fixture code ranges and archive metadata, loads small source files into vectors, and does not implement RFVP's VM, archive streaming, zlib/GBK patch decoding, multiple-plugin session commit, save profiles, or production path-opening guarantees. It is not a complete SDK or a supported game launcher.

The exact candidate contract and those integration requirements are in [PLUGIN-ABI-V1.md](../../setsumei/plugins/PLUGIN-ABI-V1.md). Measurements and their limits are in [PLUGIN-PROTOTYPE-RESULTS.md](../../setsumei/plugins/PLUGIN-PROTOTYPE-RESULTS.md).
