# Code documentation standard (for AI maintainers)

Ask Hava's code is written, changed and fixed by AI sessions, so it has to explain itself to a session that has never seen it. A fresh session must be able to answer three questions from the repository alone, without guessing: *where does this rule live*, *what must I not break*, and *how do I check my change*. This file sets the standard; `scripts/check_code_docs.py` enforces the parts a script can check, and runs in `Ci` through the pytest step (D-473).

## 1. Where a fresh session starts

1. `CLAUDE.md` at the root: the rules and the reading order.
2. `docs/README.md` (written by T141): a map of the repository, one line per folder and per document, and the common changes with the files each one touches.
3. `docs/architecture.md`: the modules, what each owns, and the dependency rules.
4. The crate's own `CLAUDE.md` (section 2) for the module it is changing.
5. The spec file for the requirement (`docs/spec/*.md`), found by its ID.

## 2. Every crate has a `CLAUDE.md`

Each crate under `app/` keeps a short `CLAUDE.md` beside its `Cargo.toml` (Claude Code loads it on its own when a session works in that folder). It is at most 120 lines and has these six headings, in this order:

- `## Purpose`: two or three sentences on what the module is for, in plain words.
- `## What it owns`: its tables, its files and its public functions, one line each.
- `## Rules it enforces`: each requirement ID it implements, with the function or file that does it (`FEE-039: payments::fees::recurring_basis`).
- `## Never break`: the invariants a change must keep, each with the test that guards it (for example, "the homeowner's phone never leaves the vault: `tests/security/tests/vault_readers.rs`").
- `## How to change`: recipes for the changes people will actually ask for (change a fee, add a job type, edit a text, add a setting), each naming the files to touch, the spec line to update and the tests to run.
- `## Tests that guard it`: the test files and test-design IDs for this module.

Any task that adds or changes behavior in a crate updates that crate's `CLAUDE.md` in the same pull request. Every crate's `CLAUDE.md` is a shared path, like its `Cargo.toml`.

## 3. Every Rust file opens with a module comment

The first lines of every `.rs` file under `app/` and `tests/` are `//!` comments. They say what the file does, why it exists, and the requirement IDs it implements or tests. Example:

```rust
//! Introduction fee by job size (FEE-001, FEE-039, FEE-043).
//!
//! Turns a quote into its fee tier. Ongoing service counts 4 months of the
//! monthly price and never pays more than $75. The tier is fixed at the pick
//! (FEE-033); nothing here talks to Stripe.
```

## 4. Every public item has a doc comment

- The workspace lints deny `missing_docs` and turn on clippy's `missing_errors_doc` and `missing_panics_doc` (T009), so a public item with no `///` comment fails `Ci`.
- A function that applies a business rule names the requirement IDs in its doc comment, so a search for `FEE-039` finds the code, the test and the spec line.
- Comments say *why*. Names say *what*. No comment restates the code.
- A number that comes from the spec (a fee, a timer, a limit) lives in the catalog or settings, never as a bare literal in logic, and its comment names the requirement.

## 5. Every requirement can be traced

- Every requirement ID mentioned in code or a crate `CLAUDE.md` must exist in `docs/spec/`.
- Once every file a build task owns exists, each requirement listed for that task in `kit/tasks.json` must be named somewhere in those files. That keeps spec, code and tests joined up as the build goes.
- Tests already carry their test-design IDs (`docs/test-design.md`, rule 2).

## 6. Migrations and scripts

- Every SQL file in `migrations/` starts with a `--` comment saying what it changes and why, with requirement IDs.
- Every Python script in `scripts/` starts with a docstring saying what it checks and how to run it.

## 7. Decisions stay findable

A change of behavior goes in `docs/decisions.md` (question, choice, reason, source line) and `docs/architecture.md` in the same pull request. A crate's `CLAUDE.md` links to the decision IDs behind anything surprising, so a later session doesn't "fix" a deliberate choice.

## 8. Checked at the end by a session that knows nothing

Before "Finished", T171 runs a cold-start drill: a fresh session given only the repository makes three typical changes on a throwaway branch and records every place it had to guess. Each gap is fixed in the docs before the build is called finished.
