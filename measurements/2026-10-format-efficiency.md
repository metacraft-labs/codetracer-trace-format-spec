# Format efficiency measurements, 2026-10

The measurements behind the 2026-10 revision of the CTFS trace format: container version 5,
`meta.dat` version 6, and `step-map.ns` version 2. Every number here comes from
[`tools/ctfs-measure`](../tools/ctfs-measure) run over the recording corpus described below.
`run_measurements.sh` reproduces the tables from a corpus, and `corpus/build_corpus.sh` rebuilds the
corpus.

## Decisions

| Item | Decision | The numbers that decided it |
|---|---|---|
| 1. `steps.dat` step encoding (`trace-events.md` §"Encoding Rules") | The first position record of every chunk is an `AbsoluteStep`. Every later one takes the shorter of `AbsoluteStep` and delta, and a tie goes to the `AbsoluteStep`. Calls, returns and thread switches force nothing. Readers refuse a delta before the chunk's first `AbsoluteStep`. | Stored size 2.6% below the 2026-09 rule. Within 1% of the smallest of nine rules on 16 of 25 corpora, more than any other rule. Decode speed is indistinguishable across the delta-coding rules (6.2-6.3 ns/record in WASM). No reader uses call/return anchors; every reader decodes a chunk on its own. |
| 2. `meta.dat`'s path list (`internal-files.md` §"Metadata") | Removed. `paths.dat` is the only list of source paths; `meta.dat` version 6 always carries `flags_ext`. | The list is 50% of all `meta.dat` bytes in the corpus (99% for Aztec, 89% for Solana), and 46,904 of 47,023 bytes on the WASM writer benchmark. |
| 3. `step-map.ns` storage (`internal-files.md` §"`step-map.ns`") | Version 2: keys delta-coded, step-id gaps run-length-coded, zstd chunks of about 64 KiB behind a small uncompressed chunk table. | 0.100 bytes per step against 8.07 for version 1 on maps of 1,000 steps or more, 81 times smaller (0.141 against 8.27 over all 288 maps, tiny ones included). The fastest-loading compressed layout measured in WASM: 4.7 ns/step against 7.0 for zstd'd gap varints. It costs 3.6 ns/step more than version 1 to load fully in WASM, counting the member's read from the page cache. |
| 4. Empty members (`ctfs-container.md` §2) | `MapBlock = 0`, no block. | 839 of 19,080 members in the corpus are empty and each held a mapping block. |
| 5. Members of at most one block (`ctfs-container.md` §2) | No mapping block; `MapBlock` carries the data block with bit 63 set. Container version 5. | Mapping blocks of such members are 27.4% of the corpus's bytes, and the median container shrinks by half. A tag rather than `Size <= BlockSize` because the size rule is unsound for a live reader and cannot be told apart from version 4's layout. |
| 6. `events.dat` kinds (`trace-events.md` §"EventLogKind") | The kind byte is the recorder's exact `EventLogKind` ordinal, 0-13, and readers report it unchanged. | Zero bytes per record (the byte already exists). At most +5% of `events.dat` after zstd under a deliberately adversarial re-expansion of the collapsed kinds; nothing for a recorder that uses one kind per kind of event. |

## Corpus

1,042 `.ct` recordings in 25 corpora, one per language and recorder (bash and zsh share a recorder, as do Erlang and Elixir). Each recorder recorded its own test programs and examples,
and for most languages we added larger programs written for this purpose (interpreters, parsers,
sorting, graph algorithms, numeric loops, simulations), so that loops, recursion and deep call
trees are all represented. `tools/ctfs-measure/corpus/` holds the recording scripts, the added
programs and `MANIFEST.tsv`, which lists every recording with its size, exec-record count and
SHA-256. The recordings themselves are not committed (they are 370 MB, and recordings are refused
by this repository's hooks).

Recorder revisions at recording time: codetracer-python-recorder `030a8e8f54`,
codetracer-ruby-recorder `3d950b606d`, codetracer-js-recorder `3adc3f0d62`,
codetracer-shell-recorders `8779f82776`, codetracer-beam-recorder `61b5c6fb43`,
codetracer-cardano-recorder `102c569d02`, aztec-avm-runtime `a89189116a`,
codetracer-flow-recorder `5d788db5f2`, codetracer-cairo-recorder `1d7be8417a`,
codetracer-circom-recorder `1d36abf2f1`, codetracer-engine-godot `352a996cdf`,
codetracer-leo-recorder `1eb7ab43fa`, codetracer-miden-recorder `34d4298741`,
codetracer-move-recorder `48dae6d9ee`, noir `fee00409bb` (`nargo trace` over every
`test_programs/execution_success` program), codetracer-php-recorder `6cd8696ed8`,
codetracer-polkavm-recorder `4db536bcf8`, codetracer-solana-recorder `e6e401e19a`,
codetracer-evm-recorder `7aeed58d4c`, codetracer-fuel-recorder `293646b439`,
codetracer-ton-recorder `c2009dd127`, codetracer-wasmi-recorder `35a54c1619`,
codetracer-wasm-recorder `78e7b49c61`.

Known gaps: the shell recordings are small (at most 23,510 steps), because the bash and zsh
recorders capture variables on every step and run at about 23 steps per second. The Elixir corpus
is only the recorder's own small fixtures. No MCR recording is included: those containers carry no
`steps.dat`.

## Method

- **An independent reader.** `ctfs-measure` reads containers with its own CTFS reader
  (`src/ctfs.rs`), not either implementation's, so a measurement cannot inherit a bug from the code
  it is judging, and it reads whatever writer revision a recorder shipped with. It decodes
  `steps.dat` to a writer-independent record list: absolute positions for the position records,
  verbatim bytes for the rest. `calls.dat` supplies the call and return points that the 2026-09 rule
  anchors on.
- **Re-encoding.** Every candidate rule re-encodes the same record list and is chunked as written:
  4096 records per chunk, one zstd level-3 frame each. Each encoding is decoded again and checked
  against the positions it was built from before its size is counted.
- **Step maps.** For each line-only recording, the map is the container's own `step-map.ns` where
  there is one, and is otherwise built from the decoded steps. Every candidate layout is decoded and
  compared list by list with the map it encodes.
- **Timing.** Timing runs on an AMD Ryzen 9 5950X, shared with other work, so each figure is the best
  of seven rounds and decode speeds are the best of three full passes per (trace, rule).
  - *Natively:* C zstd 1.5.7 through `zstd` 0.13 (what the native readers use) and ruzstd 0.8.3.
  - *In WASM:* the same Rust kernel built for `wasm32-wasip1` (rustc 1.89) and run under Node
    25.9's WASI, inflating with ruzstd, which is what `codetracer_trace_reader` uses on wasm32.
  - *Noise:* differences below about 3% are within run-to-run noise.

## Readers and the step-encoding anchors

Surveyed before choosing the step rule: which readers depend on `AbsoluteStep` placement.

- **The Rust `StepStreamReader`** (`codetracer_trace_reader/src/step_stream_reader.rs`) decodes each
  chunk independently, starting with no previous position, and resolves a delta that precedes an
  absolute against `0`. The db-backend's seekable and live-follow sources use it chunk by chunk.
- **Nim's `stepAbsoluteGlobalLineIndex`** (`new_trace_reader.nim`) replays only the containing chunk
  from position `0`. Its bulk variant carries the cursor across chunks.
- **Nobody else** uses call- or return-boundary anchors. `step-map.ns` addresses steps by
  exec-record index and is independent of the step encoding.
- **The Nim writer** promotes a chunk's first record to an `AbsoluteStep` only when that record is a
  step. A chunk that opens with a thread, raise, catch or reload record therefore keeps an
  unanchored delta, which both readers above resolve against `0`. All 24 Ruby recordings show it in
  their first chunk; it is harmless there only because the writer's cursor also started at `0`.

So the anchor every reader needs is the first position record of each chunk, and the revised rule
puts exactly one there.

## Results

Generated by `report.py` from the run that produced the decisions above. The step-map candidates
are named in `src/stepmap.rs`: `packed-rle` is version 2 as specified, and `packed` is the same
layout with gap varints instead of runs.

### Corpus by recorder

| corpus | traces | exec records | largest trace | column-aware traces | traces with an unanchored chunk | container bytes |
|---|---:|---:|---:|---:|---:|---:|
| aiken-cardano | 20 | 949 | 616 | 0 | 0 | 2,965,504 |
| aztec-avm | 10 | 2,012 | 517 | 10 | 0 | 5,611,520 |
| bash | 19 | 43,649 | 23,510 | 0 | 0 | 3,465,216 |
| beam-elixir | 16 | 247 | 64 | 0 | 0 | 2,396,160 |
| beam-erlang | 20 | 132,373 | 98,304 | 0 | 1 | 21,090,304 |
| cadence-flow | 17 | 3,233 | 3,088 | 17 | 0 | 2,379,776 |
| cairo | 39 | 616 | 170 | 39 | 0 | 5,447,680 |
| circom | 31 | 1,795 | 1,327 | 0 | 0 | 4,608,000 |
| gdscript | 19 | 26,005 | 25,437 | 0 | 0 | 3,416,064 |
| javascript | 23 | 1,771,024 | 736,152 | 23 | 0 | 12,623,872 |
| leo | 25 | 479 | 246 | 0 | 0 | 3,719,168 |
| miden-masm | 31 | 6,197 | 5,476 | 0 | 0 | 4,722,688 |
| move | 26 | 8,454 | 7,923 | 26 | 0 | 4,018,176 |
| noir | 513 | 899,761 | 331,283 | 513 | 0 | 97,173,504 |
| php | 9 | 2,765 | 2,686 | 0 | 0 | 1,404,928 |
| polkavm | 10 | 31,297 | 30,008 | 10 | 0 | 1,507,328 |
| python | 32 | 1,802,991 | 678,104 | 32 | 0 | 50,176,000 |
| ruby | 24 | 444,429 | 112,404 | 0 | 24 | 27,639,808 |
| solana | 15 | 22,789 | 15,670 | 15 | 0 | 2,179,072 |
| solidity-evm | 40 | 29,430 | 27,964 | 40 | 0 | 5,775,360 |
| sway-fuel | 22 | 45,817 | 40,096 | 0 | 0 | 3,518,464 |
| tolk-ton | 25 | 1,424 | 1,034 | 0 | 0 | 3,710,976 |
| wasm-wasmi | 10 | 10 | 1 | 0 | 0 | 1,445,888 |
| wasm-wazero | 29 | 286,816 | 247,179 | 29 | 0 | 4,583,424 |
| zsh | 17 | 31,743 | 14,474 | 0 | 0 | 3,010,560 |
| ALL | 1042 | 5,596,305 | 736,152 | 754 | 25 | 278,589,440 |

### `steps.dat`: bytes per exec record under each rule

zstd = the stream as stored: chunks of 4096 records, one level-3 frame each. Change is against the 2026-09 spec rule.

#### Whole corpus

| rule | raw B/rec | stored B/rec | stored vs spec | AbsoluteStep share of position records |
|---|---:|---:|---:|---:|
| abs-only | 3.036 | 0.1036 | +5.8% | 100.0% |
| spec-2026-09 | 2.195 | 0.0979 | +0.0% | 10.6% |
| nim-writer | 2.154 | 0.0956 | -2.3% | 14.9% |
| rust-writer | 2.195 | 0.0979 | -0.0% | 10.7% |
| anchor-only | 2.156 | 0.0966 | -1.4% | 0.0% |
| min-ties-delta | 2.148 | 0.0966 | -1.3% | 0.9% |
| min-ties-abs | 2.148 | 0.0953 | -2.6% | 26.9% |
| delta-1byte | 2.154 | 0.0956 | -2.3% | 14.9% |
| delta-1byte-ties-abs | 2.154 | 0.0957 | -2.3% | 27.5% |

#### Per corpus: stored bytes, change against the 2026-09 spec rule

| corpus | records | spec-2026-09 B/rec | abs-only | nim-writer | rust-writer | anchor-only | min-ties-delta | min-ties-abs | delta-1byte | delta-1byte-ties-abs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| aiken-cardano | 949 | 0.7503 | +4.9% | -0.4% | +0.0% | -0.3% | -0.4% | +4.9% | -0.4% | +4.9% |
| aztec-avm | 2,012 | 1.0427 | +2.0% | -9.4% | +0.0% | +6.7% | +5.7% | -10.3% | -9.4% | -9.3% |
| bash | 43,649 | 0.1228 | -4.5% | -1.6% | +0.0% | -1.4% | -1.5% | -4.6% | -1.6% | -4.6% |
| beam-elixir | 247 | 2.2834 | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% |
| beam-erlang | 132,373 | 0.1305 | -0.2% | -0.9% | -0.5% | -0.8% | -0.8% | -0.3% | -0.9% | -0.3% |
| cadence-flow | 3,233 | 0.3600 | -1.3% | +0.1% | +0.0% | -0.3% | -0.4% | -5.7% | +0.1% | -5.7% |
| cairo | 616 | 2.3328 | +16.4% | +0.6% | +0.0% | +0.9% | -0.2% | +0.8% | +0.6% | +0.8% |
| circom | 1,795 | 0.5894 | +10.5% | +0.2% | +0.0% | +0.4% | +0.2% | +10.5% | +0.2% | +10.5% |
| gdscript | 26,005 | 0.1888 | -6.6% | +2.8% | +0.0% | +3.2% | +2.7% | -7.0% | +2.8% | -7.0% |
| javascript | 1,771,024 | 0.1269 | +6.3% | -2.4% | -0.1% | -1.5% | -1.0% | -2.4% | -2.4% | -2.4% |
| leo | 479 | 1.4134 | +6.6% | -3.1% | +0.0% | -3.1% | -3.1% | +6.6% | -3.1% | +6.6% |
| miden-masm | 6,197 | 0.2746 | +8.3% | -1.5% | +0.0% | -0.2% | -1.1% | +4.8% | -1.5% | +4.8% |
| move | 8,454 | 0.2095 | +5.1% | -1.9% | +0.0% | -1.1% | -1.3% | -1.9% | -1.9% | -1.9% |
| noir | 899,761 | 0.0394 | +33.8% | +2.0% | +0.0% | -0.2% | -0.9% | -0.6% | +2.0% | +2.0% |
| php | 2,765 | 0.0984 | +0.0% | +3.3% | +0.0% | +5.5% | +3.3% | +0.0% | +3.3% | +0.0% |
| polkavm | 31,297 | 0.0324 | -51.1% | -0.8% | +0.0% | -0.8% | -0.8% | -51.1% | -0.8% | -51.1% |
| python | 1,802,991 | 0.0861 | +4.1% | -0.8% | +0.0% | +0.4% | +0.3% | -1.5% | -0.8% | -0.8% |
| ruby | 444,429 | 0.0932 | -10.9% | -13.9% | +0.0% | -13.4% | -13.9% | -11.3% | -13.9% | -11.3% |
| solana | 22,789 | 0.3123 | +6.8% | -2.5% | +0.0% | -1.2% | -2.4% | -4.5% | -2.5% | -2.5% |
| solidity-evm | 29,430 | 0.1759 | +10.8% | -5.4% | +0.0% | -7.2% | -7.2% | -5.3% | -5.4% | -5.3% |
| sway-fuel | 45,817 | 0.1247 | +6.4% | -4.3% | +0.0% | +24.3% | +22.6% | +0.9% | -4.3% | -3.6% |
| tolk-ton | 1,424 | 0.7409 | +5.8% | -0.2% | +0.0% | +0.4% | -0.2% | +5.4% | -0.2% | +5.4% |
| wasm-wasmi | 10 | 11.0000 | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% | +0.0% |
| wasm-wazero | 286,816 | 0.0999 | +9.5% | +0.1% | +0.0% | -0.3% | -0.3% | +0.1% | +0.1% | +0.1% |
| zsh | 31,743 | 0.1061 | -4.5% | +0.5% | +0.0% | +0.7% | +0.5% | -4.7% | +0.5% | -4.7% |

#### Corpora on which each rule is within 1% of the smallest stored size

| rule | corpora (of 25) |
|---|---:|
| abs-only | 8 |
| spec-2026-09 | 10 |
| nim-writer | 15 |
| rust-writer | 10 |
| anchor-only | 13 |
| min-ties-delta | 14 |
| min-ties-abs | 16 |
| delta-1byte | 15 |
| delta-1byte-ties-abs | 14 |

### `steps.dat` decode speed

ns per exec record, record-weighted over traces of at least 2000 records; best of the repeated passes per (trace, rule). `parse` decodes already-inflated chunks to absolute positions; the other columns include inflating every chunk.

#### native (3 passes, 71 traces)

| rule | parse | ruzstd + parse | C zstd + parse |
|---|---:|---:|---:|
| abs-only | 3.03 | 5.51 | 3.77 |
| spec-2026-09 | 2.13 | 4.38 | 2.80 |
| nim-writer | 2.16 | 4.47 | 2.78 |
| rust-writer | 2.22 | 4.45 | 2.82 |
| anchor-only | 1.93 | 4.32 | 2.66 |
| min-ties-delta | 2.01 | 4.27 | 2.61 |
| min-ties-abs | 2.08 | 4.42 | 2.80 |
| delta-1byte | 2.08 | 4.31 | 2.65 |
| delta-1byte-ties-abs | 2.11 | 4.36 | 2.76 |

#### wasm (3 passes, 71 traces)

| rule | parse | ruzstd + parse | C zstd + parse |
|---|---:|---:|---:|
| abs-only | 5.32 | 8.24 | n/a |
| spec-2026-09 | 3.29 | 6.31 | n/a |
| nim-writer | 3.39 | 6.33 | n/a |
| rust-writer | 3.30 | 6.32 | n/a |
| anchor-only | 3.09 | 6.17 | n/a |
| min-ties-delta | 3.08 | 6.16 | n/a |
| min-ties-abs | 3.25 | 6.28 | n/a |
| delta-1byte | 3.43 | 6.34 | n/a |
| delta-1byte-ties-abs | 3.28 | 6.24 | n/a |

### `step-map.ns`: stored bytes per step

Line-only traces only (a column-aware trace carries no step map). Candidates are described in `src/stepmap.rs`.

| corpus | maps | steps | v1 | v1-zstd | v1-chunked | gap | gap-zstd | gap-chunked | packed-nozstd | packed-rle-nozstd | packed-rle | packed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| aiken-cardano | 20 | 949 | 17.859 | 5.005 | 12.242 | 10.953 | 10.478 | 10.816 | 2.830 | 2.444 | 2.301 | 2.181 |
| bash | 19 | 43,649 | 8.312 | 1.979 | 1.822 | 1.322 | 0.467 | 0.476 | 1.057 | 1.538 | 0.226 | 0.194 |
| beam-elixir | 16 | 22 | 67.636 | 37.682 | 87.409 | 63.545 | 70.091 | 81.727 | 37.455 | 38.455 | 45.000 | 44.000 |
| beam-erlang | 20 | 121,023 | 8.115 | 1.564 | 1.491 | 1.116 | 0.127 | 0.130 | 1.018 | 0.046 | 0.027 | 0.025 |
| circom | 31 | 1,795 | 15.948 | 4.678 | 10.375 | 9.017 | 8.484 | 8.760 | 2.481 | 2.342 | 1.923 | 1.767 |
| gdscript | 19 | 26,002 | 8.686 | 1.609 | 1.958 | 1.698 | 0.836 | 0.847 | 1.106 | 1.128 | 0.238 | 0.214 |
| leo | 25 | 479 | 26.392 | 9.027 | 22.253 | 19.605 | 19.622 | 20.457 | 4.939 | 4.983 | 5.015 | 4.820 |
| miden-masm | 31 | 6,197 | 10.891 | 2.758 | 4.743 | 3.986 | 3.192 | 3.272 | 1.560 | 1.183 | 0.718 | 0.688 |
| php | 9 | 2,765 | 8.818 | 1.760 | 2.148 | 1.833 | 0.906 | 0.958 | 1.218 | 0.284 | 0.303 | 0.291 |
| ruby | 24 | 444,405 | 8.075 | 1.542 | 1.404 | 1.106 | 0.167 | 0.182 | 1.041 | 1.157 | 0.115 | 0.101 |
| sway-fuel | 22 | 45,817 | 8.779 | 1.825 | 2.153 | 1.791 | 0.883 | 0.898 | 1.103 | 1.483 | 0.172 | 0.163 |
| tolk-ton | 25 | 1,424 | 17.378 | 4.467 | 11.716 | 10.456 | 9.973 | 10.254 | 2.633 | 2.305 | 2.062 | 1.918 |
| wasm-wasmi | 10 | 10 | 78.000 | 47.000 | 107.000 | 75.000 | 84.000 | 100.000 | 50.000 | 51.000 | 60.000 | 59.000 |
| zsh | 17 | 31,743 | 8.409 | 1.944 | 2.052 | 1.420 | 0.541 | 0.549 | 1.071 | 1.259 | 0.200 | 0.177 |
| ALL | 288 | 726,280 | 8.269 | 1.645 | 1.645 | 1.292 | 0.362 | 0.377 | 1.064 | 1.027 | 0.141 | 0.126 |

#### Load and lookup, native: maps of at least 1000 steps

Full load = every list into a map, what the db-backend does at open. The read column adds reading the member's bytes from a file in the page cache first. One cold lookup = open plus one line, the hottest one, per map; summed over maps.

| layout | zstd decoder | bytes/step | full load ns/step | read + full load ns/step | one cold lookup, µs per map |
|---|---|---:|---:|---:|---:|
| v1 | - | 8.072 | 0.30 | 1.69 | 1.4 |
| packed | CZstd | 0.087 | 2.37 | 2.61 | 32.5 |
| packed | Ruzstd | 0.087 | 4.29 | 4.54 | 74.1 |
| packed-rle | CZstd | 0.100 | 2.54 | 2.73 | 31.7 |
| packed-rle | Ruzstd | 0.100 | 3.64 | 3.85 | 47.8 |
| packed-nozstd | - | 1.031 | 1.89 | 2.18 | 23.3 |
| packed-rle-nozstd | - | 0.988 | 2.21 | 2.41 | 25.1 |

Largest maps, native (µs):

| trace | steps | layout | zstd decoder | bytes | full load | read + full load | one cold lookup |
|---|---:|---|---|---:|---:|---:|---:|
| ruby/interpreter.ct | 112,403 | v1 | - | 901,406 | 23.0 | 66.5 | 2.1 |
| ruby/interpreter.ct | 112,403 | packed | Ruzstd | 5,746 | 201.5 | 206.9 | 64.9 |
| ruby/interpreter.ct | 112,403 | packed | CZstd | 5,746 | 152.5 | 162.3 | 39.2 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | 98,301 | v1 | - | 786,542 | 15.3 | 551.6 | 13.8 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | 98,301 | packed | Ruzstd | 81 | 1178.6 | 1178.9 | 1173.9 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | 98,301 | packed | CZstd | 81 | 648.7 | 655.6 | 639.6 |
| ruby/graph_algos.ct | 76,890 | v1 | - | 619,574 | 21.4 | 54.6 | 1.4 |
| ruby/graph_algos.ct | 76,890 | packed | Ruzstd | 9,402 | 189.7 | 197.6 | 94.5 |
| ruby/graph_algos.ct | 76,890 | packed | CZstd | 9,402 | 111.5 | 116.9 | 41.0 |
| ruby/numeric_loops.ct | 72,727 | v1 | - | 583,934 | 15.2 | 45.4 | 3.3 |
| ruby/numeric_loops.ct | 72,727 | packed | Ruzstd | 3,153 | 147.2 | 153.3 | 92.9 |
| ruby/numeric_loops.ct | 72,727 | packed | CZstd | 3,153 | 99.0 | 106.8 | 60.7 |
| ruby/recursion.ct | 52,510 | v1 | - | 421,494 | 10.9 | 34.2 | 3.9 |
| ruby/recursion.ct | 52,510 | packed | Ruzstd | 3,223 | 231.1 | 236.7 | 198.1 |
| ruby/recursion.ct | 52,510 | packed | CZstd | 3,223 | 106.2 | 72.8 | 34.2 |

#### Load and lookup, wasm: maps of at least 1000 steps

Full load = every list into a map, what the db-backend does at open. The read column adds reading the member's bytes from a file in the page cache first. One cold lookup = open plus one line, the hottest one, per map; summed over maps.

| layout | zstd decoder | bytes/step | full load ns/step | read + full load ns/step | one cold lookup, µs per map |
|---|---|---:|---:|---:|---:|
| v1 | - | 8.072 | 0.70 | 1.63 | 4.9 |
| packed | Ruzstd | 0.087 | 6.98 | 7.53 | 118.8 |
| packed-rle | Ruzstd | 0.100 | 4.71 | 5.24 | 53.3 |
| packed-nozstd | - | 1.031 | 1.84 | 2.40 | 19.0 |
| packed-rle-nozstd | - | 0.988 | 2.78 | 3.27 | 21.6 |

Largest maps, wasm (µs):

| trace | steps | layout | zstd decoder | bytes | full load | read + full load | one cold lookup |
|---|---:|---|---|---:|---:|---:|---:|
| ruby/interpreter.ct | 112,403 | v1 | - | 901,406 | 64.1 | 111.4 | 6.7 |
| ruby/interpreter.ct | 112,403 | packed | Ruzstd | 5,746 | 319.2 | 327.8 | 103.8 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | 98,301 | v1 | - | 786,542 | 51.3 | 144.5 | 76.7 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | 98,301 | packed | Ruzstd | 81 | 2100.0 | 2101.7 | 2097.4 |
| ruby/graph_algos.ct | 76,890 | v1 | - | 619,574 | 51.0 | 88.3 | 3.2 |
| ruby/graph_algos.ct | 76,890 | packed | Ruzstd | 9,402 | 289.9 | 301.9 | 146.1 |
| ruby/numeric_loops.ct | 72,727 | v1 | - | 583,934 | 42.5 | 77.3 | 10.7 |
| ruby/numeric_loops.ct | 72,727 | packed | Ruzstd | 3,153 | 225.4 | 237.4 | 152.2 |
| ruby/recursion.ct | 52,510 | v1 | - | 421,494 | 30.5 | 64.6 | 13.3 |
| ruby/recursion.ct | 52,510 | packed | Ruzstd | 3,223 | 361.6 | 371.7 | 315.4 |

#### `packed` chunk target

```
35 step maps with >= 1000 steps, 714681 steps
target	bytes/step	mean cold one-line lookup ns (per map, averaged)	load ns/step (averaged)
4 KiB	0.1239	16125	5.23
16 KiB	0.1108	22914	5.17
64 KiB	0.0995	37083	5.28
256 KiB	0.0963	37120	5.04
one frame	0.0963	36111	4.46
```

#### Every candidate on the largest maps (native)

| trace | candidate | inflater | bytes | open µs | full load µs | one cold lookup µs |
|---|---|---|---:|---:|---:|---:|
| ruby/interpreter.ct | v1 | CZstd | 901,406 | 0.4 | 22.8 | 2.2 |
| ruby/interpreter.ct | v1-zstd | CZstd | 162,868 | 879.1 | 1368.2 | 870.6 |
| ruby/interpreter.ct | v1-zstd | Ruzstd | 162,868 | 4723.8 | 2670.4 | 2647.1 |
| ruby/interpreter.ct | v1-chunked | CZstd | 167,655 | 0.6 | 961.0 | 96.0 |
| ruby/interpreter.ct | v1-chunked | Ruzstd | 167,655 | 0.6 | 2867.5 | 261.9 |
| ruby/interpreter.ct | gap | CZstd | 120,238 | 0.4 | 273.6 | 24.2 |
| ruby/interpreter.ct | gap-zstd | CZstd | 7,508 | 86.1 | 386.4 | 118.0 |
| ruby/interpreter.ct | gap-zstd | Ruzstd | 7,508 | 240.1 | 548.6 | 338.4 |
| ruby/interpreter.ct | gap-chunked | CZstd | 8,273 | 2.2 | 473.8 | 70.7 |
| ruby/interpreter.ct | gap-chunked | Ruzstd | 8,273 | 2.0 | 374.7 | 49.8 |
| ruby/interpreter.ct | packed | CZstd | 5,746 | 0.1 | 240.1 | 52.5 |
| ruby/interpreter.ct | packed | Ruzstd | 5,746 | 0.1 | 213.5 | 67.2 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | v1 | CZstd | 786,542 | 0.1 | 15.8 | 14.8 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | v1-zstd | CZstd | 142,923 | 820.0 | 859.1 | 849.2 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | v1-zstd | Ruzstd | 142,923 | 3455.5 | 2371.8 | 2382.9 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | v1-chunked | CZstd | 137,041 | 0.1 | 752.3 | 757.0 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | v1-chunked | Ruzstd | 137,041 | 0.1 | 2264.5 | 2292.7 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | gap | CZstd | 98,439 | 0.1 | 72.8 | 94.5 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | gap-zstd | CZstd | 162 | 4.9 | 79.4 | 103.0 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | gap-zstd | Ruzstd | 162 | 523.4 | 648.2 | 1181.0 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | gap-chunked | CZstd | 178 | 0.1 | 78.3 | 99.7 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | gap-chunked | Ruzstd | 178 | 0.1 | 596.1 | 622.3 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | packed | CZstd | 81 | 0.0 | 104.1 | 88.0 |
| beam-erlang/stress_calls_killed_at_timeout/erl.ct | packed | Ruzstd | 81 | 0.0 | 631.0 | 617.8 |
| ruby/graph_algos.ct | v1 | CZstd | 619,574 | 0.7 | 38.0 | 1.6 |
| ruby/graph_algos.ct | v1-zstd | CZstd | 117,820 | 879.2 | 584.8 | 556.6 |
| ruby/graph_algos.ct | v1-zstd | Ruzstd | 117,820 | 1747.4 | 1793.4 | 1763.0 |
| ruby/graph_algos.ct | v1-chunked | CZstd | 98,993 | 1.0 | 687.4 | 44.8 |
| ruby/graph_algos.ct | v1-chunked | Ruzstd | 98,993 | 1.0 | 1923.6 | 124.3 |
| ruby/graph_algos.ct | gap | CZstd | 82,396 | 0.8 | 73.6 | 5.0 |
| ruby/graph_algos.ct | gap-zstd | CZstd | 13,567 | 24.5 | 97.9 | 29.9 |
| ruby/graph_algos.ct | gap-zstd | Ruzstd | 13,567 | 94.7 | 170.2 | 98.2 |
| ruby/graph_algos.ct | gap-chunked | CZstd | 15,856 | 0.9 | 111.1 | 13.8 |
| ruby/graph_algos.ct | gap-chunked | Ruzstd | 15,856 | 0.9 | 205.1 | 30.3 |
| ruby/graph_algos.ct | packed | CZstd | 9,402 | 0.0 | 113.7 | 41.7 |
| ruby/graph_algos.ct | packed | Ruzstd | 9,402 | 0.0 | 190.9 | 95.7 |
| ruby/numeric_loops.ct | v1 | CZstd | 583,934 | 0.5 | 16.1 | 3.5 |
| ruby/numeric_loops.ct | v1-zstd | CZstd | 99,216 | 548.4 | 570.8 | 553.1 |
| ruby/numeric_loops.ct | v1-zstd | Ruzstd | 99,216 | 1683.4 | 1727.7 | 1721.2 |
| ruby/numeric_loops.ct | v1-chunked | CZstd | 81,920 | 0.6 | 611.7 | 176.6 |
| ruby/numeric_loops.ct | v1-chunked | Ruzstd | 81,920 | 0.6 | 1725.9 | 508.6 |
| ruby/numeric_loops.ct | gap | CZstd | 75,529 | 0.5 | 63.1 | 21.1 |
| ruby/numeric_loops.ct | gap-zstd | CZstd | 4,643 | 19.4 | 143.1 | 73.4 |
| ruby/numeric_loops.ct | gap-zstd | Ruzstd | 4,643 | 113.3 | 219.9 | 165.3 |
| ruby/numeric_loops.ct | gap-chunked | CZstd | 6,103 | 1.2 | 104.1 | 57.4 |
| ruby/numeric_loops.ct | gap-chunked | Ruzstd | 6,103 | 0.6 | 174.8 | 68.8 |
| ruby/numeric_loops.ct | packed | CZstd | 3,153 | 0.0 | 107.8 | 58.8 |
| ruby/numeric_loops.ct | packed | Ruzstd | 3,153 | 0.0 | 155.8 | 103.7 |
| ruby/recursion.ct | v1 | CZstd | 421,494 | 0.4 | 11.7 | 4.3 |
| ruby/recursion.ct | v1-zstd | CZstd | 89,685 | 415.9 | 434.8 | 424.0 |
| ruby/recursion.ct | v1-zstd | Ruzstd | 89,685 | 1325.7 | 1603.9 | 1326.8 |
| ruby/recursion.ct | v1-chunked | CZstd | 69,912 | 0.9 | 460.2 | 210.7 |
| ruby/recursion.ct | v1-chunked | Ruzstd | 69,912 | 0.5 | 1281.5 | 644.3 |
| ruby/recursion.ct | gap | CZstd | 54,077 | 0.4 | 44.0 | 23.8 |
| ruby/recursion.ct | gap-zstd | CZstd | 4,472 | 13.7 | 58.2 | 38.5 |
| ruby/recursion.ct | gap-zstd | Ruzstd | 4,472 | 190.2 | 225.0 | 204.4 |
| ruby/recursion.ct | gap-chunked | CZstd | 4,558 | 0.4 | 60.5 | 25.5 |
| ruby/recursion.ct | gap-chunked | Ruzstd | 4,558 | 0.4 | 229.4 | 159.1 |
| ruby/recursion.ct | packed | CZstd | 3,223 | 0.0 | 67.8 | 34.7 |
| ruby/recursion.ct | packed | Ruzstd | 3,223 | 0.0 | 235.5 | 200.2 |
| ruby/bench_heavy_work.ct | v1 | CZstd | 410,030 | 0.2 | 11.2 | 4.9 |
| ruby/bench_heavy_work.ct | v1-zstd | CZstd | 74,544 | 446.6 | 550.1 | 503.3 |
| ruby/bench_heavy_work.ct | v1-zstd | Ruzstd | 74,544 | 1325.8 | 1311.2 | 1306.6 |
| ruby/bench_heavy_work.ct | v1-chunked | CZstd | 60,753 | 0.5 | 424.3 | 231.4 |
| ruby/bench_heavy_work.ct | v1-chunked | Ruzstd | 60,753 | 0.3 | 1212.3 | 634.7 |
| ruby/bench_heavy_work.ct | gap | CZstd | 52,165 | 0.4 | 73.1 | 58.9 |
| ruby/bench_heavy_work.ct | gap-zstd | CZstd | 6,814 | 61.0 | 113.5 | 181.7 |
| ruby/bench_heavy_work.ct | gap-zstd | Ruzstd | 6,814 | 161.7 | 172.6 | 158.6 |
| ruby/bench_heavy_work.ct | gap-chunked | CZstd | 7,972 | 0.3 | 72.4 | 49.3 |
| ruby/bench_heavy_work.ct | gap-chunked | Ruzstd | 7,972 | 0.3 | 145.1 | 106.7 |
| ruby/bench_heavy_work.ct | packed | CZstd | 6,167 | 0.0 | 76.3 | 58.6 |
| ruby/bench_heavy_work.ct | packed | Ruzstd | 6,167 | 0.0 | 138.9 | 119.9 |

### `meta.dat`'s path list

| corpus | traces | meta.dat bytes | of which the path list | path list share of meta.dat |
|---|---:|---:|---:|---:|
| aiken-cardano | 20 | 5,026 | 2,088 | 42% |
| aztec-avm | 10 | 52,165 | 51,455 | 99% |
| bash | 19 | 5,305 | 2,295 | 43% |
| beam-elixir | 16 | 7,046 | 3,009 | 43% |
| beam-erlang | 20 | 9,037 | 4,100 | 45% |
| cadence-flow | 17 | 4,197 | 1,699 | 40% |
| cairo | 39 | 9,545 | 3,856 | 40% |
| circom | 31 | 7,951 | 3,247 | 41% |
| gdscript | 19 | 4,461 | 461 | 10% |
| javascript | 23 | 11,391 | 4,129 | 36% |
| leo | 25 | 6,001 | 2,454 | 41% |
| miden-masm | 31 | 7,713 | 3,128 | 41% |
| move | 26 | 8,315 | 6,676 | 80% |
| noir | 513 | 91,415 | 11,673 | 13% |
| php | 9 | 4,192 | 1,304 | 31% |
| polkavm | 10 | 3,332 | 1,431 | 43% |
| python | 32 | 22,865 | 14,422 | 63% |
| ruby | 24 | 7,567 | 2,979 | 39% |
| solana | 15 | 28,348 | 25,213 | 89% |
| solidity-evm | 40 | 12,061 | 6,460 | 54% |
| sway-fuel | 22 | 11,540 | 10,138 | 88% |
| tolk-ton | 25 | 6,148 | 2,532 | 41% |
| wasm-wasmi | 10 | 793 | 170 | 21% |
| wasm-wazero | 29 | 8,496 | 2,794 | 33% |
| zsh | 17 | 4,855 | 2,121 | 44% |
| ALL | 1042 | 339,765 | 169,834 | 50% |

### Small and empty members

Mapping blocks a container would not have under version 5's rules (one data block, no mapping block, for a member of at most one block; no block at all for an empty member).

| corpus | containers | members | empty | one block | container bytes | saved | saved share | median per container |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| aiken-cardano | 20 | 360 | 17 | 342 | 2,965,504 | 1,470,464 | 49.6% | 50.0% |
| aztec-avm | 10 | 180 | 12 | 147 | 5,611,520 | 651,264 | 11.6% | 22.3% |
| bash | 19 | 342 | 1 | 323 | 3,465,216 | 1,327,104 | 38.3% | 48.6% |
| beam-elixir | 16 | 288 | 10 | 276 | 2,396,160 | 1,171,456 | 48.9% | 49.3% |
| beam-erlang | 20 | 358 | 5 | 322 | 21,090,304 | 1,339,392 | 6.4% | 44.7% |
| cadence-flow | 17 | 289 | 16 | 272 | 2,379,776 | 1,179,648 | 49.6% | 50.0% |
| cairo | 39 | 663 | 35 | 628 | 5,447,680 | 2,715,648 | 49.8% | 50.0% |
| circom | 31 | 558 | 24 | 533 | 4,608,000 | 2,281,472 | 49.5% | 50.0% |
| gdscript | 19 | 380 | 17 | 355 | 3,416,064 | 1,523,712 | 44.6% | 50.0% |
| javascript | 23 | 397 | 3 | 360 | 12,623,872 | 1,486,848 | 11.8% | 48.6% |
| leo | 25 | 450 | 17 | 433 | 3,719,168 | 1,843,200 | 49.6% | 50.0% |
| miden-masm | 31 | 558 | 26 | 529 | 4,722,688 | 2,273,280 | 48.1% | 50.0% |
| move | 26 | 442 | 0 | 440 | 4,018,176 | 1,802,240 | 44.9% | 48.6% |
| noir | 513 | 9,747 | 487 | 9,086 | 97,173,504 | 39,211,008 | 40.4% | 50.0% |
| php | 9 | 162 | 1 | 158 | 1,404,928 | 651,264 | 46.4% | 48.6% |
| polkavm | 10 | 170 | 8 | 160 | 1,507,328 | 688,128 | 45.7% | 50.0% |
| python | 32 | 544 | 1 | 497 | 50,176,000 | 2,039,808 | 4.1% | 46.5% |
| ruby | 24 | 432 | 1 | 400 | 27,639,808 | 1,642,496 | 5.9% | 48.6% |
| solana | 15 | 255 | 15 | 233 | 2,179,072 | 1,015,808 | 46.6% | 50.0% |
| solidity-evm | 40 | 680 | 13 | 665 | 5,775,360 | 2,777,088 | 48.1% | 48.6% |
| sway-fuel | 22 | 396 | 62 | 325 | 3,518,464 | 1,585,152 | 45.1% | 52.2% |
| tolk-ton | 25 | 450 | 21 | 428 | 3,710,976 | 1,839,104 | 49.6% | 50.0% |
| wasm-wasmi | 10 | 180 | 17 | 163 | 1,445,888 | 737,280 | 51.0% | 50.0% |
| wasm-wazero | 29 | 493 | 29 | 457 | 4,583,424 | 1,990,656 | 43.4% | 50.0% |
| zsh | 17 | 306 | 1 | 287 | 3,010,560 | 1,179,648 | 39.2% | 48.6% |
| ALL | 1042 | 19,080 | 839 | 17,819 | 278,589,440 | 76,423,168 | 27.4% | 50.0% |

### `events.dat` kinds

| corpus | traces | records | stored bytes, kinds collapsed | kinds expanded at random within each class (upper bound) | cost | kind bytes seen (count) |
|---|---:|---:|---:|---:|---:|---|
| aiken-cardano | 20 | 7 | 216 | 219 | +3 B | 0:3 11:4 |
| aztec-avm | 10 | 26 | 3599 | 3617 | +18 B | 12:26 |
| bash | 19 | 124 | 1898 | 1949 | +51 B | 0:121 11:3 |
| beam-elixir | 16 | 765 | 38325 | 38429 | +104 B | 0:35 11:5 12:725 |
| beam-erlang | 20 | 962285 | 17109139 | 17984070 | +874931 B | 0:360 11:67 12:961858 |
| cadence-flow | 17 | 5 | 796 | 797 | +1 B | 11:3 12:2 |
| cairo | 39 | 12 | 730 | 733 | +3 B | 0:3 4:5 11:2 12:2 |
| circom | 31 | 7 | 413 | 413 | +0 B | 12:7 |
| gdscript | 19 | 6 | 169 | 169 | +0 B | 11:1 12:5 |
| javascript | 23 | 88 | 2579 | 2595 | +16 B | 0:88 |
| leo | 25 | 15 | 633 | 637 | +4 B | 0:2 4:10 11:3 |
| miden-masm | 31 | 16 | 561 | 564 | +3 B | 4:13 11:3 |
| move | 26 | 29 | 1613 | 1613 | +0 B | 11:1 12:28 |
| noir | 513 | 137 | 2696 | 2720 | +24 B | 0:137 |
| php | 9 | 55 | 927 | 960 | +33 B | 0:55 |
| polkavm | 10 | 4 | 184 | 185 | +1 B | 0:1 11:1 12:2 |
| python | 32 | 230 | 10796 | 10882 | +86 B | 0:227 4:3 |
| ruby | 24 | 148 | 2942 | 2971 | +29 B | 0:115 11:33 |
| solidity-evm | 40 | 47 | 3867 | 3876 | +9 B | 11:2 12:45 |
| sway-fuel | 22 | 13 | 452 | 454 | +2 B | 12:13 |
| tolk-ton | 25 | 9 | 259 | 259 | +0 B | 0:3 4:2 11:4 |
| wasm-wazero | 29 | 2 | 56 | 56 | +0 B | 11:2 |
| zsh | 17 | 68 | 1462 | 1489 | +27 B | 0:65 11:3 |
| ALL | 1017 | 964098 | 17184312 | 18059657 | +875345 B (+5.094%) | |


### Durability

What it costs a writer to keep its container readable while it records (`ctfs-container.md` §6,
"Durability"). `examples/durability.rs` replays each recording as a writer emits it: every chunk of
every chunked stream is a *seal*, interleaved by how far through its stream it lies, the other
members growing in proportion. The same version 5 container is then written to a file on btrfs
over NVMe in three ways: once at close (*buffered*); publishing at every seal the new bytes, changed
mapping slots and root entries (*per-seal*, uncoalesced `pwrite`s); and per-seal with an
`fdatasync` after each seal. Times are in ms, best of three.

| corpus | containers | MB | seals | writes | buffered ms | per-seal ms | per-seal cost | per-seal+fsync ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| aiken-cardano | 20 | 1.5 | 22 | 740 | 5.4 | 9.1 | +3.7 ms (1.68x) | 30 |
| aztec-avm | 10 | 5.0 | 16 | 1504 | 19.4 | 22.1 | +2.7 ms (1.14x) | 37 |
| bash | 19 | 2.1 | 197 | 4667 | 6.0 | 25.3 | +19.3 ms (4.22x) | 262 |
| beam-elixir | 16 | 1.2 | 21 | 683 | 3.1 | 7.3 | +4.2 ms (2.37x) | 28 |
| beam-erlang | 20 | 19.7 | 15572 | 111381 | 51.4 | 571.1 | +519.6 ms (11.10x) | 20819 |
| cadence-flow | 17 | 1.2 | 29 | 821 | 3.1 | 7.4 | +4.3 ms (2.39x) | 35 |
| cairo | 39 | 2.7 | 39 | 1256 | 8.1 | 16.1 | +8.0 ms (1.98x) | 51 |
| circom | 31 | 2.3 | 36 | 1200 | 6.2 | 13.7 | +7.6 ms (2.23x) | 47 |
| gdscript | 19 | 1.9 | 130 | 3479 | 4.0 | 15.2 | +11.2 ms (3.82x) | 147 |
| javascript | 23 | 11.1 | 7889 | 74780 | 46.2 | 394.7 | +348.5 ms (8.54x) | 10239 |
| leo | 25 | 1.9 | 25 | 866 | 4.9 | 12.0 | +7.2 ms (2.48x) | 35 |
| miden-masm | 31 | 2.4 | 52 | 1642 | 6.1 | 16.5 | +10.4 ms (2.72x) | 68 |
| move | 26 | 2.2 | 57 | 1658 | 6.1 | 16.7 | +10.7 ms (2.75x) | 74 |
| noir | 513 | 58.0 | 4151 | 72539 | 146.5 | 451.7 | +305.3 ms (3.08x) | 11858 |
| php | 9 | 0.8 | 19 | 635 | 2.6 | 5.3 | +2.7 ms (2.07x) | 102 |
| polkavm | 10 | 0.8 | 137 | 2077 | 2.1 | 8.6 | +6.5 ms (4.08x) | 598 |
| python | 32 | 48.1 | 7804 | 83053 | 91.7 | 976.9 | +885.2 ms (10.65x) | 43475 |
| ruby | 24 | 26.0 | 2065 | 39901 | 87.7 | 593.9 | +506.2 ms (6.77x) | 13022 |
| solana | 15 | 1.2 | 106 | 2625 | 2.7 | 17.5 | +14.8 ms (6.49x) | 509 |
| solidity-evm | 40 | 3.0 | 173 | 3567 | 6.7 | 19.8 | +13.1 ms (2.95x) | 730 |
| sway-fuel | 22 | 1.9 | 244 | 3929 | 4.6 | 17.8 | +13.2 ms (3.86x) | 1078 |
| tolk-ton | 25 | 1.9 | 29 | 965 | 3.8 | 8.7 | +4.9 ms (2.30x) | 164 |
| wasm-wasmi | 10 | 0.7 | 10 | 326 | 1.6 | 3.3 | +1.8 ms (2.12x) | 55 |
| wasm-wazero | 29 | 2.6 | 1222 | 13933 | 7.0 | 50.1 | +43.1 ms (7.14x) | 5646 |
| zsh | 17 | 1.8 | 148 | 3754 | 5.4 | 22.8 | +17.4 ms (4.20x) | 777 |
| ALL | 1042 | 202.1 | 40193 | 431981 | 532.4 | 3303.9 | +2771.5 ms (6.21x) | 109886 |

Publishing at every seal costs 2.8 s across 202 MB and 5.6 million exec records: about 0.5 µs per
exec record. A writer that coalesces each seal's writes would pay less. Syncing at every seal costs
110 s, 200 times the buffered write, which is why the spec requires publication (survives a
process crash) and not synchronisation (survives power loss).

### `values.dat` event tags in the corpus

`examples/value_tags.rs`, counts per corpus. Tags 0 (`StepValues`), 3 (`DropVariables`) and 9
(`Assignment`) occur; tags 1 and 4-8 do not -- not because no recorder emits them, but because every
corpus recording that would carry them was written through the Nim writer, which dropped them
(the Python recorder emits `BindVariable` and logged "bind_variable records are dropped" on every
run). The spec therefore keeps all ten tags, and both writers and readers must handle them
(`trace-events.md` §"Value Stream").

### Steps at line 0

No step map in the corpus has a step at line 0: 288 maps, 14 line-only recorders. The line-0 rule
(`internal-files.md` §"Global Line Index") changes no measured recording.

## Reproducing

```sh
# 1. Rebuild the recordings (needs the workspace's recorders built).
FMT_EFF=/some/work/dir WS=/path/to/workspace tools/ctfs-measure/corpus/build_corpus.sh
# 2. Measure and write report.md.
tools/ctfs-measure/run_measurements.sh /some/work/dir/corpus /some/work/dir/out
```

`run_measurements.sh` needs cargo, a Rust toolchain with the `wasm32-wasip1` target (default:
rustup's 1.89.0; override with `WASM_TOOLCHAIN`), Node 20 or later and Python 3.
