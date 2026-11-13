# Code documentation standard (for AI maintainers)

Ask Hava's code is written, changed and fixed by AI sessions, so it has to explain itself to a session that has never seen it. A fresh session must be able to answer three questions from the repository alone, without guessing: *where does this rule live*, *what must I not break*, and *how do I check my change*. This file sets the standard; `scripts/check_code_docs.py` enforces the parts a script can check, and runs in `Ci` through the pytest step (D-473).

## 1. Where a fresh session starts

1. `CLAUDE.md` at the root: the rules and the reading order.
2. `docs/README.md` (written by T141): a map of the repository, one line per folder and per document, and the common changes with the files each one touches.
3. `docs/architecture.md`: the modules, what each owns, and the dependency rules.
4. The crate's own `CLAUDE.md` (section 2) for the module it is changing.
5. The spec file for the requirement (`docs/spec/*.md`), found by its ID.

## 2. Every crate has a `CLAUDE.md`

Each crate under `app/` keeps a short `CLAUDE.md` beside its `Cargo.toml` (Claude Code loads it on its own when a session works in that folder). It has these six headings, in this order:

- `## Purpose`: two or three sentences on what the module is for, in plain words.
- `## What it owns`: its tables, its files and its public functions, one entry per line.
- `## Rules it enforces`: each requirement ID it implements, with the function or file that does it (`FEE-039: payments::fees::recurring_basis`).
- `## Never break`: the invariants a change must keep, each with the test that guards it (for example, "the homeowner's phone never leaves the vault: `tests/security/tests/vault_readers.rs`").
- `## How to change`: recipes for the changes people will actually ask for (change a fee, add a job type, edit a text, add a setting), each naming the files to touch, the spec line to update and the tests to run.
- `## Tests that guard it`: the test files and test-design IDs for this module.

Any task that adds or changes behavior in a crate updates that crate's `CLAUDE.md` in the same pull request. Every crate's `CLAUDE.md` is a shared path, like its `Cargo.toml`.

**One entry per line, under a size budget (D-3474).** Two tasks that each add a line insert at one place, which the runner merges by itself (D-3269); two tasks that both lengthen one long line change the same line, which nothing can merge and which costs a resync (22 of the 25 resyncs before D-3474 involved a crate `CLAUDE.md`, 23 file conflicts, every one a same-line rewrite). So:

- Under the five list headings (every heading but `## Purpose`) each line is one bullet (`- `) holding one entry: one source file and what it owns, one rule and the function that applies it, one invariant and its test, one recipe, or one test file and its IDs. A task adds bullets of its own and never appends to another task's bullet; a change to an existing fact edits only that fact's bullet.
- No line is longer than 400 characters, `## Purpose` included; an entry that needs more is two entries.
- A note about which task did what ("T123 added ...") goes in the bullet for the file or rule it changed, never as a line or paragraph of its own.
- The whole file is at most 84,000 bytes, about 21,000 tokens or a tenth of a worker's 200,000-token window, which every session in the crate loads. There is no line limit once T437 replaces the 120-line limit with this budget.
- The runner holds a crate to one task at a time while its file, plus 2,000 bytes for each task in flight there and for the next one, would pass the budget, or while the file is still in the old format below (D-3303, D-3474). The task holding such a crate may condense its file, since no other task can be writing it.
- Old format: T437 puts one marker line under the title of each crate file that still has a line over 400 characters or a line under a list heading that is not a bullet. The next task that starts in a marked crate (not a fix round and not a resync) splits those lines into one entry per line, keeping every fact, requirement ID, decision ID and path, and deletes the marker line in the same pull request. That rewrite is safe because the runner lets no other task into a marked crate meanwhile.

A task adds or edits only the lines its change needs. Condensing or restructuring a crate's `CLAUDE.md` (joining bullets, reordering, rewording lines it does not own, as D-3304 and D-3308 did) is a separate change, made only while no task that writes that crate is in flight (IN_PROGRESS or MERGE_PENDING in `kit/state.json` on `claude/kit-state`); its decision entry names the check. A rewrite under a running task's edits cannot be merged by insertion and forces a resync (D-3386, D-3444). The two exceptions above, the task holding a crate at its budget and the next task in an old-format crate, meet that condition by construction (D-3474).

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

## 9. Files every task adds to stay append-only

The same reasoning as section 2 (D-3474): a shared file merges by itself only when each task adds whole lines of its own.

- A crate's `src/api.rs` holds its `//!` module comment and one-line `pub use` and `pub mod` re-exports, nothing else. No per-task changelog paragraph ("T123: ...") in the module comment: the note goes in the crate `CLAUDE.md` bullet or in the `//!` comment of the module that does the work. No type, function, `impl`, constant or static is defined there (a page struct, a helper): it is defined in its own module and re-exported. A new re-export is a line of its own, not a name added to an existing `{...}` list.
- Existing changelog paragraphs and definitions in `api.rs` files stay where they are: moving them would rewrite lines that tasks in flight edit, and some (the vault's release proof, for one) sit there on purpose. From T437, `scripts/check_code_docs.py` holds each crate's `api.rs` to the number of `//!` lines and definitions it had when T437 merged (at least 12 `//!` lines), so both counts can only fall.
- In `app/server/src/lib.rs`, a new worker-task registration goes next to the registrations of its own crate (after the last of them), or, for a crate with none yet, among the other crates' registrations in alphabetical order of crate; never simply at the end of `register_handlers` or `worker_registry`, where every task's addition would meet. Anything longer than one `registry.register` statement (a closure, settings, several kinds) goes in a `register_<name>` function in the task's own module under `app/server/src/`, called from one line. A task goal that says "register it in `register_handlers`" is met this way. Existing registrations are not moved.
