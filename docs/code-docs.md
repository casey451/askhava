# Code documentation standard (for AI maintainers)

Ask Hava's code is written, changed and fixed by AI sessions, so it has to explain itself to a session that has never seen it. A fresh session must be able to answer three questions from the repository alone, without guessing: *where does this rule live*, *what must I not break*, and *how do I check my change*. This file sets the standard; `scripts/check_code_docs.py` enforces the parts a script can check, and runs in `Ci` through the pytest step (D-473).

## 1. Where a fresh session starts

1. `CLAUDE.md` at the root: the rules and the reading order.
2. `docs/README.md` (written by T141): a map of the repository, one line per folder and per document, and the common changes with the files each one touches.
3. `docs/architecture.md`: the modules, what each owns, and the dependency rules.
4. The crate's own `CLAUDE.md` (section 2) for the module it is changing, and the lines of its `NOTES.md` that name the requirement IDs at hand.
5. The spec file for the requirement (`docs/spec/*.md`), found by its ID.

## 2. Every crate has a short `CLAUDE.md` and a greppable `NOTES.md`

Claude Code loads a crate's `CLAUDE.md` into every session that reads a file in the crate, so it stays small and the detail sits beside it, read by grep (D-3474). Both are shared paths.

- **`CLAUDE.md`**: at most 8,192 bytes and 120 lines, headings in this order: `## Purpose` (a sentence or two), `## Never break` (one bullet per invariant, each with its guarding test, for example "the homeowner's phone never leaves the vault: `tests/security/tests/vault_readers.rs`"; a long one may end "More: `(never N)` in NOTES.md.") and `## Detail` (the pointer to `NOTES.md`). It loads on its own, so the prompt only points to it.
- **`NOTES.md`**: everything else (owned tables, files and public functions; each requirement ID with the function that applies it; change recipes; tests and test-design IDs; decision IDs behind anything surprising). Line 1 is a `# ` title; every other line is one entry, `- [REQ-IDS or -] (section) text`, the section one of `purpose`, `owns`, `rules`, `never N`, `change`, `tests`, or `task NNN` for a task's build note (what task NNN built, once a "What T### built" paragraph in `docs/architecture.md`, D-3620; `grep -n '(task 250)' app/*/NOTES.md docs/build-notes.md`), the text at most 400 characters, the line at most 600. Grep it (`grep -n 'FEE-039' app/payments/NOTES.md`); never read it whole.

A task that adds or changes behavior adds one `NOTES.md` line per new fact at its end, and a `## Never break` bullet only for a new invariant. It never lengthens or joins another task's line, and edits a line only when its change makes that line false. `.gitattributes` union-merges `app/*/NOTES.md`, so two tasks that append both keep their lines. Until T437 makes `NOTES.md` a shared path (`grep -q NOTES.md runner/verify.py`), a task adds at most 2 entry lines, each at most 400 bytes, at the end of the crate's `CLAUDE.md`, after the `## Detail` paragraph (the split left each guide at most 6,052 bytes, room for two tasks in flight at `crate_cap` 2; a task whose guide would pass 8,192 bytes shortens its own new lines and moves the rest to the `//!` comment of its own module); entries naming the brand also wait there, since the brand-literal guard skips only `CLAUDE.md` until T437; `python3 scripts/split_crate_doc.py --sweep app/<crate>` moves them. That script made the split; `--verify <ref>` checks every word of the old file survived.

Condensing or restructuring either file (joining entries, reordering, rewording lines it does not own, as D-3304 and D-3308 did) is a separate change, made only while no task that writes that crate is in flight (IN_PROGRESS or MERGE_PENDING in `kit/state.json` on `claude/kit-state`); a rewrite under a running task's edits forces a resync (D-3386, D-3444).

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
- Ask Hava is national-ready (D-1515): new code takes market-specific values (the towns, the licensing and identity sources, the quiet-hours and business-clock time zone, tax treatment, a launch date) from data (`docs/catalog/markets.yaml` and the catalog tables, or settings), never as a literal Lake Havasu City or Arizona value, except where a requirement line names one (for example JOB-007, LIC-011, QH-001, FEE-046), and such a literal's comment names that requirement.

## 5. Every requirement can be traced

- Every requirement ID mentioned in code or a crate `CLAUDE.md` or `NOTES.md` must exist in `docs/spec/`.
- Once every file a build task owns exists, each requirement listed for that task in `kit/tasks.json` must be named somewhere in those files. That keeps spec, code and tests joined up as the build goes.
- Tests already carry their test-design IDs (`docs/test-design.md`, rule 2).

## 6. Migrations and scripts

- Every SQL file in `migrations/` starts with a `--` comment saying what it changes and why, with requirement IDs.
- Every Python script in `scripts/` starts with a docstring saying what it checks and how to run it.

## 7. Decisions stay findable

A change of behavior goes in `docs/decisions.md` (question, choice, reason, source line) in the same pull request, and in `docs/architecture.md` when it changes the design (its section 12); what the task built goes in its crate's `NOTES.md` as `(task NNN)` lines, never as a paragraph in `docs/architecture.md` (D-3620). A crate's `NOTES.md` links to the decision IDs behind anything surprising, so a later session doesn't "fix" a deliberate choice.

## 8. Checked at the end by a session that knows nothing

Before "Finished", T171 runs a cold-start drill: a fresh session given only the repository makes three typical changes on a throwaway branch and records every place it had to guess. Each gap is fixed in the docs before the build is called finished.

## 9. Code files every task adds to stay append-only

As in section 2, a shared file merges by itself only when each task adds whole lines of its own (D-3474).

- `src/api.rs` holds its `//!` comment and one-line `pub use` and `pub mod` re-exports. No per-task changelog paragraph there (put it in `NOTES.md` or the working module's `//!`), and no new definition: define it in its own module and re-export it, one line each, also when a goal says "add it to `api.rs`". Existing paragraphs and definitions stay.
- In `app/server/src/lib.rs`, a new worker-task registration goes in its crate's block, in alphabetical order of its kind constant (a new crate's block among the others by crate name), never just at the end of `register_handlers` or `worker_registry`; more than one `registry.register` statement goes in a `register_<name>` function in the task's own module, called from one line. Existing registrations stay.
