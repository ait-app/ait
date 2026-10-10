# Rust Style Guide

This document provides guidelines for maintaining high-quality Rust code. These rules MUST be followed by all AI coding agents and contributors.

Read this guide before every Rust change, including production code, tests, and refactors. It is based on the `rust.md` attachment to NEC-317, with the project's test-module and coverage-report requirements incorporated below.

Apply these rules within the architectural boundaries in [`AGENTS.md`](../../AGENTS.md) and the accepted decisions indexed in [`docs/README.md`](../README.md). Tool recommendations apply only when the corresponding capability is needed; they do not require adding unused dependencies or features. In particular, `domain` remains independent of runtime and adapter dependencies, and the workspace's `unsafe_code = "forbid"` rule remains in force.

## Your Core Principles

All code you write MUST be fully optimized.

"Fully optimized" includes:

- maximizing algorithmic big-O efficiency for memory and runtime
- using parallelization and SIMD where appropriate
- following proper style conventions for Rust (e.g. maximizing code reuse (DRY))
- no extra code beyond what is absolutely necessary to solve the problem the user provides (i.e. no technical debt)

Review the code for unnecessary work and allocations before handing it off, while prioritizing clarity and maintainability.

## Preferred Tools

- Use `cargo` for project management, building, and dependency management.
- Use `indicatif` to track long-running operations with progress bars. The message should be contextually sensitive.
- Use `serde` with `serde_json` for JSON serialization/deserialization.
- Use `ratatui` and `crossterm` for terminal applications/TUIs.
- Use `axum` for creating any web servers or HTTP APIs.
  - Keep request handlers async, returning `Result<Response, AppError>` to centralize error handling.
  - Use layered extractors and shared state structs instead of global mutable data.
  - Add `tower` middleware (timeouts, tracing, compression) for observability and resilience.
  - Offload CPU-bound work to `tokio::task::spawn_blocking` or background services to avoid blocking the reactor.
- When reporting errors to the console, use `tracing::error!` or `log::error!` instead of `println!`.

## Code Style and Formatting

- **MUST** use meaningful, descriptive variable and function names
- **MUST** follow Rust API Guidelines and idiomatic Rust conventions
- **MUST** use 4 spaces for indentation (never tabs)
- **NEVER** use emoji, or unicode that emulates emoji (e.g. ✓, ✗). The only exception is when writing tests and testing the impact of multibyte characters.
- Use snake_case for functions/variables/modules, PascalCase for types/traits, SCREAMING_SNAKE_CASE for constants
- Limit line length to 100 characters (rustfmt default)

## Documentation

- **MUST** include doc comments for all public functions, structs, enums, and methods
- **MUST** document function parameters, return values, and errors
- Keep comments up-to-date with code changes
- Include examples in doc comments for complex functions

Example doc comment:

````rust
/// Calculate the total cost of items including tax.
///
/// # Arguments
///
/// * `items` - Slice of item structs with price fields
/// * `tax_rate` - Tax rate as decimal (e.g., 0.08 for 8%)
///
/// # Returns
///
/// Total cost including tax
///
/// # Errors
///
/// Returns `CalculationError::EmptyItems` if items is empty
/// Returns `CalculationError::InvalidTaxRate` if tax_rate is negative
///
/// # Examples
///
/// ```
/// let items = vec![Item { price: 10.0 }, Item { price: 20.0 }];
/// let total = calculate_total(&items, 0.08)?;
/// assert_eq!(total, 32.40);
/// ```
pub fn calculate_total(items: &[Item], tax_rate: f64) -> Result<f64, CalculationError> {
````

## Type System

- **MUST** leverage Rust's type system to prevent bugs at compile time
- **NEVER** use `.unwrap()` in library code; use `.expect()` only for invariant violations with a descriptive message
- **MUST** use meaningful custom error types with `thiserror`
- Use newtypes to distinguish semantically different values of the same underlying type
- Prefer `Option<T>` over sentinel values

## Error Handling

- **NEVER** use `.unwrap()` in production code paths
- **MUST** use `Result<T, E>` for fallible operations
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors
- **MUST** propagate errors with `?` operator where appropriate
- Provide meaningful error messages with context using `.context()` from `anyhow`

## Function Design

- **MUST** keep functions focused on a single responsibility
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible
- Limit function parameters to 5 or fewer; use a config struct for more
- Return early to reduce nesting
- Use iterators and combinators over explicit loops where clearer

### Write Short Functions

Prefer small and focused functions.

**MUST** keep every function under **1000 lines**. This is a hard upper bound, not a target. If a function exceeds about 40 lines, think about whether it can be broken up without harming the structure of the program; long before approaching the 1000-line ceiling you should already have split the function into helpers.

Even if your long function works perfectly now, someone modifying it in a few months may add new behavior. This could result in bugs that are hard to find. Keeping your functions short and simple makes it easier for other people to read and modify your code. Small functions are also easier to test.

You could find long and complicated functions when working with some code. Do not be intimidated by modifying existing code: if working with such a function proves to be difficult, you find that errors are hard to debug, or you want to use a piece of it in several different contexts, consider breaking up the function into smaller and more manageable pieces.

### Keep Files Small

**MUST** keep every source file under **5000 lines**. This is a hard upper bound. When a file approaches the limit, split it into a module directory (`foo.rs` → `foo/mod.rs` plus feature-scoped submodules) before adding more code. Test files count toward the same limit. All test modules MUST live in separate child files, regardless of size; see Testing below.

## Struct and Enum Design

- **MUST** keep types focused on a single responsibility
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate
- Use `#[derive(Default)]` when a sensible default exists
- Prefer composition over inheritance-like patterns
- Use builder pattern for complex struct construction
- Make fields private by default; provide accessor methods when needed

## Testing

Run only tests for the changed code and directly related behavior, including during commit and PR preparation. Select the relevant module, test filter, integration target, or affected crate. Expand focused testing only when the change or a failure affects related code.

Run the full Rust workspace suite only when explicitly requested by the user. A request to commit changes, update a PR, or complete a handoff does not imply a request for full tests. Workspace-wide coverage runs execute the full suite too and require the same explicit request. If the current task changes no Rust code in `bins/` or `crates/`, skip Rust tests unless the user explicitly requests them, even if the working tree contains Rust changes from other tasks.

When the user explicitly requests the full suite, run `cargo nextest run --workspace` followed by `cargo test --workspace --doc`; nextest runs every test in its own process across all binaries but does not execute doctests. Do not lower test parallelism (for example `--test-threads=1` or `-j1`) to make the suite pass: a test that fails only under parallel load has a timing or shared-state bug, so fix it. `cargo test --workspace` remains a valid equivalent when nextest is unavailable.

- **MUST** write unit tests for all new functions and types
- **MUST** mock external dependencies (APIs, databases, file systems)
- **MUST** use the built-in `#[test]` attribute; tests must pass under both `cargo test` and `cargo nextest run`
- Follow the Arrange-Act-Assert pattern
- Do not commit commented-out tests
- **MUST** put all test-related modules in separate child files, including unit tests, regression suites, fixtures, and test helpers. Do not use inline test module bodies such as `mod tests { ... }`, even for a small suite; nested test modules follow the same rule.
- Gate unit-test modules with `#[cfg(test)]` and declare them with `mod tests;` (or a descriptive module name). Keep the test implementations in the child file.
- For `src/lib.rs`, `src/main.rs`, or `src/foo/mod.rs`, the child file is the sibling `tests.rs`. For `src/foo.rs`, the child file is `src/foo/tests.rs`. Split larger suites into additional named child files.
- Keep integration tests in the crate's `tests/` directory. Place shared integration-test helpers in a subdirectory such as `tests/common/mod.rs` so they are not discovered as standalone test targets.
- When adding or modifying an existing inline test module, move that module into its child file as part of the change, preserving test coverage and behavior.

### Faster local builds

- A fresh worktree recompiles all ~340 dependencies and downloads the sherpa-onnx static archive. On macOS, seed its `target/` from an existing checkout of the same toolchain with an APFS clone (`cp -c -R <checkout>/target <worktree>/target`); only workspace crates rebuild and the archive is reused. Clone time grows with the source directory, so run `cargo clean` there occasionally. Other filesystems can use a shared `CARGO_TARGET_DIR` instead, but concurrent builds then serialize on its lock.
- The dev profile keeps line tables only. For a debugger session that needs local variables, build with `CARGO_PROFILE_DEV_DEBUG=full`.

Example unit-test layout:

```text
src/
├── lib.rs
├── calculator.rs
└── calculator/
    └── tests.rs
```

`src/calculator.rs` keeps production code and the module declaration:

```rust
/// Returns the sum of two values; callers must ensure the sum fits in `u32`.
pub fn add(left: u32, right: u32) -> u32 {
    left + right
}

#[cfg(test)]
mod tests;
```

`src/calculator/tests.rs` contains the test implementation:

```rust
use super::add;

#[test]
fn adds_two_numbers() {
    assert_eq!(add(2, 3), 5);
}
```

## Code Coverage

- When measuring coverage, **MUST** use `cargo llvm-cov` (`cargo-llvm-cov`). Keep measurements within the affected crates and focused tests unless the user explicitly requests workspace-wide coverage, including during commit and PR preparation.
- Coverage measurement is not mandatory for a commit or PR. When the user explicitly requests workspace-wide coverage, generate an HTML report with `cargo llvm-cov nextest --workspace --html`; the report is written to `target/llvm-cov/html/index.html`. Do not run this full suite merely to populate a report or satisfy the commit checklist.
- Every new public function or behaviour change **MUST** be covered by at least one test; aim to keep line coverage above **80%** across the workspace
- Cover both the happy path and key error/edge-case branches
- Do not add `#[allow(dead_code)]` or dummy call sites solely to satisfy the coverage tool; fix the underlying gap with a real test
- Record coverage measurement status in the pre-commit checklist (see below)

### Project Reports

Every project progress or delivery report, including PR descriptions and issue completion reports, **MUST** include a **Test coverage** section with:

- The measured line coverage percentage and covered/total line counts for the actual measurement scope, when measured. Report focused results by default; include workspace results only when the user explicitly requested that measurement. Include the change from a comparable baseline when one exists; if none exists, say so.
- The exact command, revision, and measurement scope: workspace or selected crates, enabled features, and any excluded files, skipped tests, or unavailable platforms.
- A reviewable coverage artifact (for example, an attached report or CI artifact link), plus the important uncovered behavior and follow-up needed. The generated local report is at `target/llvm-cov/html/index.html`; a local path alone is not a shared artifact.
- Test execution results separately from coverage. A passing test count is not a coverage percentage.

If coverage was not measured, state **not measured**, the reason, and the next step if measurement is later requested; never invent a percentage or present an older result as current. Coverage not being requested is a valid reason to omit measurement, including during commit and PR preparation; reporting requirements must not expand the test scope. For documentation-only changes, the section may state **not applicable — no Rust behavior changed**, with the validation performed. An unavailable or omitted measurement does not waive the requirement to report its status.

## Imports and Dependencies

- **MUST** keep items private by default. Use `pub` only for items that another workspace crate (ultimately the `daemon` binary or its integration tests) actually uses, and `pub(crate)` for crate-internal sharing. Do not add forwarding `pub use` aliases; callers import from the owning module. The workspace enables `unreachable_pub`, so `dead_code` also reports crate-private items that nothing uses.
- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules (`use super::*`), and prelude re-exports
- **MUST** document dependencies in `Cargo.toml` with version constraints
- Use `cargo` for dependency management
- Organize imports: standard library, external crates, local modules
- Use `rustfmt` to automate import formatting

## Rust Best Practices

- **NEVER** use macros to implement business logic; use ordinary functions, methods, and types instead.
- Before adding any new macro definition, including `macro_rules!` and procedural macros, **MUST** explain the proposed design and obtain explicit confirmation from the user.
- **NEVER** use `unsafe` in this workspace; `unsafe_code = "forbid"` is enforced by the workspace lints
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and iterators
- **MUST** declare parameterless `&'static str` (or any other pure-constant) producers as `const`, not `fn`. Anything shaped like `fn FOO() -> &'static str { "..." }` should be `const FOO: &str = "...";` — call sites become `FOO` instead of `FOO()`. The constant form makes it immediately obvious there is no runtime work; it can be used in `const` contexts; and it costs zero indirection. The only reason to keep a function is when the value actually depends on a runtime input. This applies most often to hand-written SQL string helpers.
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible
- **MUST** use `format!` macro for string formatting
- Use iterators and iterator adapters over manual loops
- Use `enumerate()` instead of manual counter variables
- Prefer `if let` and `while let` for single-pattern matching

## Memory and Performance

- **MUST** avoid unnecessary allocations; prefer `&str` over `String` when possible
- **MUST** use `Cow<'_, str>` when ownership is conditionally needed
- Use `Vec::with_capacity()` when the size is known
- Prefer stack allocation over heap when appropriate
- Use `Arc` and `Rc` judiciously; prefer borrowing

## Concurrency

- **MUST** use `Send` and `Sync` bounds appropriately
- **MUST** prefer `tokio` for async runtime in async applications
- **MUST** use `rayon` for CPU-bound parallelism
- Avoid `Mutex` when `RwLock` or lock-free alternatives are appropriate
- Use channels (`mpsc`, `crossbeam`) for message passing

## Security

- **NEVER** store secrets, API keys, or passwords in code or committed files.
  - Keep local `.env` files out of Git; `.gitignore` already excludes `.env` and `.env.*`.
- **MUST** read sensitive configuration from environment variables injected at runtime via `std::env`; the daemon does not load credentials from `.env`, command-line flags, URLs, or TOML (see [daemon operations](../operations/daemon.md)).
- **NEVER** log sensitive information (passwords, tokens, PII)
- Use `secrecy` crate for sensitive data types

## Version Control

- **MUST** write clear, descriptive commit messages
- **NEVER** commit commented-out code; delete it
- **NEVER** commit debug `println!` statements or `dbg!` macros
- **NEVER** commit credentials or sensitive data

## Tools

- **MUST** use `rustfmt` for code formatting
- **MUST** use `clippy` for linting and follow its suggestions
- **MUST** ensure code compiles with no warnings (use `-D warnings` flag in CI, not `#![deny(warnings)]` in source)
- Use `cargo` for building, testing, and dependency management
- Use `cargo nextest run` for full test runs and `cargo test` for doctests or a single focused target
- Use `cargo doc` for generating documentation

## Before Committing

Apply this checklist when preparing a commit, not during routine local edits or handoffs. For Rust changes in `bins/` or `crates/`, run affected tests and applicable format, lint, and build checks. Full Rust workspace tests and workspace-wide coverage require an explicit user request; preparing a commit does not make them mandatory. Documentation-only changes do not require Rust tests or coverage unless the user explicitly requests them.

- [ ] Affected Rust tests pass; run the full workspace suite only if explicitly requested by the user
- [ ] No compiler warnings (`cargo build --workspace`)
- [ ] Clippy passes (`cargo clippy --locked --workspace --all-targets -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --all --check`)
- [ ] All public items have doc comments
- [ ] No commented-out code or debug statements
- [ ] No hardcoded credentials
- [ ] New behavior is covered by focused tests; coverage measurement status is reported, with an artifact if measured. Run workspace-wide coverage only if explicitly requested by the user
- [ ] Test modules are declared in their parent and implemented in separate child files
- [ ] Project report includes test coverage status, measurement scope and results when measured, and an artifact (or an explicit reason coverage was not measured or is not applicable)

---

**Remember:** Prioritize clarity and maintainability over cleverness.
