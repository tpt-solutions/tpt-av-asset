# Security Policy

## Supported versions

`tpt-av-asset` is pre-1.0. Only the latest tagged release and the `master`
branch receive security fixes; consumers should track releases closely until
the API stabilizes.

| Version | Supported |
| :--- | :--- |
| latest tag | yes |
| older tags | no |
| `master` | best effort |

## Reporting a vulnerability

**Do not open a public GitHub issue for security reports.**

Email `security@tptsolutions.co.nz` with:

1. A description of the issue and its impact.
2. Steps to reproduce (a crafted malicious media file is ideal — see the
   fuzz targets under `fuzz/` for the parsing surfaces of greatest interest).
3. The crate/binary version or commit you tested against.

You will get an acknowledgment within 5 business days. We will credit
reporters in the release notes by default; say so explicitly if you prefer
otherwise. Please keep the issue confidential until a fix is released.

## Scope

### In scope

- **Untrusted-media parsing.** The engine is designed to ingest files from
  arbitrary sources, so parsers are the primary attack surface:
  - `.tkvp` proxy-stream container (`tpt-av-asset-cache::container` and
    `video::ProxyStreamSource`) — header fields and frame table are
    attacker-controlled and allocation-relevant (see "Hardening notes").
  - `.peaks` waveform files (`tpt-av-asset-cache::reader`, `waveform`).
  - The hand-rolled binary row codec in `tpt-av-asset-db`
    (`schema.rs`/`asset_table.rs`/`job_table.rs`).
  - The thumbnail `meta` sidecar (`tpt-av-asset-cache::thumbnail`).
- **Panic safety.** Worker threads isolate job panics, but a reproducible
  panic reachable from parsing untrusted input is a bug we want.
- **Dependency-level issues** in the workspace or its pinned ecosystem git
  dependencies (both first-party `tpt-solutions` repos).

### Out of scope

- Vulnerabilities in the underlying codec stacks themselves
  (`tpt-kinetix`, `tpt-cadence`) — report those to the respective repos.
  We track them and will bump the pinned `rev`s on fix releases.
- Denial of service requiring local filesystem access to files the attacker
  already controls (e.g. filling `~/.tpt-av-asset`).
- The demo binaries in `examples/` — they are documentation, not hardened
  tooling. Use `tpt-av-asset-cli` for machine-facing batch work.

## Hardening notes

What the engine already does, and what to keep in mind when extending it:

- **Allocation bounding on untrusted lengths.** Every allocation sized from
  attacker-controlled data is bounded by the actual remaining file size
  before allocating (`ProxyStreamSource::open` bounds both the frame table
  and per-frame payload reads; truncated tails degrade to the valid prefix).
  Any new parser must follow this rule — see the regression tests in
  `tpt-av-asset-cache/tests/proxy_stream_hardening.rs` for the pattern.
- **Fixed-size records, positioned reads.** Waveform chunks and `.tkvp`
  frames are reached by offset arithmetic against a validated header, so a
  corrupt index can at worst miss, not overrun.
- **Row codecs reject, never guess.** The db codec returns
  `AssetError::Db("corrupt row…")` on truncation, unknown tags, or trailing
  bytes instead of inferring structure.
- **Pinned ecosystem sources.** The `tpt-kinetix`/`tpt-cadence` git
  dependencies are pinned to exact commits (`rev =`) in the root
  `Cargo.toml`, and `cargo deny check` (CI) enforces the allow-listed
  sources and permissive-only license tree.
- **Real-time safety is a correctness invariant**, not just performance: the
  allocation-counting test proves cache reads cannot become an allocation
  amplifier on the hot path.
- **Fuzzing.** `fuzz/` contains `cargo-fuzz` targets for the two
  highest-risk surfaces (`.tkvp` container, db row codec). Run them per
  [CONTRIBUTING.md](CONTRIBUTING.md#fuzzing) before touching either parser;
  CI runs a short non-blocking smoke fuzz on every push.
