# rust

```stele
kind: system
purpose: rust — lints.toml, the canonical maximalist opt-out [lints] table every jgeschwendt crate pastes, and `lints`, the binary that holds the grove to it (check, sync, report, inventory).
commands:
  check: mise run check
  cover: mise run cover
  lint: cargo clippy --all-targets -- -D warnings && cargo fmt --check
  test: cargo test
invariants:
  - claim: lints.toml is compiled into the binary, and Cargo.toml carries it verbatim
    anchor: src/main.rs#CANON
  - claim: the dirless walk skips .archive, .git, _build, deps, node_modules and target
    anchor: src/main.rs#SKIP
  - claim: tracked.toml is compiled in; a dirless walk acts only on the crates it lists
    anchor: src/main.rs#TRACKED
  - claim: tests/lints.rs dogfood asserts Cargo.toml carries lints.toml verbatim
    anchor: tests/lints.rs#dogfood
```

<!-- @stele -->

## Hazards (0 active)


## Map

| node | kind | purpose | unfold |
| ---- | ---- | ------- | ------ |

## Indexes

All invariants: `.stele/index/invariants.md` · all hazards: `.stele/index/hazards.md`

## Engine

`stele` CLI available → `stele root | unfold <id> | invariants --touching <path> | hazards | nodes --kind <k>`. MCP: `stele serve`.
No engine → everything above is complete; nested AGENTS.md files carry the detail (nearest file wins).
<!-- @end -->

## Map

| path            | what                                                                                                   |
| --------------- | ------------------------------------------------------------------------------------------------------ |
| `lints.toml`    | the canonical `[lints.rust]` + `[lints.clippy]` tables; every `allow` carries its reason above it      |
| `Cargo.toml`    | carries `lints.toml` verbatim (`tests/lints.rs` `dogfood` asserts it)                                  |
| `src/main.rs`   | the whole binary: crate walk, `check`, `sync`, `report`, `inventory`                                   |
| `tests/lints.rs` | end-to-end suite over temp groves with sh stubs for `mise`, `clippy-driver`, `rustc`; never the real grove |
| `tracked.toml`  | `crates`, the opted-in crate paths relative to the grove root, alpha-sorted; compiled in like `lints.toml` |

## Usage

Paste `lints.toml` into a crate's `Cargo.toml` as-is, or under `[workspace.lints.*]` with `[lints] workspace = true` in each member. Cargo inherits lints only from a crate's own workspace, so the table is copied, never referenced. A crate's own opt-outs are edits to its copy: an `allow`, or a weaker level, with a `#` reason on the line above.

The table: every stable rustc allow-by-default lint at `warn`, `unsafe_code` at `forbid`; clippy's `all`, `pedantic`, `nursery`, `cargo` groups at `warn`, priority -1; every `restriction` lint on its own line, since `blanket_clippy_restriction_lints` fires on the group. rustc's unstable lints are absent: naming one at any level trips `unknown_lints` on stable (probed on 1.94.0).

## Verbs

Run from this repo: `mise x -C <this repo> -- cargo run --release -q -- <verb>`.

- `lints check [<dir>…]` — each package's effective table (`workspace = true` reads the root's `[workspace.lints]`) against `lints.toml`. Drift: a canonical lint missing, a lint weaker than canon or an extra `allow` with no `#` comment on the line above, no table at all. A reasoned weaker level (`unsafe_code` at `deny` for one `#[expect]` file) or a reasoned `allow` is an opt-out, listed. Exit 1 on drift.
- `lints sync [<dir>…]` — the fix for drift and the way a canonical change spreads: splices `lints.toml`'s tables over each package's `[lints.rust]`/`[lints.clippy]` (`workspace = true` writes the root's `[workspace.lints.*]`; a member with no `[lints]` in a workspace adopts the root's), appending any table that is absent; every byte outside those tables survives. `check`'s opt-outs keep their lines, reason included, in the canonical slot or alphabetically for an extra, and a level stricter than canon stays as written, so a canon loosening reaches a crate only after its own line is deleted; its "without a reason" lints are dropped and listed for re-adding with one. Idempotent; exit 2 on a manifest it cannot write.
- `lints report [<dir>…]` — `mise x -C <crate> -- cargo clippy --all-targets --no-deps --message-format=json -- <flags>`, the flags `lints.toml` as `-A/-W/-D/-F`, groups first; prints warnings per lint per crate, deduplicated across targets, then each crate's run outcome. `LINTS_TIMEOUT_SECS` (default 900) bounds each crate. Trailing args override the crate's own `[lints]` — a `deny` dropped by `-A`, an `allow` raised by `-W` (probed on 1.94.0, 2026-10-28) — but `#[allow]`s in source still win.
- `lints inventory` — `lints.toml` against `clippy-driver -W help` plus an empty-crate `rustc` compile that names the unstable lints; prints lints the toolchain has that the table lacks and names the toolchain lacks. Exit 1 on either. Part of `mise run check`, so a toolchain bump surfaces every lint diff.

With no dirs, `check`, `sync` and `report` walk every `~/.grove/code/jgeschwendt/*/main`, skipping `target/`, `node_modules/`, `.archive/`, `deps/`, `_build/`, and act only on the crates `tracked.toml` lists; crate paths print relative to that root. Tracking is opt-in: `check` ends with `untracked: <n>`, the walked crates left out, which never sets the exit code; a listed crate the walk does not find exits 2 before any work. Explicit dirs bypass the list, so a crate is checked or synced before it is opted in. The routine `~/.routines/lints-drift.md` runs `check` daily and exits 1 on drift.

`clippy::single_call_fn` stays on, and is met by shape, not by attribute: a step used once, always run and short is a `let name = { … };` block in place; a step that is lazy, a callback, a subcommand's entry or called by a test stays a function under `#[expect(clippy::single_call_fn, reason = "…")]`. Never a helper invented to save lines. (since 2026-11-23 · owner on the canon form: "high standards everywhere"; "do not collapse to the point of hasty abstractions, use language for language")

The toolchain is pinned in `mise.toml`: `lints.toml` enumerates that toolchain's lints, and `inventory` fails on any other.
