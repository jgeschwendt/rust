# Invariants

| claim | node | anchor |
| --- | --- | --- |
| lints.toml is compiled into the binary, and Cargo.toml carries it verbatim | / | src/main.rs#CANON |
| tests/lints.rs dogfood asserts Cargo.toml carries lints.toml verbatim | / | tests/lints.rs#dogfood |
| the dirless walk skips .archive, .git, _build, deps, node_modules and target | / | src/main.rs#SKIP |
| tracked.toml is compiled in; a dirless walk acts only on the crates it lists | / | src/main.rs#TRACKED |
