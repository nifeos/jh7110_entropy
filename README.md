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
use jh7110_entropy::{GenerationOutcome, generation_outcome, is_status_okay, INTERRUPT_STATUS_RANDOM_NUMBER_READY, STATUS_256_BIT_OUTPUT, STATUS_SEEDED};

// Firmware that says nothing about `status` means "okay".
assert!(is_status_okay(None));
assert!(!is_status_okay(Some(b"disabled\0")));

// A seeded core in 256-bit mode with RAND_RDY set: the eight words, little-endian, word 0 first.
let random_numbers = [0x0403_0201, 0, 0, 0, 0, 0, 0, 0];
let status = STATUS_SEEDED | STATUS_256_BIT_OUTPUT;
match generation_outcome(status, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers) {
    GenerationOutcome::Ready(bytes) => assert_eq!(&bytes[..4], &[1, 2, 3, 4]),
    other => panic!("expected Ready, got {other:?}"),
}

// The same snapshot from a core left in 128-bit mode is refused, not served.
assert_eq!(
    generation_outcome(STATUS_SEEDED, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers),
    GenerationOutcome::WrongOutputWidth
);
```

A driver writes `MODE_256_BIT_OUTPUT` during bring-up. Otherwise only four words carry the
answer, and `generation_outcome` answers `WrongOutputWidth` until the driver writes it. The driver
then clears `ISTAT` with `INTERRUPT_STATUS_DOCUMENTED_BITS_MASK`, asks for a reseed with `CONTROL_COMMAND_RESEED`, then for each request
writes `CONTROL_COMMAND_GENERATE`, polls `STAT` and `ISTAT`, and hands them to `generation_outcome`. Once the
outcome is `Ready` and the eight words are read, the driver acknowledges the request by writing
`INTERRUPT_STATUS_RANDOM_NUMBER_READY` (`RAND_RDY`) to `INTERRUPT_STATUS_REGISTER`. The bit is write-1-to-clear, so
without that write it stays latched and the next request sees it on its first poll. On `Lockup` or
`Unseeded` it reseeds and retries, bounded. On `NotReady`, which also covers a generation or a
reseed still in flight, it polls again rather than issuing a command. The crate sets no bound, no poll interval and no
timeout: you pick them, ideally from a measurement on your board. This
README gives no timing figure. Wrap `Pool` and the register access in one lock; `Pool`
does no locking of its own.

## What is proven

Six Kani harnesses exhaust `generation_outcome`, the function that turns one snapshot of the status
register (`STAT`) and the interrupt status register (`ISTAT`), plus the eight random-output words
(`RAND`), into an answer:

- `verification::a_lockup_bit_is_never_overridden`: whenever `ISTAT.LFSR_LOCKUP` (a lockup of the linear-feedback shift register (LFSR) that feeds the
  generator) reads 1, the answer
  is `Lockup`, whatever else the register says.
- `verification::ready_requires_rand_rdy_and_carries_the_words_untouched`: `RAND_RDY` on a seeded,
  idle core in 256-bit mode is `Ready`, and its 32 bytes are the eight words, little-endian, word 0
  first.
- `verification::neither_bit_set_is_always_not_ready`: a seeded core in 256-bit mode with neither
  bit set is `NotReady`.
- `verification::an_unseeded_core_never_yields_bytes`: with `STAT.SEEDED` clear, no `ISTAT` value
  yields bytes.
- `verification::a_128_bit_answer_is_never_served`: with `STAT.R256` clear, no input yields bytes,
  and a seeded, idle core answers `WrongOutputWidth`.
- `verification::work_in_flight_is_always_not_ready`: with a generation or a reseed in flight, the
  answer is `NotReady` unless `LFSR_LOCKUP` is set.

Two more cover `Pool::take`, over every pool state and every request size:

- `verification::take_serves_the_bytes_in_order_and_zeroes_them`: with a generator that answers,
  `take` serves `min(byte_count, 8)` bytes, the pool's own first and the new block's after them, in
  order, and zeroes each byte it served.
- `verification::a_refusing_generator_shortens_the_count`: with a generator that never answers,
  the count is what the pool held, never padded with zeros.

Each harness has a falsification record at
`falsifications/<module.path>.<harness>.patch`; applying it with `git apply -p1` from the crate root
makes that harness fail at its own assertion, which shows the proof checks something. The patches
need a git checkout, and you revert each one with `git checkout -- src` before the next.

Not proven: the quality of the randomness, which is a property of the silicon; that dropping a
`Pool` zeroes its buffer, which safe code cannot observe; and the device-tree decoders, which have
unit tests. `Pool::take` serves at most `WORD_BYTES`, 8 bytes, per call, and its doc comment carries
a runnable example.

## Build, test and prove

You need a Rust toolchain at or above the minimum version at the foot of this page (`rustup` is the
usual way to get one) and a git checkout. From the crate root:

- `cargo build` and `cargo test` run on the host and need nothing else. The tests are 23 unit tests
  and 2 doctests.
- The crate also builds for bare metal. Add the three targets with `rustup target add
  riscv64imac-unknown-none-elf aarch64-unknown-none-softfloat x86_64-unknown-none`. Then run
  `cargo build --lib --target <target>` for each. CI does exactly this.
- The proofs need Kani, which `rustup` does not install. Run `cargo install --locked
  kani-verifier --version 0.68.0`, then `cargo kani setup`. The setup step downloads a model
  checker, so it needs network access. CI uses version 0.68.0, and another version may verify
  differently. After that, `cargo kani` runs all eight harnesses, which takes about a minute on a
  desktop-class machine. To run one, add `--harness <name>`.

## The two device trees

Mainline Linux names the block `starfive,jh7110-trng`. The vendor U-Boot's control tree, which a
VisionFive 2 boots with, names the same block `starfive,trng`, at the same window (`0x1600_C000`,
`0x4000` bytes) and interrupt (30), and marks it `status = "disabled"` because U-Boot has no driver
for it. `COMPATIBLES` lists both, mainline's first. Walk your tree for a node carrying one, read
its `reg`, decode `interrupts` with `interrupt_line` and `status` with `is_status_okay`, and keep the
result in a `Discovered`. A disabled node is the first suspect when the window reads all zeros. The crate has no helper that
walks a tree for you; it decodes the properties and you do the walking with your own parser.

## Hardware evidence

Version 0.2.0 has run on a VisionFive 2 (JH7110): it served real TRNG output on two boots with the
chip in 256-bit mode, the draws differing within each boot and across the boots. The clock and
reset must be ungated and released first, as the first item under BUGS says.

The record below is of a later commit, `ee0bde8`, which a register-level check ran on its own.

Run on 2026-11-09 (UTC), on one VisionFive 2, against commit `ee0bde8feb9f78a67da011c928f9a2a697f3bbc1`
of this repository. A small bare-metal RISC-V payload built on that commit's constants and its
`generation_outcome` performed the register sequences below and printed one checksummed line per
result; a script judged the log. The payload came over the network, and the person running it
powered the board on from off. The payload's SHA-256 is
`e421000bc0b66d35ac60315cbe618ba1c8e15fb46d9bf8aac44f3a27b82054e8`; the console log's is
`540996ad74682bd9fbe350e897028af2310113a9d210df37590847b40a2c960e` and the 1,310,720-byte capture's
is `97fc5325be5ca81d31cfb85b73aaea73191efc474d24d25a0af82f0817079263`. The log holds two boots (the
first fell back to the card's own kernel); the judged run is the second. The checker printed
`PASS, 0 problem(s)`.

What passed, with the board as the judge:

- The window is at `0x1600c000`, and the device tree named it with the `starfive,trng` compatible.
  With the clocks enabled and the reset released, a reseed completed by polling and left
  `STAT.SEEDED` set.
- Two generations by polling both answered `ready`, with 32 bytes that were not all zero and
  differed from each other.
- `STAT.R256` read 1 after the payload wrote `MODE_256_BIT_OUTPUT`. `FEATURES` bits 1:0 and `BCONF` bit 2
  both said the silicon supports 256-bit output.
- No poll timed out in 1,048,576 generations on the driver path. All
  1,048,576 answered `ready`, none was all zero, none came back unseeded or at the wrong width.
- After the bring-up's clear of `ISTAT`, no documented bit remained. Setting `AUTO_RQSTS` or
  `AUTO_AGE` latched `ISTAT` bit 3 or bit 2 as documented, and the residue after clearing held no
  documented bit.

What the board answered, recorded and not judged:

- Reset state. The first read of `STAT` showed `SEEDED` = 1 and `LAST_RESEED` = 4, before the
  payload wrote anything to the block. That is the state as it stood after the bootloader, and it is
  not shown to be the silicon's reset state. `LAST_RESEED` read 0 after the payload's first reseed;
  the values 0 and 4 were the only ones seen in the run.
- Width. `STAT.R256` read 1 and `MODE` read `0x00000008` before the payload wrote `MODE`;
  `FEATURES` read `0x0000000d` and `BCONF` read `0x0000003d`. Read against that reset width, `BCONF`
  bit 3 set means 256-bit. That reading holds only if nothing wrote `MODE` between reset and the
  payload, which the run cannot show.
- `STAT` bits 19 to 23 were never set in any read. A board cannot show where `LAST_RESEED` ends;
  it shows only that the next five bits stayed clear. No bit outside the crate's named fields was
  ever set.
- `RAND_RDY` beside `RAND_GENERATING`. Counts are of snapshots (`STAT`, `ISTAT`, `STAT`) with
  `RAND_RDY` showing. With `RAND_RDY` acknowledged first, 1 of 9,999 snapshots showed
  `RAND_GENERATING` in either `STAT` read and none showed it in both. With a second `GENERATE`
  written over a latched `RAND_RDY`, 8,775 snapshots showed it in both reads and 9,000 in at least
  one, over 1,000 trials; the block accepted all 1,000 commands and changed the `RAND` words each
  time. So the two bits do appear together when a command lands on a latched `RAND_RDY`.
- Timing, at the 4 megahertz time control and status register (CSR): a generation took a median of 24 ticks (6.0 microseconds, maximum
  6.25) and a reseed a median of 174 ticks (43.5 microseconds, maximum 50.75). One read of the time
  CSR costs at least one tick, and every figure includes it.
- `LFSR_LOCKUP` never set in 1,048,576 generations, so the run could not test whether a reseed
  clears it. That stays open.

Statistics, on one 1,310,720-byte capture (40,960 generations, every record's checksum matched): 2 of 524
Federal Information Processing Standard 140-2 blocks failed. A good source fails about one in 1,250, and the check's bar was at most 3.
Shannon entropy was 7.999854 bits per byte, and the chi-square over byte values was 264.17 (upper-tail
p 0.3333). The mean byte was 127.5159, the lag-1 serial correlation -0.000949, and no 32-byte
generation repeated. No outside statistical suite ran.

One capture showing no gross defect says nothing about unpredictability. A stream that looks good
from one boot is also what a weak source produces. A health test and a measurement in the manner of
National Institute of Standards and Technology Special Publication 800-90B are still absent.

Second run, 2026-11-10 (UTC): the same commit and the same payload, on the same board. The person
ran it after the board sat unpowered for about an hour, which is their statement, since nothing
reports the plug state. The payload came over the network on the first boot this time, so the log
holds one boot. The log's SHA-256 is `fde9661f55f814865cb6f843b76f93ff5d3717a1c6290ebbd072db48500c8a92`
and the capture's is `208c42b41ae4118ae000ee2839cc62ecd182e3d4f760b20694395f8b2529637f`. The checker
printed `PASS, 0 problem(s)`.

- Every row that passed in the first run passed again. The first `STAT` read was again `SEEDED` = 1,
  `LAST_RESEED` = 4 and `R256` = 1, with the same `MODE`, `FEATURES` and `BCONF` values. That
  reading repeated across two power-ons. It is still the state the bootloader left, not shown to be
  the silicon's reset state.
- The answers differ from the first run in four places. The reseed median was 172 ticks (43.0
  microseconds). With `RAND_RDY` acknowledged first, no snapshot showed `RAND_GENERATING` at all.
  With a second `GENERATE` over a latched `RAND_RDY`, 8,777 snapshots showed it in both reads and
  9,000 in at least one. The generation bytes and the capture are new.
- Statistics, on a new 1,310,720-byte capture: 1 of 524 Federal Information Processing Standard
  140-2 blocks failed. Shannon entropy was 7.999873 bits per byte, the chi-square 231.12 (upper-tail
  p 0.8560), the mean byte 127.4203, the lag-1 serial correlation -0.001500, and no 32-byte
  generation repeated. The `ent` program printed the same entropy, chi-square and correlation. It
  recomputes those statistics, so it cross-checks the arithmetic and is not an independent suite.

Two runs show that the reading repeats. They remain two measurements. They do not show that the
output is unpredictable.

Not established:

- The quality of the randomness, as above. There is no health test, and the maintainers have not
  decided whether this crate should carry one.
- Any other board, any other firmware, or a reset taken from anything but the two power-ons above.
- The behavior of anything but version 0.2.0 and commit `ee0bde8`. A later commit that changes `src/`
  has not run.

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
