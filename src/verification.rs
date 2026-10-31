//! Machine-checked proofs, run with `cargo kani`.
//! The unit tests sample `generation_outcome` at chosen bit patterns; `ISTAT` is a register a fault could
//! set to anything, and `generation_outcome` is a few comparisons over two `u32`s, so these exhaust the
//! whole input space instead.

use super::*;

/// Falsification: replayable `falsifications/verification.a_lockup_bit_is_never_overridden.patch`
/// **A hardware fault is never read as a clean answer.** For every `istat` with
/// `LFSR_LOCKUP` set, `generation_outcome` returns `Lockup`, regardless of what `RAND_RDY` or any other
/// bit says and regardless of what the `RAND` words happen to hold. This is the property the
/// module doc's "Health testing" section argues for in prose; here it holds for the full
/// `2^32` space of `istat`, not the two combinations the unit tests above pick.
#[kani::proof]
fn a_lockup_bit_is_never_overridden() {
    let stat: u32 = kani::any();
    let istat: u32 = kani::any();
    kani::assume(istat & ISTAT_LFSR_LOCKUP != 0);
    let rand: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(stat, istat, rand),
        GenerationOutcome::Lockup
    );
}

/// Falsification: replayable `falsifications/verification.ready_requires_rand_rdy_and_carries_the_words_untouched.patch`
/// **`Ready` is returned exactly when the register file says so, and carries exactly those
/// bytes.** No `istat` value produces `Ready` unless `RAND_RDY` is set and `LFSR_LOCKUP` is
/// clear, and when it is, the bytes are `rand`'s little-endian encoding with nothing dropped,
/// substituted, or reordered.
#[kani::proof]
fn ready_requires_rand_rdy_and_carries_the_words_untouched() {
    let stat: u32 = kani::any();
    let istat: u32 = kani::any();
    kani::assume(istat & ISTAT_LFSR_LOCKUP == 0 && istat & ISTAT_RAND_RDY != 0);
    kani::assume(stat & STAT_SEEDED != 0);
    let rand: [u32; 8] = kani::any();
    let GenerationOutcome::Ready(bytes) = generation_outcome(stat, istat, rand) else {
        panic!("RAND_RDY set and LFSR_LOCKUP clear did not read as Ready");
    };
    // **The layout written out, not `output_bytes` compared with itself.**
    // `generation_outcome` calls `output_bytes`, so `assert_eq!(generation_outcome(..), Ready(output_bytes(rand)))`
    // is satisfied by any consistently wrong `output_bytes`: a byte-swap, a word reversal or a
    // silent truncation would leave both sides equal and this harness green, while the doc
    // above claims the bytes are little-endian with nothing reordered. Indexing the words
    // by hand states that claim in terms the implementation cannot pick.
    let mut i = 0;
    while i < 32 {
        assert_eq!(
            bytes[i],
            (rand[i / 4] >> (8 * (i % 4))) as u8,
            "a RAND word reached the caller reordered, truncated or substituted",
        );
        i += 1;
    }
}

/// Falsification: replayable `falsifications/verification.neither_bit_set_is_always_not_ready.patch`
/// **`NotReady` is the only answer when neither bit is set**, whatever the rest of `istat` or
/// the stale `RAND` words say: a device that has not finished is not accidentally read as done
/// or as faulted because some other bit happened to be set.
#[kani::proof]
fn neither_bit_set_is_always_not_ready() {
    let stat: u32 = kani::any();
    let istat: u32 = kani::any();
    kani::assume(istat & ISTAT_LFSR_LOCKUP == 0 && istat & ISTAT_RAND_RDY == 0);
    kani::assume(stat & STAT_SEEDED != 0);
    let rand: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(stat, istat, rand),
        GenerationOutcome::NotReady
    );
}

/// Falsification: replayable `falsifications/verification.an_unseeded_core_never_yields_bytes.patch`
/// **An unseeded core never yields bytes, whatever `ISTAT` claims**. For every
/// `istat` with `LFSR_LOCKUP` clear, if `STAT.SEEDED` is clear then `generation_outcome` returns
/// `Unseeded` rather than `Ready`, so no path exists from a latched `RAND_RDY` on an unseeded
/// device to 32 bytes handed to a caller. This is the property `GenerationOutcome::Unseeded`'s doc
/// argues for in prose, over the full `2^64` space of the two status words.
#[kani::proof]
fn an_unseeded_core_never_yields_bytes() {
    let stat: u32 = kani::any();
    let istat: u32 = kani::any();
    kani::assume(istat & ISTAT_LFSR_LOCKUP == 0);
    kani::assume(stat & STAT_SEEDED == 0);
    let rand: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(stat, istat, rand),
        GenerationOutcome::Unseeded
    );
}
