# jh7110_entropy

The logic of a driver for the true random number generator (TRNG) in the StarFive JH7110, the
RISC-V system-on-chip of the VisionFive 2. It holds the register layout and the decision of whether a
generation succeeded, failed or is still running. It also holds a small pool that serves the 32
bytes of one generation without serving any byte twice, and what each of the chip's two device
trees calls the block. It never touches a register: you map the window and do the volatile reads, and this crate
decides what they mean. It is `no_std` and tested on the host. The nife kernel is its first caller.

## Build, test and prove

You need a Rust toolchain at or above the minimum version at the foot of this page (`rustup` is the
usual way to get one) and a git checkout. From the crate root:

- `cargo build` and `cargo test` run on the host and need nothing else. The tests are 18 unit tests
  and 2 doctests.
- The crate also builds for bare metal. Add the three targets with `rustup target add
  riscv64imac-unknown-none-elf aarch64-unknown-none-softfloat x86_64-unknown-none`. Then run
  `cargo build --lib --target <target>` for each. CI does exactly this.
- The proofs need Kani, which `rustup` does not install. Run `cargo install --locked
  kani-verifier --version 0.68.0`, then `cargo kani setup`. The setup step downloads a model
  checker, so it needs network access. CI uses version 0.68.0, and another version may verify
  differently. After that, `cargo kani` runs all four harnesses.

## What is proven

Four Kani harnesses exhaust `generation_outcome`, the function that turns one `(STAT, ISTAT)` snapshot and
the eight `RAND` words into an answer:

- `verification::a_lockup_bit_is_never_overridden`: whenever `ISTAT.LFSR_LOCKUP` reads 1, the answer
  is `Lockup`, whatever else the register says.
- `verification::ready_requires_rand_rdy_and_carries_the_words_untouched`: `Ready` comes only from
  `RAND_RDY` on a seeded core, and its 32 bytes are the eight words, little-endian, word 0 first.
- `verification::neither_bit_set_is_always_not_ready`: a seeded core with neither bit set is
  `NotReady`.
- `verification::an_unseeded_core_never_yields_bytes`: with `STAT.SEEDED` clear, no `ISTAT` value
  yields bytes.

Each harness has a falsification record at
`falsifications/<module.path>.<harness>.patch`; applying it with `git apply -p1` from the crate root
makes that harness fail at its own assertion, which shows the proof checks something. The patches
need a git checkout, and you revert each one with `git checkout -- src` before the next.

Not proven: the quality of the randomness, which is a property of the silicon; `Pool` and
`output_bytes`, which copy words and have unit tests; and the device-tree decoders, which have tests too.
`Pool::take` serves at most `WORD_BYTES`, 8 bytes, per call, and its doc comment carries a runnable
example.

## The two device trees

Mainline Linux names the block `starfive,jh7110-trng`. The vendor U-Boot's control tree, which a
VisionFive 2 boots with, names the same block `starfive,trng`, at the same window (`0x1600_C000`,
`0x4000` bytes) and interrupt (30), and marks it `status = "disabled"` because U-Boot has no driver
for it. `COMPATIBLES` lists both, mainline's first. Walk your tree for a node carrying one, read
its `reg`, decode `interrupts` with `interrupt_line` and `status` with `is_status_okay`, and keep the
result in a `Discovered`. A disabled node is the first suspect when the window reads all zeros. The crate has no helper that
walks a tree for you; it decodes the properties and you do the walking with your own parser.

## EXAMPLES

```rust
use jh7110_entropy::{GenerationOutcome, generation_outcome, is_status_okay, ISTAT_RAND_RDY, STAT_SEEDED};

// Firmware that says nothing about `status` means "okay".
assert!(is_status_okay(None));
assert!(!is_status_okay(Some(b"disabled\0")));

// A seeded core with RAND_RDY set: the eight words, little-endian, word 0 first.
let rand = [0x0403_0201, 0, 0, 0, 0, 0, 0, 0];
match generation_outcome(STAT_SEEDED, ISTAT_RAND_RDY, rand) {
    GenerationOutcome::Ready(bytes) => assert_eq!(&bytes[..4], &[1, 2, 3, 4]),
    other => panic!("expected Ready, got {other:?}"),
}
```

A driver writes `MODE_R256` during bring-up (otherwise only four words carry the answer), clears
`ISTAT` with `ISTAT_ALL`, asks for a reseed with `CTRL_COMMAND_RESEED`, then for each request
writes `CTRL_COMMAND_GENERATE`, polls `STAT` and `ISTAT`, and hands them to `generation_outcome`. On `Lockup` or
`Unseeded` it reseeds and retries, bounded. The crate sets no bound, no poll interval and no
timeout: you pick them, ideally from a measurement on your board. The only timing figure here is the
955,223 bytes per second under "On silicon". Wrap `Pool` and the register access in one lock; `Pool`
does no locking of its own.

## On silicon

What ran, and when. On 2026-09-04 a confined userspace driver in the nife kernel used this logic on radon, a VisionFive 2. It served 32 plus 32 bytes of real TRNG output on each of two boots. The first draw differed across the boots, and the two draws within a boot differed from each other. `STAT` read `0x00040308` after bring-up, which shows the chip is in 256-bit mode, so all eight `RAND` words are the answer. The bench found and fixed three faults: a gated clock and reset, a get that asked for more bytes than the channel carried, and an `ISTAT` clear that had to be write-one-to-clear. On 2026-09-16 the bench timed the step at 955,223 bytes per second. The records are in the roadmap of the [nifeos/nife](https://github.com/nifeos/nife) repository, under its TRNG driver entry and its entry on timing the hardware-entropy step.

What is not established. The quality of the randomness is open. A TRNG has no known answer, and the bench checked only that the bytes were nonzero, differed across boots and differed between draws. There is no health test and no SP 800-90B-class measurement. Nife decision section 137 on health tests is still `PROPOSED`.

What is not verified. Nobody has run this repository on the board since it left nife. The copy here is not byte for byte the code that ran on 2026-09-04: the crate carried the name `jh7110_trng` then, and renames and documentation edits since have changed most lines. The decision function (`generation_outcome`, then `interpret`) and the byte layout (`output_bytes`, then `assemble`) read the same line for line in the two versions. Nobody has compared the two versions by running them.

The clock and reset steps are outside this crate, as the first item under BUGS says.

## BUGS

- The TRNG needs its clocks ungated and its reset released first. This crate does not do that;
  the `jh7110_clock_and_reset` crate holds the plan, or your platform must do it some other way.
  That crate is in the crates directory of the [nifeos/nife](https://github.com/nifeos/nife)
  repository, not in this checkout, and this crate does not depend on it.
- The device-tree walk that uses it has tests against fixtures written by hand from the mainline
  binding's example and the vendor U-Boot tree, not dumps from a board.

Minimum Rust version: 1.88.

## License

MIT OR Apache-2.0, at your option: see `LICENSE-MIT` and `LICENSE-APACHE`.

What the crate takes from Linux, NetBSD, the vendor trees and the JH7110 manual, and why it owes
them no notice, is in [docs/the-crate-carries-facts-not-source-text.md](docs/the-crate-carries-facts-not-source-text.md).
