# Real-data and platform plugin checks

These tools verify the plugin host on authorized local game data and on Linux. They never write to the game root and never copy game data into the repository. Reports contain hashes, counts, and timings. Per-run transcripts under `target/` may also include archive entry names from engine diagnostics. Neither contains script text or assets.

## Real-data equivalence and measurements

```sh
uv run --no-project python tools/plugin-real-data/run.py --game-root /path/to/sakura_moyu
```

Requirements: the reference worktrees created by `tools/plugin-reference/run.py` (`target/plugin-reference/worktrees/{main,sakura}`), the `wasm32-unknown-unknown` target, and a game root containing one `.hcb` and no `.bch`.

The runner builds three release probes from `probe.rs`, each in its own target directory:

- unchanged main `ffc7380`;
- the unchanged Sakura reference `8f6458f`;
- the working tree, using the production `prepare_game` service.

It then builds the actual Sakura guest after the working-tree host is fixed, packages it with the selected script's hash, and runs five observations:

| Label | Implementation |
| --- | --- |
| `main-disabled` | main, no patch |
| `branch-reference` | reference `StringPatchTable` + `Vfs::new_with_sakura_moyu_chs_patch` |
| `current-disabled` | `prepare_game` with no plugins |
| `current-bundled` | `--sakura-moyu-chs-patch` migration alias |
| `current-external` | external Wasm package, executed by Wasmi |

Each observation does the following:

- It linearly sweeps the selected script's code area from offset 4 to the system-description offset, and it fails if the sweep does not end exactly there.
- It executes `Context::push_string` at every string instruction, recording the payload address, the cursor, and a digest of the effective text.
- It streams and hashes every logical `folder/entry` path from every original archive, including the `patch/*` paths.

These comparisons must be exact:

- disabled equals main;
- the bundled and external Sakura modes both equal the reference branch;
- the replacement-hit flags are identical.

A separate timing pass runs only preparation under `/usr/bin/time`. It records the median preparation time, the peak RSS, and the runtime report: execution time, fuel, linear memory, and transferred bytes. The report is written to `target/plugin-real-data/run-*/report.json`.

`CARGO_TARGET_DIR` relocates all build and output files. This allows read-only source and game mounts. Probe dependency versions are pinned by each revision's lockfile. Set `CARGO_NET_OFFLINE=true` to forbid fetching.

## Linux container validation

```sh
tools/plugin-platform/linux-docker.sh --game-root /path/to/sakura_moyu
tools/plugin-platform/linux-docker.sh --platform linux/amd64 --game-root /path/to/sakura_moyu
```

The script mounts the checkout and the game read-only at their host paths, so git worktree metadata stays valid. Builds go to a named Docker volume. In a `rust:1-bookworm` image it runs:

- the GPU CLI build;
- the GPU library and software-desktop checks;
- the default build;
- the plugin crate tests;
- the preparation/save, GPU options, and software launch-option tests;
- synthetic conformance;
- the real-data runner, when a game root is supplied.

The summary and copies of the reports are written to `target/plugin-platform/linux-<arch>/`. `test_read_save_item` is skipped because it needs a local fixture that is absent on every platform. This is a build-and-headless check. It does not open a window or play audio.
