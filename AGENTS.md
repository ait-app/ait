# Repository guidance

Before changing domain boundaries, read `docs/README.md`.

- Rust is the fixed implementation language; keep the root Cargo workspace buildable.
- Before any Rust change, read and follow [the Rust style guide](docs/policy/rust.md). All Rust changes, including tests and refactors, MUST comply with it.
- Dependencies point inward: adapters implement ports; application coordinates domain behavior.
- Never commit credentials, provider tokens, local SQLite databases, or runtime artifacts.
- Run format and lint checks appropriate to the changed files before handing off changes. `npm run verify` maps changed files to the format, lint, affected-test and docs checks; CI runs the same script.
- Run only tests for the changed code and directly related behavior, including when preparing commits or updating PRs. Run full Rust workspace tests only when explicitly requested by the user; workspace-wide coverage counts as a full test run and requires the same explicit request. A request to commit changes or update a PR does not imply a request for full tests or coverage.
- Keep provider behavior tests in the crate that owns the behavior: an adapter's tests in its `crates/provider-<adapter>`, runtime tests in `crates/provider`, tests that need several real adapters in `crates/provider-builtin`. This holds for every adapter, including Oh My Pi in `crates/provider-omp` (ADR-249); see [ADR-270](docs/decisions/providers/adr-270-provider-crate-family.md). Keep them next to the owning code. Daemon tests cover host lifecycle, transport, and cross-capability integration; see [ADR-179](docs/decisions/providers/adr-179-provider-owned-tests.md).
- If the current task changes no Rust code in `bins/` or `crates/`, skip Rust tests unless the user explicitly requests them.
- Record durable boundary changes as an ADR (see the threshold in [the documentation policy](docs/policy/documentation.md)), then regenerate the category index with `node scripts/adr.mjs index` and update the architecture overview and the affected spec under `docs/architecture/`. `docs/README.md` is navigation only; do not list ADRs there.
- Before changing a file, run `node scripts/adr.mjs governs <path>` to list the ADRs in force for it.
