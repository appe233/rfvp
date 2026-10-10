# Change 1 reference comparison results

The synthetic comparison gate passed on macOS 15.8 / Apple Silicon using Rust 1.99.0 (`b940084d7`, host `aarch64-apple-darwin`). No game data was used and no engine production code was changed during this comparison.

## Reproduction and evidence

The [runner instructions](../../tools/plugin-reference/README.md) describe the shared probe, build profiles, fixtures, assertions, and cleanup. The successful command was:

```sh
uv run --no-project python tools/plugin-reference/run.py \
  --fixture-root target/plugin-reference/mount --require-case-sensitive
```

The fixture root was a temporary 64 MiB case-sensitive HFSX disk image, mounted with `hdiutil`. The runner independently confirmed that differently cased paths were distinct. The volume was detached after the run. The image and fixture data remain under ignored `target/plugin-reference/`.

Machine-readable evidence, build logs, and individual transcripts are retained at:

```text
target/plugin-reference/run-20261010-113938-70389/report.json
```

Reference revisions:

- Main: `ffc73809fd3ba30fccfd3cdf6deb35c93a9f7d3e`.
- Sakura: `8f6458f234eead7a702606694a0ef30224a3a14b`.
- Implementation: uncommitted Change 1 code on `feat/plugin-data`, based on main above. The aggregate source fingerprint was `cd1bd55a8d97fc551b979f3fb9ee3bf3e99bf266362451908d7418dfa7bb70a3`.

The report contains the individual implementation source hashes, full toolchain details, generated lockfile and executable hashes, fixture hashes, transcripts, and comparison results. The shared probe SHA-256 was `1ad2a9bbfe0650e6a41a80f5659833f281deb2cd3c80cb6acbdbd26ea6b6bd15`; the runner SHA-256 was `8feaf008624766a02ea60be656aa2814235272fc638fdce27015dc0c8bf961fe`. The runner verified that implementation sources did not change during execution and both detached reference worktrees remained clean.

## Results

**29 isolated process runs passed, with nine direct transcript comparisons and no unexplained differences.** Each valid run was also checked against explicit fixture expectations. Fixture byte hashes matched within each comparison pair.

| Comparison | Shift-JIS | GBK | UTF-8 |
| --- | --- | --- | --- |
| Main vs current, Sakura feature absent and preparation disabled | Identical | Identical | Identical |
| Main vs current, Sakura compiled in but preparation disabled | Identical | Identical | Identical |
| Sakura branch vs current static adapter enabled | Identical except documented loose-file precedence | Same | Same |

The comparison exercised:

- Original decoding, GBK translation independent of script NLS, longer translated text, empty replacement, and missing-offset fallback.
- Original payload address, cursor advancement, next bytecode marker, and rejection of an invalid original operand length before replacement.
- Original title, syscall name/argument metadata, and code-area endpoint.
- Unique archive-name mapping, ambiguous/unmatched names, original fallback, Unicode entry names, and entry-name case sensitivity.
- Logical loose files, normalized-folder loose files, distinct-case requested paths, and source-archive loose-file bypass.
- Stream lengths, partial reads, end-relative seeks, clamping, EOF, and independent streams.
- Mapped-source deletion errors without fallback, alongside unaffected original resource reads.

## Explicit differences

For each NLS, exactly one valid-input transcript field differed:

```text
/logical_loose/GRAPH/menu_bg/bytes
```

With `graph/menu_bg` present as a loose file and no `GRAPH/menu_bg` loose file, the Sakura branch returned `patched-menu` from the mapped archive. Current RFVP returned `logical-loose`, exactly matching main. This is the precedence correction specified by the RFC. When a separate `GRAPH/menu_bg` loose file was subsequently created, all applicable probes honored that explicitly requested path. These cases ran on the case-sensitive volume; they were not skipped.

Malformed-input checks ran separately from valid-input equivalence. Both Sakura implementations rejected missing `patch.dat`, missing `patch.bin`, corrupt zlib, incorrect compressed size, duplicate offsets, and truncated records: 12 successful rejection runs. Two further runs demonstrated intentional stricter validation: after stripping one trailing NUL, the old branch accepted a remaining NUL in the string, while the new API rejected it.

## Scope

This evidence completes the Change 1 synthetic reference-comparison checkbox. It does not establish real-game equivalence, renderer/audio behavior, Windows/Linux behavior, CLI activation, external plugin loading, or save/profile compatibility. Those remain separate implementation and release gates.
