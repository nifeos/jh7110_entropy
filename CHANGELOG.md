# Changelog

One section per release, newest first: what changed and why, so a reader of the diff has the
sentence beside it.

## Unreleased

- Breaking: `GenerationOutcome` has a fourth refusing variant, `WrongOutputWidth`, which
  `generation_outcome` answers when `STAT.R256` (`STATUS_256_BIT_OUTPUT`) is clear. In 128-bit mode
  only four of the eight `RAND` words carry the answer, and the decode used to serve all eight.
- `generation_outcome` answers `NotReady` while `STAT` reports a generation (`STATUS_GENERATING`) or a
  reseed (`STATUS_RESEEDING`) in flight, because the `RAND` words may be mid-rewrite and the other
  refusals would tell a caller to issue a command while one runs.
- Breaking: `Pool` is no longer `Clone` or `Copy`, since a copy serves the same bytes twice. Its
  `Debug` prints `remaining_bytes` and never the buffer. Dropping it zeroes the buffer, and
  `refill` zeroes the bytes it discards.
- Breaking: `Pool::remaining` is `Pool::remaining_bytes`, `Pool::take`'s parameter is
  `byte_count`, and `take` answers a `TakenBytes { byte_count, word }` in place of a `(u64, u64)`
  whose order a caller had to remember.
- Kani proves the two new decode rules and `Pool::take`: the count it serves, the order of the
  bytes across a refill, the zeroing behind the cursor, and the short count from a generator that
  refuses. Each new harness carries a falsification patch.
- The minimum Rust version is 1.85, the oldest toolchain the source builds and tests on, in place of
  1.88.

## 0.2.0

- Breaking: the crate now spells out every register offset, command value and bit constant. The
  offsets are `CONTROL_REGISTER`, `STATUS_REGISTER`, `MODE_REGISTER`, `STATUS_MODE_REGISTER`,
  `INTERRUPT_ENABLE_REGISTER`, `INTERRUPT_STATUS_REGISTER`, `FEATURES_REGISTER`,
  `RANDOM_NUMBER_0_REGISTER` to `RANDOM_NUMBER_7_REGISTER`, `SEED_0_REGISTER`, `SEED_7_REGISTER`,
  `AUTOMATIC_RESEED_REQUESTS_REGISTER`, `AUTOMATIC_RESEED_AGE_REGISTER` and
  `BUILD_CONFIGURATION_REGISTER`. The command values start with `CONTROL_COMMAND_`. The bit
  constants start with `MODE_`, `STATUS_`, `INTERRUPT_ENABLE_` or `INTERRUPT_STATUS_`, and
  `INTERRUPT_STATUS_DOCUMENTED_BITS_MASK` is the mask of every documented interrupt status bit.
- Each renamed item keeps the datasheet and driver spellings as `#[doc(alias)]`, so a search for
  `ISTAT`, `JH7110_TRNG_ISTATUS` or `RAND_RDY` still lands on it.
- Breaking: the `Discovered` fields are `registers_physical_address`, `registers_bytes` and
  `interrupt_line`, aliased `reg` and `interrupts` after the device-tree properties they hold.
- The parameters of `generation_outcome` are `status`, `interrupt_status` and `random_numbers`.
- The falsification patches follow the new names and still make each harness fail.

## 0.1.1

- The documentation no longer refers to any consumer of the crate.

## 0.1.0

- First version.
- Doc comments state the vendor tree's true random number generator (TRNG) node and five manual register descriptions in the
  crate's own words, so the crate copies no source text.
- Doc comments no longer cite a "Health testing" section that does not exist. `ISTAT_LFSR_LOCKUP` says
  the crate has no health test and that the question is open. The crate-level doc keeps the
  current status and leaves the dated history to the README.

## BUGS

- The template's lag check guards the CI pin, not this file; a change that matters to a user of
  the crate belongs in a release line here.
