# jh7110_entropy

[![check](https://github.com/nifeos/jh7110_entropy/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/nifeos/jh7110_entropy/actions/workflows/ci.yml?query=branch%3Amain)
[![proofs](https://github.com/nifeos/jh7110_entropy/actions/workflows/proofs.yml/badge.svg?branch=main)](https://github.com/nifeos/jh7110_entropy/actions/workflows/proofs.yml?query=branch%3Amain)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/nifeos/jh7110_entropy#license)
[![minimum Rust version](https://img.shields.io/badge/rust-1.88%2B-blue.svg)](https://github.com/nifeos/jh7110_entropy/blob/main/Cargo.toml)

The logic of a driver for the true random number generator (TRNG) in the StarFive JH7110, the
RISC-V system-on-chip of the VisionFive 2. It holds the register layout and the decision of whether a
generation succeeded, failed or is still running. It also holds a small pool that serves the 32
bytes of one generation without serving any byte twice, and what each of the chip's two device
trees calls the block. It never touches a register: you map the window and do the volatile reads, and this crate
decides what they mean. It is `no_std` and tested on the host. It has no dependencies and holds no
`unsafe` code.

## EXAMPLES

```rust
use jh7110_entropy::{GenerationOutcome, generation_outcome, is_status_okay, INTERRUPT_STATUS_RANDOM_NUMBER_READY, STATUS_SEEDED};

// Firmware that says nothing about `status` means "okay".
assert!(is_status_okay(None));
assert!(!is_status_okay(Some(b"disabled\0")));

// A seeded core with RAND_RDY set: the eight words, little-endian, word 0 first.
let random_numbers = [0x0403_0201, 0, 0, 0, 0, 0, 0, 0];
match generation_outcome(STATUS_SEEDED, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers) {
    GenerationOutcome::Ready(bytes) => assert_eq!(&bytes[..4], &[1, 2, 3, 4]),
    other => panic!("expected Ready, got {other:?}"),
}
```

A driver writes `MODE_256_BIT_OUTPUT` during bring-up (otherwise only four words carry the answer), clears
`ISTAT` with `INTERRUPT_STATUS_DOCUMENTED_BITS_MASK`, asks for a reseed with `CONTROL_COMMAND_RESEED`, then for each request
writes `CONTROL_COMMAND_GENERATE`, polls `STAT` and `ISTAT`, and hands them to `generation_outcome`. On `Lockup` or
`Unseeded` it reseeds and retries, bounded. The crate sets no bound, no poll interval and no
timeout: you pick them, ideally from a measurement on your board. This
README gives no timing figure. Wrap `Pool` and the register access in one lock; `Pool`
does no locking of its own.

## What is proven

Four Kani harnesses exhaust `generation_outcome`, the function that turns one snapshot of the status
register (`STAT`) and the interrupt status register (`ISTAT`), plus the eight random-output words
(`RAND`), into an answer:

- `verification::a_lockup_bit_is_never_overridden`: whenever `ISTAT.LFSR_LOCKUP` (a lockup of the linear-feedback shift register (LFSR) that feeds the
  generator) reads 1, the answer
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
  differently. After that, `cargo kani` runs all four harnesses, which takes about 15 seconds on a laptop-class
  machine. To run one, add `--harness <name>`.

## The two device trees

Mainline Linux names the block `starfive,jh7110-trng`. The vendor U-Boot's control tree, which a
VisionFive 2 boots with, names the same block `starfive,trng`, at the same window (`0x1600_C000`,
`0x4000` bytes) and interrupt (30), and marks it `status = "disabled"` because U-Boot has no driver
for it. `COMPATIBLES` lists both, mainline's first. Walk your tree for a node carrying one, read
its `reg`, decode `interrupts` with `interrupt_line` and `status` with `is_status_okay`, and keep the
result in a `Discovered`. A disabled node is the first suspect when the window reads all zeros. The crate has no helper that
walks a tree for you; it decodes the properties and you do the walking with your own parser.

## Hardware evidence

A driver built on this logic has produced real TRNG output on a VisionFive 2 with the chip in
256-bit mode, so all eight `RAND` words are the answer. Ungate the clock and release the reset
first, as the first item under BUGS says.

Not established:

- The quality of the randomness. A TRNG has no known answer. There is no health test and no
  National Institute of Standards and Technology (NIST) Special Publication 800-90B-class measurement, and the maintainers have not decided whether this crate should carry a health test.
- A run of this exact source tree on hardware. An earlier version of the code, under a different
  crate name, is the one that ran. Until someone runs this tree on hardware and records the result, treat its
  behavior on the board as unconfirmed.

## BUGS

- The TRNG needs its clocks ungated and its reset released first. This crate does not do that;
  your platform must do it, in the driver or in firmware, before the first generation.
- The two compatible strings come from the mainline binding's example and the vendor U-Boot tree,
  not from dumps of a board's device tree. No test here walks a real tree.
- No fuzz target. The only functions that read bytes a caller hands in are the two device-tree
  decoders: `interrupt_line` reads at most the first four bytes of a slice, and `is_status_okay`
  compares a slice with four fixed strings. Neither loops or holds `unsafe` code, and their unit tests
  cover each case each one decides, so a fuzzer has nothing to find the tests miss.
  `generation_outcome` reads only the device's own registers, and Kani exhausts it.

## Minimum Rust version

1.88, the manifest's `rust-version`.

## License

MIT OR Apache-2.0, at your option: see `LICENSE-MIT` and `LICENSE-APACHE`.

What the crate takes from Linux, NetBSD, the vendor trees and the JH7110 manual, and why it owes
them no notice, is in [docs/the-crate-carries-facts-not-source-text.md](docs/the-crate-carries-facts-not-source-text.md).
