# DBCrab

DBCrab is a modern REPL-first PostgreSQL client.

## App features

- There are two editors: SQL and Command.
- Each editor supports two edit modes: Emacs and Vi.
- Each editor supports reverse history search.
  - Only the SQL editor persists history across sessions.
- Both editors share the same core editing behavior and configuration.
- SQL editor has schema-aware and context-aware auto-complete and suggestions.

## Rust Standards

- Prefer expression-oriented and functional code.
- Write idiomatic modern Rust with clear ownership and minimal to zero cloning.
- Prefer borrowing over cloning, but optimize for clarity before micro-optimizing allocations.
- Avoid `unwrap`, `expect`, and `panic!`.
- Use typed errors in reusable code and add application-level context at the boundaries.
- Preserve error sources and add context at process boundaries, user-facing boundaries, and async task boundaries.
- Use your judgment, if code or design quality degrades or refactoring feels warranted, pause and ask before proceeding.

## Coding Patterns

- Builder Pattern: For constructing complex objects step by step
- RAII Pattern: For resource management tied to object lifetimes
- Newtype Pattern: For type safety and abstraction
- Decorator Pattern: For adding behavior to objects dynamically
- Command Pattern: For encapsulating operations as objects

## Testing

- Read the Unit Testing skill before modifying or adding unit tests.
- Gate tests that require a live PostgreSQL instance behind explicit environment setup.

## Ask Before

- Adding runtime dependencies.
- Introducing persisted configuration or changing its format.

## Development Commands

Before finishing code changes, run:

```bash
cargo fmt
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```
