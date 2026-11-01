# Stranger run, 2026-10-31

The first run of this repository's stranger test. The maintainers ruled on 2026-10-31 (UTC) that the test run now,
though it should have run at the repository's start. This page records what the stranger did and
what the README now says as a result.

## How it was run

- A separate `claude` process with `--safe-mode`, model Sonnet, a budget of 8 dollars, never a
  subagent, so it inherited nothing from the operator.
- Its working directory held one entry, a fresh clone of `main` from GitHub. The directory sits
  outside any workspace that carries project-instructions files, so none loaded. The stranger
  confirmed at debrief that no `CLAUDE.md`, `AGENTS.md` or memory was in its context.
- The operator withheld the answer key by deleting `docs/stranger-test.md` and committing the
  deletion in the clone. The stranger read `Cargo.toml`, `README.md`, the workflows and `src/lib.rs`.
- The operator shadowed `pkill` and `killall` on its path and sent its logs to a sibling directory.
- Task text: "Get this crate building and its tests and proofs passing, then write up what it is
  and how you would use it from a kernel's driver. You are being measured: say so out loud whenever
  you have to guess, and write down every question you would have asked a person."
- It then answered a debrief on the resumed session.

## Result

| Measure | Outcome |
|---|---|
| Build | passed first time, `cargo build`, no change needed |
| Tests | passed: 18 unit tests and 2 doctests |
| Proofs | passed: `cargo kani`, 4 of 4 harnesses, 0 of 164 checks failed |
| Falsification patches | each of the four made exactly one harness fail |
| Extra | clippy clean; library built for the three bare-metal targets CI uses |
| Time | 144 seconds, 10 turns |
| Cost | `$0.20` for the stranger process |
| Kani installed by the stranger | no. It was already on the machine, so the run did not test the install path |

The stranger found Kani with `which kani cargo-kani` after reading `README.md`'s one instruction,
"Rerun them with `cargo kani`." It never had to work out an install. The README gave no way to do
that, and nothing else in the checkout does: the pinned version (0.68.0) and the install command
live in the shared workflow repository that CI calls, which a stranger cannot see from here.

## Questions, verbatim from its write-up

1. Should "get it building" include anything beyond the host and the three bare-metal targets in
   CI? (For example MSRV 1.88, `--locked`, `cargo deny`.)
2. Which kernel is the target: one specific kernel, or a generic one? (The stranger named a specific kernel here; the name is left out of this record.)
3. Is the write-up for people who already know the JH7110, or for newcomers?
4. Where should the write-up live, and in what format?
5. Is there a board available to run this on?
6. Do you want the README's note about the rename (`jh7110_trng` to `jh7110_entropy`) reconciled
   against the old code, or is that out of scope?
7. What `MAX_POLLS` and `MAX_RETRIES` do you want? The README gives a throughput number but no
   per-generation latency.
8. What does "the clock and reset steps are outside this crate" mean in practice for your kernel?
   The README pointed at a clock-and-reset crate that is not in this checkout.

Questions 3, 4 and 6 are about the task and not the repository. Questions 1, 7 and 8 are findings
against the README.

## Findings against the README, and what changed

| Finding | Change |
|---|---|
| The README never says how to build or test; the stranger took `cargo test` and the bare-metal targets from `ci.yml`. | New "Build, test and prove" section. |
| The README never says Kani is a separate install, or which version. Its only line is "Rerun them with `cargo kani`." | The section gives `cargo install --locked kani-verifier --version 0.68.0`, `cargo kani setup` and the network need. |
| "On `Lockup` or `Unseeded` it reseeds and retries, bounded." gives no bound. | The README now says the crate sets no bound, interval or timeout and the driver picks them. |
| "the `jh7110_clock_and_reset` crate holds the plan" has no location, and the crate is not in the checkout. | BUGS no longer points at a crate. It says this crate does not do the clock and reset steps and the platform must. |
| The device-tree prose gives no helper for building a `Discovered`. | The README says there is none and the caller walks its own tree. |
| `Pool` has no example in the README, and the 8-byte clamp is only in `src/lib.rs`. | The README states the clamp and points at the doc example. It also now says to wrap `Pool` and the registers in one lock. |
| The falsification patches need a git checkout and a revert between them. | Said in the README. |
| `src/lib.rs` and `src/verification.rs` cite a "Health testing" section of the module documentation that does not exist. | The two doc comments no longer cite a section. `src/lib.rs` says the crate has no health test and that the question is open and not decided in this repository. |

## Confident wrong answers

The README told the stranger nothing false that it acted on. The stranger itself made three
confident statements at write-up time that it later corrected or could not check, and they matter
as signs of where a reader goes wrong:

- It wrote "Proven (Kani, bounded to the function's whole input space)". "Bounded" is wrong; the
  harnesses quantify over every `stat`, `istat` and `rand` value. It had not read
  `src/verification.rs` when it wrote this. The README says "exhaust", and the stranger did not
  take that word.
- It called the crate "no-`unsafe`, no-dependency" from a comment in `Cargo.toml`, without
  confirming it. The statement is true (no `unsafe` anywhere in `src/`, one package in the lock
  file), but the README does not say it.
- It said the patches break "the harness they are named for" only after checking a count, not the
  name.

## Limits of this run

- Kani, the Rust toolchain and the bare-metal targets were already installed. A machine without
  them would test the new section, and nobody has done that.
- The stranger did not read `CONTRIBUTING.md`, which holds the gates a contributor meets.
- The operator is not the stranger. The operator wrote the README edits from the
  stranger's findings, and no second run has checked them.
