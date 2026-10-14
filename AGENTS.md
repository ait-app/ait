# Repository guidance

Before changing domain boundaries, read `docs/README.md`.

- Rust is the fixed implementation language; keep the root Cargo workspace buildable.
- Before any Rust change, read and follow [the Rust style guide](docs/policy/rust.md). All Rust changes, including tests and refactors, MUST comply with it.
- Dependencies point inward: adapters implement ports; application coordinates domain behavior.
- Never commit credentials, provider tokens, local SQLite databases, or runtime artifacts.
- Run format and lint checks appropriate to the changed files before handing off changes.
- Run only tests for the changed code and directly related behavior, including when preparing commits or updating PRs. Run full Rust workspace tests only when explicitly requested by the user; workspace-wide coverage counts as a full test run and requires the same explicit request. A request to commit changes or update a PR does not imply a request for full tests or coverage.
- If the current task changes no Rust code in `bins/` or `crates/`, skip Rust tests unless the user explicitly requests them.
- Record durable boundary changes as an ADR and update `docs/README.md`.
