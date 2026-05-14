# Trace Filters — Cross-Language Recording Scope Control

> **Spec status:** Draft 2026-05-14. This document is the canonical
> wire-format-adjacent spec for trace filters. The design discussion
> lives in `codetracer-specs/Recording-Backends/Trace-Filters.md`;
> recorder CLI conventions live in `codetracer-specs/Recorder-CLI-Conventions.md`
> § 3a. This document is the source of truth for the schema and the
> hot-path contract.

## 1. Purpose

CodeTracer recorders capture execution events from a target language runtime
and serialize them into the CTFS trace format. Two operational realities make
"capture everything" the wrong default:

1. **Stdlib + framework noise dominates the trace.** A trivial program
   (`let x = 1 + 2; echo x` under the Nim VM tracer; `print("hi")` under
   Python; `puts "hi"` under Ruby) emits thousands of events that originate
   inside the host runtime's stdlib formatting / IO / type machinery and are
   irrelevant to the user. For test-suite snapshot comparison (`evalTrace:`),
   this stdlib noise makes goldens fragile and unreviewable. For interactive
   tracing, it inflates disk usage and slows the recorder's hot path.

2. **Production tracing has compliance constraints.** Variables named
   `password`, `api_key`, `token`, secrets returned by auth flows — these
   must not appear verbatim in a serialized trace. Recorders deployed in
   production environments need a built-in way to redact sensitive values
   while preserving the trace's structural shape.

This document specifies a **cross-language filter mechanism** that addresses
both concerns. The Python recorder already ships an implementation that
satisfies most of this design (see
`codetracer-python-recorder/design-docs/US0028 - Configurable Python trace filters.md`);
this spec lifts that design to a cross-language contract and adds the
performance guidance every recorder must follow.

## 2. Scope and Non-Goals

### In scope

- Scope-level filtering: deciding whether to record events from a given
  package / module / file / code object at all.
- Value-level filtering (optional second tier): deciding whether to redact /
  drop the value of an individual variable, argument, or return payload
  inside a recorded scope.
- Configuration format (TOML), filter chain composition, evaluation order,
  precedence rules.
- Cross-language guidance on the hot-path cost: how each recorder MUST
  integrate the filter classifier with its host runtime's per-code-object
  identity to avoid hash-table lookups during emission.
- Filter provenance recording in trace metadata so post-trace tooling can
  audit which filters were applied.

### Out of scope (explicit non-goals)

- The wire format itself does not encode filter rules. Filters are a
  recorder-side configuration concern; the trace records the _result_ of
  filtering (events that survived) plus _provenance_ (which filter files
  were active) but not the rules.
- Live policy reconfiguration. Filters are bound at recorder startup and
  remain fixed for the duration of the recording session.
- Cross-recorder selector portability. A `pkg:` selector means "Python
  package" in the Python recorder, "Nim module" in the Nim VM recorder, etc.
  Recorders MAY interpret `pkg:` differently based on the host language;
  the schema is universal but the semantics are bound to the recorder.

## 3. Two-Tier Architecture

### Tier 1 — Scope filtering (universal, MUST implement)

Every recorder MUST implement scope-level filtering. A scope is a
recorder-identifiable unit of code: a Python code object, a Nim file index,
a Ruby `iseq`, a V8 script, an EVM contract, etc.

Scope filtering decides: "for events originating from this scope, should we
emit them at all?"

### Tier 2 — Value filtering (optional, MAY implement)

Recorders that capture variable values MAY additionally implement
value-level filtering. This decides, _within an emitted event_, whether to
include each variable's serialized value verbatim, replace it with a
`<redacted>` marker (preserving the variable name and type), or drop the
variable entirely.

Recorders that operate at instruction granularity without per-variable
visibility (most blockchain VMs) MAY omit Tier 2 entirely.

## 4. Configuration Format (TOML)

### File structure

```toml
[meta]
name = "example-filter"
version = 1
description = "Skip stdlib, redact password-shaped variables"
labels = ["builtin", "example"]

[scope]
default_exec = "trace"          # one of: "trace" | "skip"
default_value_action = "allow"  # one of: "allow" | "redact" | "drop"

[[scope.rules]]
selector = "file:glob:**/lib/std/**"
exec = "skip"
reason = "Suppress stdlib noise"

[[scope.rules]]
selector = "pkg:literal:my_app.payments"
exec = "trace"
value_default = "redact"
reason = "Payments code is sensitive; trace structure but redact values"

[[scope.rules.value_patterns]]
selector = "local:regex:(?i)(password|token|api_key|secret)"
action = "redact"
reason = "Redact common secret names"

[[scope.rules.value_patterns]]
selector = "arg:literal:user_id"
action = "allow"
reason = "Explicitly allow user_id for audit trails"
```

### Selector grammar

```
<selector> ::= <kind> ":" [<match_type> ":"] <pattern>
```

#### Selector kinds

| Kind     | Tier | Domain                                                                             |
| -------- | ---- | ---------------------------------------------------------------------------------- |
| `pkg`    | 1    | Recorder-defined "package" (Python module dotted name; Nim module; Ruby gem; etc.) |
| `file`   | 1    | Absolute or repo-relative file path                                                |
| `obj`    | 1    | Fully-qualified code object (function / class / method)                            |
| `local`  | 2    | Local variable inside a recorded frame                                             |
| `global` | 2    | Module-global referenced by a recorded frame                                       |
| `arg`    | 2    | Function argument by name                                                          |
| `ret`    | 2    | Return value emitted by a scope (only meaningful with `obj` scope)                 |
| `attr`   | 2    | Attribute on a captured value (future-friendly for nested fields)                  |

Tier 1 selectors are valid in `[[scope.rules]]` entries. Tier 2 selectors
are valid in `[[scope.rules.value_patterns]]` entries. A recorder
encountering a tier-2 selector while it has not implemented tier 2 SHOULD
emit a warning and skip the rule rather than fail.

#### Match types

| Match type | Semantics                                                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `glob`     | Shell-style wildcards (`*`, `?`, `**` with path-segment semantics for file selectors). **Default** when `<match_type>` is omitted.                         |
| `regex`    | Language-native regular expression evaluated with full-match semantics. Recorders SHOULD document which regex flavour they use (PCRE, RE2, std-lib, etc.). |
| `literal`  | Exact, case-sensitive string comparison                                                                                                                    |

#### Pattern

Everything after the second colon is the pattern. Colons inside the pattern
are not escaped; parsing stops after two separators.

### Execution actions

| Action   | Tier | Effect                                                                                                                                                   |
| -------- | ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `trace`  | 1    | Record events from this scope                                                                                                                            |
| `skip`   | 1    | Do not record any events from this scope                                                                                                                 |
| `allow`  | 2    | Include the variable's value verbatim in serialized events                                                                                               |
| `redact` | 2    | Emit the variable's name and type but replace the value with `<redacted>`                                                                                |
| `drop`   | 2    | Omit the variable entirely (for `ret` values, still emit the structural return event with a `<dropped>` placeholder so call/return pairing is preserved) |

### Evaluation order

1. Initialize execution policy to `scope.default_exec`.
2. Walk `[[scope.rules]]` top to bottom. Each rule whose selector matches
   the current scope updates the execution policy. Later matching rules
   override earlier decisions (no rewind, no backtracking).
3. If the final execution policy is `skip`, suppress all events from this
   scope. Stop.
4. For Tier-2 recorders, when serializing a value inside a recorded scope:
   start from `scope.default_value_action`, overridden by the matched
   scope-rule's `value_default` if present. Then walk the scope-rule's
   `[[value_patterns]]` top to bottom; the first pattern whose selector
   matches the variable wins and short-circuits further evaluation.

## 5. Filter Chain Composition

### CLI

```
<recorder> [other flags...] --trace-filter:<path1> --trace-filter:<path2> [...]
```

`--trace-filter:` is repeatable. Filters are loaded in left-to-right order.

### Environment variable

```
CODETRACER_TRACE_FILTER=<path1>::<path2>[::...]
```

Paths separated by `::` (double colon — chosen to avoid collision with
Windows drive letters and POSIX `$PATH`-style `:`). Whitespace around
separators is not stripped.

### Auto-discovery

Recorders SHOULD walk upward from the script / project directory looking
for `.codetracer/trace-filter.toml`. If found, it loads after the builtin
default and before any explicit `--trace-filter:` arguments. Recorders MAY
disable auto-discovery via a `--no-auto-filter` flag for reproducible
runs.

### Composition order (top to bottom; later overrides earlier)

1. Builtin default (recorder-shipped — typically skips host stdlib,
   redacts common secret names).
2. Project filter from auto-discovery, if found.
3. Environment-variable filters, in `::`-separated order.
4. Command-line `--trace-filter:` arguments, in left-to-right order.

Within each loaded filter, rules are evaluated in source order.

## 6. Hot-Path Performance Requirement

This is the critical guidance every recorder implementation MUST follow.

### What MUST happen (the only acceptable design)

Filter resolution runs **once per unique scope** (the "classifier"). The
host runtime already assigns every scope an identity — a code-object
pointer, a file index, a script id, a contract address. The classifier
runs at scope registration:

```
classify(scope_path) -> Tracked(PathId) | Skipped
```

If the scope is `Tracked`, the recorder assigns it a real `PathId` and
proceeds normally. If `Skipped`, the recorder assigns an invalid sentinel
(`InvalidPathId`, or whatever the local convention is) and the per-event
emission path early-returns when it sees it.

The hot path (per event) is therefore:

```
if scope.cached_path_id == InvalidPathId: return     # one read, no hash
```

**Every recorder MUST stash the decision in its host runtime's per-scope
metadata slot.** Hash-table lookups on the hot path are a defect and
SHOULD be reported as such in code review.

Per-language slot examples (non-exhaustive):

| Recorder              | Native slot                                                                  |
| --------------------- | ---------------------------------------------------------------------------- |
| Python                | `_PyEval_RequestCodeExtraIndex` + `co_extra` (per code object pointer slot)  |
| Nim VM                | Parallel `seq[PathId]` indexed by the existing `FileIndex.int32` (dense int) |
| Ruby                  | `rb_iseq_t` extra-data slot, or a hash on `iseq_imemo`                       |
| V8 / JS               | `script.id` is already a dense int — direct array index                      |
| EVM / PolkaVM / Wasmi | Per-contract / per-module record gets a `path_id` field at load time         |

Where no native slot exists, the recorder SHOULD allocate one as part of
its initialization rather than fall back to per-event hashing.

### What MUST NOT happen

- Per-event hashing of paths or scope identifiers.
- Re-evaluating selectors per event (the classifier output is bound to the
  scope identity at registration; selectors are not consulted again).
- Allocating during emission for filter-related bookkeeping.

### Classifier-time algorithm choice

The classifier itself runs once per unique scope, so the algorithm choice
is bounded by path cardinality, not event cardinality. For 10 unique
paths, linear scan over compiled selectors is fine. For 10000 unique
paths matched against 100+ rules, more sophisticated structures (prefix
trie, compiled regex alternation, Aho-Corasick) start to pay off.

Algorithms and rulesets MUST be benchmarked. The
`codetracer-trace-format-benchmarks` repository is the home for these
measurements; recorders that implement a classifier MUST include
benchmark cases for their representative rulesets.

## 7. Provenance in Trace Metadata

The trace's metadata block MUST record which filter files were active for
the recording session, in their composition order, so post-trace tooling
can audit what was filtered.

Recommended metadata shape (under the existing metadata stream):

```json
{
  "trace_filter": {
    "filters": [
      { "path": "<inline:builtin-default>", "sha256": "..." },
      {
        "path": "/path/to/project/.codetracer/trace-filter.toml",
        "sha256": "..."
      },
      { "path": "/path/to/user/override.toml", "sha256": "..." }
    ]
  }
}
```

The `path` field MAY use sentinel values like `<inline:builtin-default>`
for filters that aren't loaded from a real file path. The `sha256` is the
content hash of the filter file; computed once at load time. This closes
the regression flagged in
`codetracer-python-recorder/AUDIT-CTFS-2026-05.md` § "Known coverage
regression — trace-filter chain assertions (follow-up)".

The on-disk encoding inside `meta.dat` is documented in
[internal-files.md](internal-files.md) § "Metadata (meta.dat)" — see the
`trace_filter.filters[]` extended-field block. Consumers that read the
metadata block via the standard reader MUST surface this array
unmodified.

## 8. Default Filter Policy

Each recorder SHOULD ship a built-in default filter that loads first in
the composition chain. The default SHOULD:

- Skip the host language's stdlib (or a curated noisy subset of it).
- For Tier-2 recorders, redact a built-in regex list of common secret
  variable names: `(?i)(password|passwd|token|api[_-]?key|secret|auth)`
  across `local`, `global`, `arg`, `ret`, `attr` value selectors.
- Be loadable via the same composition mechanism (treated as the
  ordered-first entry).

Users can override the default by passing their own filter; their rules
appear later in the composition order and so take precedence.

## 9. Validation and Error Handling

- Invalid TOML SHOULD fail recorder startup with a clear diagnostic
  (file path + line if available).
- Unknown selector kinds, match types, or actions SHOULD fail startup
  (preserving forward compatibility via spec versioning is the answer to
  "I want to use a future selector kind").
- A Tier-2 selector in a recorder that hasn't implemented Tier 2 SHOULD
  emit a warning and skip the rule, NOT fail (so the same filter file is
  cross-recorder portable).
- Circular includes — N/A; the schema has no include mechanism, only
  composition via the CLI/env chain.

## 10. Library API Surface

The cross-language CTFS library code (in
`codetracer-trace-format-nim` and the corresponding `codetracer_trace_*`
Rust crates if/when they re-enter the design space) SHOULD expose:

```nim
# Compile a list of TOML filter file paths (in composition order) into a
# Classifier. Returns Err for invalid files.
proc compileFilters*(paths: seq[string]): Result[Classifier, string]

# Compile from inline TOML content. Useful for builtin defaults embedded
# as string constants.
proc compileFiltersInline*(toml: string; sourceName: string): Result[Classifier, string]

# Classify a single scope's path. Pure; no IO.
proc classify*(c: Classifier; path: string): Decision

# The Decision discriminator. For Tier-2 recorders this carries the
# matched scope rule's value-default and value patterns; for Tier-1-only
# recorders, only the exec field is consulted.
type Decision = object
  exec*: ExecAction          # tEmit, tSkip
  valueDefault*: ValueAction # tAllow, tRedact, tDrop
  valuePatterns*: seq[CompiledValuePattern]
  matchedRuleSource*: string # for debugging

# Tier-2 only: classify a variable value given its name, kind, and the
# Decision from the enclosing scope. The recorder calls this per-variable
# during event serialization.
proc classifyValue*(d: Decision; kind: ValueKind; name: string): ValueAction
```

Nim signatures are the canonical reference because
`codetracer-trace-format-nim` is the first implementation. Language ports
follow the same shape with idiomatic types — Rust crates expose
`compile_filters(paths: &[Path]) -> Result<Classifier, Error>`,
`Classifier::classify(&self, path: &str) -> Decision`, etc., preserving
the same semantics.

The library does NOT dictate caching strategy — that's per-recorder
integration code, per § 6.

The library DOES provide the TOML parser, selector compilation, glob /
regex matcher selection, and the classifier algorithms (with benchmark
coverage from `codetracer-trace-format-benchmarks`).

## 11. Versioning

The TOML schema carries a `[meta] version = N` field. Initial spec
version is `1`. Breaking changes to selector grammar, action semantics, or
evaluation order increment the version. Recorders SHOULD refuse to load a
filter file whose `version` is higher than the version they support.

## 12. References

- `codetracer-specs/Recording-Backends/Trace-Filters.md` — design
  discussion + cross-recorder impact analysis. The current document is
  the canonical-spec mirror of sections 1–11 of that design doc.
- `codetracer-specs/Recording-Backends/Trace-Filters.milestones.md` —
  implementation milestone sequence across the trace-format repos, the
  Nim library, the Nim VM tracer, and the Python recorder.
- `codetracer-specs/Recorder-CLI-Conventions.md` § 3a — recorder CLI
  convention for `--trace-filter:`, the `CODETRACER_TRACE_FILTER` env
  var, and `--no-auto-filter`.
- [internal-files.md](internal-files.md) § "Metadata (meta.dat)" —
  on-disk encoding of `trace_filter.filters[]` provenance fields.
- `codetracer-python-recorder/design-docs/US0028 - Configurable Python trace filters.md`
  — proto-spec; the design doc above is a refinement and cross-language
  lift of this document.
- `codetracer-python-recorder/AUDIT-CTFS-2026-05.md` § "Known coverage
  regression — trace-filter chain assertions (follow-up)" — motivates
  the trace-metadata provenance requirement in § 7.
