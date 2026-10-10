# RFVP preparation plugin ABI 1.0 candidate

Status: normative candidate for [RFC 001](PLUGIN-SYSTEM-RFC.md). Not a published stable API. The [standalone prototype](../../experiments/plugin-v1/README.md) implements the execution boundary and single-plan validation. The engine implementation is in this repository: see [Change 3](CHANGE-3-RESULTS.md) for adapters and transactional composition, [Change 4](CHANGE-4-RESULTS.md) for save metadata and load preflight, and [Change 5](CHANGE-5-RESULTS.md) for real-data, Linux, and schema evidence. Draft JSON Schemas are in [`schemas/`](schemas/). Neither the prototype nor the current engine integration constitutes stable ABI publication.

**Package and manifest**

A package is a directory with `plugin.json` and a WebAssembly binary. The manifest is UTF-8 JSON, at most 64 KiB. Fields below are required; unknown/duplicate fields are errors. All JSON integers must be integral and fit the stated type. Hashes are 64 lowercase hexadecimal SHA-256 characters.

```json
{
  "schema_version": 1,
  "id": "example.translation",
  "version": "0.1.0",
  "api_major": 1,
  "api_minor": 0,
  "capabilities": ["script-string-table-v1", "resource-mapping-v1"],
  "module": "plugin.wasm",
  "supported_scripts": ["REPLACE_WITH_64_LOWERCASE_HEX_CHARACTERS"],
  "inputs": ["patch.dat", "patch.bin"],
  "claimed_archives": ["patch.bin"]
}
```

The placeholder hash above is intentionally invalid; the prototype runner generates valid synthetic manifests.

| Field | Contract |
| --- | --- |
| `schema_version` | `u32`, exactly 1. |
| `id` | 1–128 ASCII bytes from `[a-z0-9._-]`; unique in one session. |
| `version` | 1–64 UTF-8 bytes; opaque release identifier, compared exactly in save identity. |
| `api_major`, `api_minor` | `u32`; exactly 1 and 0 for this candidate. Future hosts may explicitly implement older versions; compatibility must not be inferred from a greater version number. |
| `capabilities` | Unique subset of the two listed capability strings; unknown requirements reject before execution. |
| `module` | Package-relative path to an at-most-8-MiB Wasm binary. |
| `supported_scripts` | Nonempty list containing the selected script's SHA-256; no wildcards. |
| `inputs` | Unique ordered game-relative file paths. Each file must exist. The host supplies exactly these inputs, not a filesystem API. |
| `claimed_archives` | Unique subset of `inputs` interpreted as standard FVP archives. Claimed archives are excluded from the original archive view. Claims may not overlap between plugins. |

Relative paths are 1–1024 UTF-8 bytes, use `/`, have no empty, `.` or `..` component, and contain no NUL, backslash, or colon. Resolve module paths within the package root and input paths within the resolved game root; reject symlink resolution outside the respective root. These are plugin-interface rules, not new rules for all legacy game paths. Production opening must avoid a check/open race if packages can change concurrently; the prototype canonicalizes paths but does not provide race-resistant filesystem opening.

**WebAssembly surface**

Use a validated wasm32 core module, compiled without WASI. Export one unshared 32-bit linear memory named `memory`. Multi-memory, memory64, and SIMD are not part of this candidate. A start section is rejected. At most one table is allowed, with at most 10,000 elements. The runtime may use internal memory/table machinery, but it must not grant other host imports.

Required exports:

```text
rfvp_api_version() -> i32    // returns 0x00010000
rfvp_prepare() -> i32        // returns 0 for success, any other value fails
memory                      // linear memory used for transfers
```

`rfvp_api_version` executes before preparation and must not call host imports. Its result must agree with the manifest. `rfvp_prepare` executes once. Guest panic, trap, allocation failure, or fuel exhaustion aborts the plugin. There is no resume, retry, or shutdown callback. A fresh store is used for each plugin and each game launch.

Imports, all in module `rfvp_v1`:

| Import | Signature | Behavior |
| --- | --- | --- |
| `context_len` | `() -> i32` | Length of the UTF-8 context JSON. |
| `context_read` | `(offset: i32, dst: i32, len: i32) -> i32` | Copy exactly `len` context bytes starting at `offset` into guest memory. Return `len`. |
| `source_read` | `(id: i32, offset: i64, dst: i32, len: i32) -> i32` | Copy exactly `len` bytes from the declared source handle. Return `len`. |
| `submit` | `(src: i32, len: i32) -> i32` | Copy one complete plan JSON from guest memory into tentative host storage. Return 0. |

Integer parameters represent unsigned bit patterns (`u32` or `u64`) despite Wasm's signed-looking type notation. All offset-plus-length calculations must be checked. Invalid ranges, handles, quotas, guest-memory accesses, or calls outside `rfvp_prepare` trap; no partial copy or negative-status recovery is specified. Zero-length transfers are permitted only at a valid boundary. Reads are exact: requesting past EOF fails, rather than returning a short read. Each read is at most 1 MiB. The guest obtains lengths from metadata and chunks larger reads.

`submit` may be called once, only during preparation. The host copies its bytes before returning. A successful submission does not commit anything: `rfvp_prepare` must still return zero, and decoding/validation/session composition must succeed. Submission is mandatory even for an empty plan. A second submission, malformed JSON, or a subsequent trap discards the tentative result.

Host import code must not panic, invoke the guest recursively, acquire the gameplay `GameData` lock, or enter the active overlay chain. No guest pointers or Wasmi-owned data survive session preparation.

**Game context schema**

```json
{
  "script_sha256": "...",
  "code_start": 4,
  "code_end": 1024,
  "sources": [
    {
      "id": 1,
      "path": "patch.dat",
      "len": 4096,
      "sha256": "...",
      "entries": []
    },
    {
      "id": 2,
      "path": "patch.bin",
      "len": 8192,
      "sha256": "...",
      "entries": ["menu_bg", "title_logo"]
    }
  ],
  "originals": [
    {"path": "graph/menu_bg", "entry": "menu_bg"},
    {"path": "graph_vis/title_logo", "entry": "title_logo"}
  ]
}
```

`code_start` and `code_end` are `u32` and describe the half-open HCB code range; production derives them from the actual selected script. Source IDs are nonzero `u32`, assigned 1..N in manifest input order and valid only for that plugin. Lengths are `u64`. Digests identify full input bytes. Non-archive inputs have an empty `entries` array. Claimed archives supply decoded entry names from RFVP's archive parser, preserving case and Unicode. Empty archives are valid.

`originals` enumerates the original standard FVP archives after the union of claims is removed. Each record has the logical `folder/entry` path and decoded entry name. Sort original records by UTF-8 path bytes and archive entry lists by UTF-8 entry-name bytes for deterministic iteration. Duplicate entry names in different archives remain separate records, enabling the plugin to detect ambiguous matches. Do not include loose files or other plugins' overrides in this view. Bound the serialized context before executing a guest.

The selected script's NLS is a host/session setting, not a guest override. Archive parsing uses the existing host NLS. The Sakura decoder explicitly interprets its own text table as GBK. A later capability can expose more metadata if a concrete plugin needs it; ABI 1.0 supplies the fields above.

**Plan schema**

```json
{
  "strings": [{"offset": 6, "text": "你好"}],
  "resources": [{"path": "graph/menu_bg", "source_id": 2, "entry": "menu_bg"}]
}
```

Both arrays are required. Reject unknown/duplicate fields, invalid UTF-8, fractional/out-of-range numbers, and invalid source references.

A string offset is `u32` within `[code_start, code_end)`. Text is at most 64 KiB of UTF-8, contains no NUL, and may be empty. Empty text is a replacement, not an absent mapping. Offsets may not repeat within one plan. The host does not assert that a range-valid offset is an instruction boundary. The engine continues enforcing its own text rendering limits; registration does not imply that a 64-KiB string is displayable by `TextPrint`.

Resource paths follow the relative-path grammar and must include a nonempty folder and entry. Normalize the folder only to ASCII lowercase for key comparison; preserve the remaining path. A `source_id` must belong to a claimed archive for this plugin, and `entry` must exactly match its metadata. Entry names are opaque archive identifiers, not filesystem paths. The mapped source opens raw archive bytes; never resolve loose files relative to the source archive. A logical mapping may introduce a new resource path as well as replace an existing one.

Require `script-string-table-v1` for a nonempty `strings` array and `resource-mapping-v1` for a nonempty `resources` array. Limit each array to 100,000 records. Reject duplicate canonical resource keys within one plan.

The host merges successful plans in explicit activation order. Later plugins win key conflicts with diagnostics. Original archive metadata remains unchanged during merging. Commit all plans together into engine-owned tables/sources after the final validation succeeds.

**Candidate host limits**

| Resource | Limit |
| --- | ---: |
| Manifest bytes | 64 KiB |
| Wasm module bytes | 8 MiB |
| Serialized context | 8 MiB |
| Submitted plan | 16 MiB |
| Guest linear memory | 64 MiB, one memory |
| Table entries | 10,000, at most one table |
| Fuel | 10,000,000,000 total across version query and preparation (revised from 1,000,000,000; see below) |
| One context/source read | 1 MiB |
| Cumulative context/source reads plus submission bytes | 64 MiB per plugin |
| Records | 100,000 per output array |
| One replacement string | 64 KiB UTF-8 |
| Activated plugins | 16 per game session |
| Sum of submitted plan bytes across plugins | 64 MiB per game session |

All units above are binary multiples. The production host additionally bounds archive metadata construction before serialization and JSON allocation counts, rather than first building an unlimited structure. Its candidate host policy is: at most 256 inputs/claims and 512 supported-script hashes per manifest; at most 1,024 archive metadata parses, 100,000 archive-entry records, and 100,000 filename-table records across one preparation; and 8 MiB aggregate metadata accounting. That accounting includes raw headers/tables, decoded retained names, and logical original paths, so it can reject before the independent serialized-context limit. Empty filename records count. These additional finite implementation limits need representative-data verification before publication.

Detailed key-conflict diagnostics are bounded to 1 MiB, followed by a suppressed-count summary. The full ordered merge still executes: diagnostic truncation does not change later-plugin-wins behavior. Merged tables may exceed one plugin's 100,000-record array limit, but remain bounded by the 16-plugin count and 64-MiB aggregate submitted-plan quota.

Plugin-enabled preflight rejects malformed/unopenable discovered original `.bin` archives instead of silently skipping them. Plugin-disabled legacy scanning is unchanged. Claims are excluded from the original view by opened-file identity, including case/symlink/hardlink aliases; two claims of the same opened archive are rejected even if their path strings differ. Source handles are retained throughout the session. Replacing a pathname does not replace the prepared source, while in-place file mutation remains unsupported.

Compilation uses Wasmi validation and `EnforcedLimits::strict()` for the pinned release. These are implementation limits, not a promise that every workload below each individual bound will fit the combined memory/fuel budget.

The prototype also limits total fixture source bytes to 64 MiB because it holds synthetic sources in memory. That is an experiment restriction, not an archive-size limit in the production ABI: production reads/hashes large sources through bounded streams, and only bytes crossing into the guest count toward the 64-MiB copy quota. A resource mapping may reference a large archive without reading its contents into Wasm.

**Candidate revision after authorized data (Change 5).** The actual Sakura guest needed 3,687,674,188 fuel on the verified script and patch, so the original 1,000,000,000 budget failed. Fuel use was identical on macOS arm64, Linux arm64, and Linux x86_64. About 2.3 billion units are spent in `encoding_rs` GBK decoding. Wasmi's per-block fuel accounting charges that code far more than its execution time: about 0.2 s. The budget is now 10,000,000,000, which leaves 2.7× headroom. This is not a time bound. On an Apple M2 Pro, a guest that simply spins until the budget runs out takes about 19 s; under the old budget it took about 1.9 s. The guest also used 34 MiB of its 64 MiB of linear memory and 6.8 MB of its 64 MiB copy quota. Its 0.85 MB context fit the 8 MiB limit, and its 4.7 MB plan fit the 16 MiB limit. No other limit changed. Wire fields and import signatures are unchanged. Re-measure before freezing, and whenever Wasmi is upgraded.

The guest may retain allocated memory until preparation ends. The host tracks copied output, source handles, and runtime-owned memory separately. RSS is not bounded by the linear-memory limit. Fuel meters guest execution; it does not meter filesystem reads, hashing, compilation, or JSON work. There is no real-time deadline guarantee.

**Session identity and save metadata**

Profile identity schema 1 records:

```text
script_sha256
nls: "sjis" | "gbk" | "utf8"
plugins: [
  { id, version, implementation_sha256,
    inputs: [{ path, sha256 }, ...] }, ...
]
```

Plugin order is activation order; input order is manifest order. An external implementation digest is the complete module's SHA-256. A static implementation uses a reproducible build-generated implementation digest. Claims, capabilities, supported-script lists, and API/schema versions must also influence the implementation identity used for saves: compute `implementation_sha256` as SHA-256 over the implementation content digest followed by the normalized manifest semantics described below. This prevents an unchanged module with a changed claim list from sharing an identity.

Canonical identity bytes use no JSON canonicalization. `str(s)` is `u32_le(UTF-8 byte length) || UTF-8 bytes`; `hash(h)` is the decoded 32 bytes; lists use `u32_le(count)` followed by items. Implementation identity is:

```text
ASCII("RFVP-IMPLEMENTATION-1\0")
hash(module_or_static_content_digest)
u32_le(schema_version) u32_le(api_major) u32_le(api_minor)
str(id) str(version)
list(str(capability))                  // sorted UTF-8 byte order
list(hash(supported_script))           // sorted hex order, deduplicated
list(str(input_path))                  // manifest order
list(str(claimed_archive_path))        // sorted UTF-8 byte order
```

The overall profile digest is SHA-256 over:

```text
ASCII("RFVP-PROFILE-1\0")
hash(script_sha256) str(nls)
u32_le(plugin_count)
for each plugin:
  str(id) str(version) hash(implementation_sha256)
  u32_le(input_count)
  for each input: str(path) hash(sha256)
```

Metadata JSON contains `schema_version: 1`, `profile_sha256`, and the fields of the profile above. Readers recompute the profile digest and reject inconsistencies; they compare the typed profile with the active session before applying any state. The digest is an identity checksum, not a signature or proof of trust.

Save placement, legacy handling, and error policy are specified in [RFC 001](PLUGIN-SYSTEM-RFC.md). Metadata is at most 64 KiB. Missing metadata is unknown compatibility; known mismatch is a load error. Old engines cannot enforce this policy. Changes to unrelated loose overrides/base archives are outside the profile fingerprint, matching RFVP's existing modifiable-resource model.

**Version publication**

Before declaring ABI 1.0 stable, publish the schema, example package source, plugin build instructions, limits, host support matrix, and conformance fixtures. Freeze field semantics and import signatures. A host upgrade must continue implementing a published version exactly or explicitly reject it. New required capabilities and behavior changes must be versioned, never inferred from package filenames or game titles.
