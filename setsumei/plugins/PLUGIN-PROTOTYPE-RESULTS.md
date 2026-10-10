# RFVP plugin prototype results

The standalone experiment supports selecting Wasmi for the desktop implementation proposed by [RFC 001](PLUGIN-SYSTEM-RFC.md). It demonstrates independently compiled external modules, bounded host calls, validated output, and discard-on-failure. It does not integrate plugins into RFVP or establish real-game compatibility.

**Environment and method**

- Host: Apple Silicon arm64, macOS 15.8.
- Compiler: `rustc 1.95.0 (59807616e 2026-04-14)`.
- Interpreter: Wasmi 2.0.0, `default-features = false`, `stable`, `std`, `validate`, `portable-dispatch`; no WASI or WAT parser in the release loader.
- Release profile: size optimization, LTO, one codegen unit, stripped symbols, aborting panics.
- Guest target: `wasm32-unknown-unknown`, Rust standard library plus Serde JSON, compiled independently of the host.
- Each case: one warm-up and eleven fresh-process samples with warm filesystem caches. RSS measured in a separate `/usr/bin/time -l` run, in bytes.
- Fixtures: two or 10,000 synthetic Chinese translations; a small standard-FVP-shaped archive; unique, ambiguous, and unmatched metadata entries. The synthetic script bytes identify the fixture and are not a runnable HCB program.

The runner builds the host once, hashes it, builds a normal guest and then a modified guest with the `alternate` feature, and runs both against the same executable. It verifies the host hash did not change, exact first-string content, record count, and the sole expected resource mapping. The alternate guest appends a suffix to translations. Both skip ambiguous and unmatched archive entries.

The guest reads translation JSON through the host import and performs its matching policy inside Wasm. The host receives the plan after the guest exits, validates it, and returns host-owned data. Archive metadata is supplied by fixture JSON; the probe does not run RFVP's archive parser or open the returned mapping as a game resource.

**Recorded measurements**

These are the final recorded run after rejecting start sections. Timings are rounded; the runner retains full precision in ignored `target/probe-results/results.json` when rerun.

| Fixture | Variant | Preparation median | Whole-process median | Whole-process max | Peak process RSS |
| --- | --- | ---: | ---: | ---: | ---: |
| 2 strings | Baseline | — | 1.90 ms | 2.16 ms | 1.36 MiB |
| 2 strings | Normal plugin | 2.37 ms | 5.54 ms | 10.42 ms | 3.73 MiB |
| 2 strings | Alternate plugin | 1.71 ms | 3.92 ms | 3.97 ms | 3.73 MiB |
| 10,000 strings | Baseline | — | 2.05 ms | 2.34 ms | 2.00 MiB |
| 10,000 strings | Normal plugin | 73.55 ms | 78.12 ms | 78.71 ms | 10.80 MiB |
| 10,000 strings | Alternate plugin | 83.73 ms | 88.82 ms | 90.73 ms | 11.70 MiB |

Preparation timing begins inside `execute`: it includes manifest/context checks, source digest revalidation, interpreter construction, module compilation, instantiation, guest execution, and plan validation. It excludes initial package/source file reads and final report serialization. Whole-process timing includes launch, those reads, and full plan output. The baseline reads context and source bytes but does not parse the translation table, hash inputs, or build a plan. It measures a lightweight loader baseline, not equivalent native patch processing. Differences must not be described as pure Wasm execution overhead.

| Artifact / measure | Result |
| --- | ---: |
| Host executable | 1,117,632 bytes |
| Fixture-reading baseline executable | 319,392 bytes |
| Executable delta | 798,240 bytes (0.76 MiB) |
| Normal guest module | 71,801 bytes |
| Alternate guest module | 71,982 bytes |
| Normal 10,000-record guest linear memory | 4,784,128 bytes |
| Alternate 10,000-record guest linear memory | 4,980,736 bytes |
| Normal 10,000-record fuel consumed | 141,814,140 |
| Alternate 10,000-record fuel consumed | 176,671,734 |

Host SHA-256:

```text
81c18a8bbd69b6a6f794ef0484fac9b0471aed6ac6eb9e2c5c47626822ea19a6
```

Normal and alternate module SHA-256 values:

```text
a75f198cb0989c5ffc36ee94237386d93016c99a989bddec8ff872dfeb6bbf26
e3dd57c0cb5c4e622f57d3aaa811c92bcd365d90042d9327c0fc31a0f9ea538c
```

RSS includes the whole probe process and is not an isolated interpreter allocation count. The executable delta includes hashing, validation, and reporting as well as Wasmi, and is not a measurement of RFVP's eventual binary growth. The recorded memory value inside the report is final guest linear memory; Wasm memory cannot shrink, but it does not include compiler/host allocations. Neither small-fixture timing variation nor these samples establish a statistically rigorous comparison between guest variants.

**Limit adjustment and decision**

The original 100-million-fuel budget rejected the 10,000-record normal guest with fuel exhaustion. Raising the candidate budget to one billion allowed both variants to finish. This was a workload calibration, not disabling the bound: an infinite-loop fixture still exhausts fuel and returns an error. Unit tests use a lower explicit budget for that case so they remain fast.

The experiment meets the proposed local regression budgets: less than 2 MiB executable delta, less than 250 ms median preparation for 10,000 records, and less than 64 MiB peak process RSS. Proceed with Wasmi for desktop engine integration. Do not freeze the limits for release until the actual Sakura decoder, including zlib/GBK work and representative translation counts, has been measured.

**Verification**

Ten tests pass, covering:

- Accepted output retains Unicode and offsets after guest ownership ends.
- Unsupported manifest API/capability/script identity is rejected before compilation.
- Malformed or unknown JSON fields, duplicate/out-of-range offsets, and invalid mapping handles fail validation.
- Trap, nonzero status, fuel exhaustion, and memory growth failure after submission do not replace the caller's committed plan; the expected failure cause is asserted.
- Oversized and out-of-bounds guest buffers fail.
- Duplicate submission and undeclared capability fail.
- Missing submission, unknown imports, and a start-section module fail.
- Invalid source read and excessive initial memory fail.
- Claimed-entry validation preserves inner-name case and detects canonical folder-key conflicts.
- Invalid relative path forms fail.

Host and wasm-target guest Clippy checks pass with warnings denied. Formatting checks pass. The runner verifies both real Rust guest variants on both fixture sizes. These are local experiment checks; the engine workspace was not built or changed.

**Known limits of the evidence**

No Windows/Linux execution, browser embedding, real HCB VM execution, `patch.dat` decompression, GBK conversion, archive streaming, actual rendering/audio, full save round-trip, multi-plugin merge, or real-game smoke test was performed. No claim is made that every normative ABI rule is implemented by the probe: session quotas, deterministic metadata construction, race-resistant file opening, save fingerprints, and production host diagnostics remain integration work.

The source vectors are limited to 64 MiB total in the probe; real archive sources must be streamed by RFVP. Fuel cannot interrupt native filesystem I/O or compilation. A fixture can fit the output count limit while exceeding fuel or memory; those are independent limits. The executable and RSS measurements apply to this standalone release build on this machine.

**Reproduction and sources**

See the [experiment README](../../experiments/plugin-v1/README.md) for commands and outputs. The isolated workspace has its own lockfile and does not modify RFVP's root workspace or dependencies.

Wasmi documentation consulted: [2.0.0 API](https://docs.rs/wasmi/2.0.0/wasmi/), [Config](https://docs.rs/wasmi/2.0.0/wasmi/struct.Config.html), [Store](https://docs.rs/wasmi/2.0.0/wasmi/struct.Store.html), [StoreLimitsBuilder](https://docs.rs/wasmi/2.0.0/wasmi/struct.StoreLimitsBuilder.html), and [Memory](https://docs.rs/wasmi/2.0.0/wasmi/struct.Memory.html). Context7 did not return a matching Wasmi library, so the implementation was checked against these official docs and the downloaded 2.0.0 sources.
