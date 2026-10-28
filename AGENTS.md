# rust

```stele
kind: system
purpose: rust — lints.toml, the canonical maximalist opt-out [lints] table every jgeschwendt crate pastes, and `lints`, the binary that holds the grove to it (check, report, inventory).
commands:
  check: mise run check
  cover: mise run cover
  lint: cargo clippy --all-targets -- -D warnings && cargo fmt --check
  test: cargo test
```

## Map

| path            | what                                                                                                   |
| --------------- | ------------------------------------------------------------------------------------------------------ |
| `lints.toml`    | the canonical `[lints.rust]` + `[lints.clippy]` tables; every `allow` carries its reason above it      |
| `Cargo.toml`    | carries `lints.toml` verbatim (`tests/lints.rs` `dogfood` asserts it)                                  |
| `src/main.rs`   | the whole binary: crate walk, `check`, `report`, `inventory`                                           |
| `tests/lints.rs` | end-to-end suite over temp groves with sh stubs for `mise`, `clippy-driver`, `rustc`; never the real grove |

## Usage

Paste `lints.toml` into a crate's `Cargo.toml` as-is, or under `[workspace.lints.*]` with `[lints] workspace = true` in each member. Cargo inherits lints only from a crate's own workspace, so the table is copied, never referenced. A crate's own opt-outs go below it: an `allow` with a `#` reason on the line above.

The table: every stable rustc allow-by-default lint at `warn`, `unsafe_code` at `forbid`; clippy's `all`, `pedantic`, `nursery`, `cargo` groups at `warn`, priority -1; every `restriction` lint on its own line, since `blanket_clippy_restriction_lints` fires on the group. rustc's unstable lints are absent: naming one at any level trips `unknown_lints` on stable (probed on 1.94.0).

## Verbs

Run from this repo: `mise x -C <this repo> -- cargo run --release -q -- <verb>`.

- `lints check [<dir>…]` — each package's effective table (`workspace = true` reads the root's `[workspace.lints]`) against `lints.toml`. Drift: a canonical lint missing or weaker, an `allow` with no `#` comment on the line above, no table at all. A reasoned `allow` is an opt-out, listed. Exit 1 on drift.
- `lints report [<dir>…]` — `mise x -C <crate> -- cargo clippy --all-targets --no-deps --message-format=json -- <flags>`, the flags `lints.toml` as `-A/-W/-D/-F`, groups first; prints warnings per lint per crate, deduplicated across targets, then each crate's run outcome. `LINTS_TIMEOUT_SECS` (default 900) bounds each crate. Trailing args override the crate's own `[lints]` — a `deny` dropped by `-A`, an `allow` raised by `-W` (probed on 1.94.0, 2026-10-28) — but `#[allow]`s in source still win.
- `lints inventory` — `lints.toml` against `clippy-driver -W help` plus an empty-crate `rustc` compile that names the unstable lints; prints lints the toolchain has that the table lacks and names the toolchain lacks. Exit 1 on either. Part of `mise run check`, so a toolchain bump surfaces every lint diff.

With no dirs, `check` and `report` walk every `~/.grove/code/jgeschwendt/*/main`, skipping `target/`, `node_modules/`, `.archive/`, `deps/`, `_build/`; crate paths print relative to that root. The routine `~/.routines/lints-drift.md` runs `check` daily and opens the docket thread `lints-drift:rust` on drift.

The toolchain is pinned in `mise.toml`: `lints.toml` enumerates that toolchain's lints, and `inventory` fails on any other.
