//! Machine-checked proofs, run with `cargo kani`.
//! The unit tests sample `generation_outcome` at chosen bit patterns; `ISTAT` is a register a fault could
//! set to anything, and `generation_outcome` is a few comparisons over two `u32`s, so these exhaust the
//! whole input space instead. The `Pool` harnesses do the same for `take`, over every pool state,
//! every request size and every block a generator can answer with.

use super::*;

/// Falsification: replayable `falsifications/verification.a_lockup_bit_is_never_overridden.patch`
/// **A hardware fault is never read as a clean answer.** For every `interrupt_status` with
/// `LFSR_LOCKUP` set, `generation_outcome` returns `Lockup`, regardless of what `RAND_RDY` or any other
/// bit says and regardless of what the `RAND` words happen to hold. Here the property holds for
/// the full `2^32` space of `interrupt_status`, not the two combinations the unit tests above pick.
#[kani::proof]
fn a_lockup_bit_is_never_overridden() {
    let status: u32 = kani::any();
    let interrupt_status: u32 = kani::any();
    kani::assume(interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP != 0);
    let random_numbers: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(status, interrupt_status, random_numbers),
        GenerationOutcome::Lockup
    );
}

/// Falsification: replayable `falsifications/verification.ready_requires_rand_rdy_and_carries_the_words_untouched.patch`
/// **`Ready` is returned exactly when the register file says so, and carries exactly those
/// bytes.** On a seeded, idle core in 256-bit mode, every `interrupt_status` with `RAND_RDY` set
/// and `LFSR_LOCKUP` clear is `Ready`, and the bytes are `random_numbers`'s little-endian encoding
/// with nothing dropped, substituted, or reordered.
#[kani::proof]
fn ready_requires_rand_rdy_and_carries_the_words_untouched() {
    let status: u32 = kani::any();
    let interrupt_status: u32 = kani::any();
    kani::assume(
        interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP == 0
            && interrupt_status & INTERRUPT_STATUS_RANDOM_NUMBER_READY != 0,
    );
    kani::assume(status & STATUS_SEEDED != 0 && status & STATUS_256_BIT_OUTPUT != 0);
    kani::assume(status & (STATUS_GENERATING | STATUS_RESEEDING) == 0);
    let random_numbers: [u32; 8] = kani::any();
    let GenerationOutcome::Ready(bytes) =
        generation_outcome(status, interrupt_status, random_numbers)
    else {
        panic!("RAND_RDY set and LFSR_LOCKUP clear did not read as Ready");
    };
    // **The layout written out, not `output_bytes` compared with itself.**
    // `generation_outcome` calls `output_bytes`, so `assert_eq!(generation_outcome(..), Ready(output_bytes(random_numbers)))`
    // is satisfied by any consistently wrong `output_bytes`: a byte-swap, a word reversal or a
    // silent truncation would leave both sides equal and this harness green, while the doc
    // above claims the bytes are little-endian with nothing reordered. Indexing the words
    // by hand states that claim in terms the implementation cannot pick.
    let mut i = 0;
    while i < 32 {
        assert_eq!(
            bytes[i],
            (random_numbers[i / 4] >> (8 * (i % 4))) as u8,
            "a RAND word reached the caller reordered, truncated or substituted",
        );
        i += 1;
    }
}

/// Falsification: replayable `falsifications/verification.neither_bit_set_is_always_not_ready.patch`
/// **`NotReady` is the only answer when neither bit is set** on a seeded core in 256-bit mode,
/// whatever the rest of `interrupt_status` or `status` or the stale `RAND` words say: a device
/// that has not finished is not accidentally read as done or as faulted because some other bit
/// happened to be set.
#[kani::proof]
fn neither_bit_set_is_always_not_ready() {
    let status: u32 = kani::any();
    let interrupt_status: u32 = kani::any();
    kani::assume(
        interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP == 0
            && interrupt_status & INTERRUPT_STATUS_RANDOM_NUMBER_READY == 0,
    );
    kani::assume(status & STATUS_SEEDED != 0 && status & STATUS_256_BIT_OUTPUT != 0);
    let random_numbers: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(status, interrupt_status, random_numbers),
        GenerationOutcome::NotReady
    );
}

/// Falsification: replayable `falsifications/verification.an_unseeded_core_never_yields_bytes.patch`
/// **An unseeded core never yields bytes, whatever `ISTAT` claims**. For every
/// `interrupt_status` with `LFSR_LOCKUP` clear and nothing in flight, if `STAT.SEEDED` is clear
/// then `generation_outcome` returns `Unseeded` rather than `Ready`, so no path exists from a
/// latched `RAND_RDY` on an unseeded device to 32 bytes handed to a caller. This is the property `GenerationOutcome::Unseeded`'s doc
/// argues for in prose, over the full `2^64` space of the two status words.
#[kani::proof]
fn an_unseeded_core_never_yields_bytes() {
    let status: u32 = kani::any();
    let interrupt_status: u32 = kani::any();
    kani::assume(interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP == 0);
    kani::assume(status & STATUS_SEEDED == 0);
    kani::assume(status & (STATUS_GENERATING | STATUS_RESEEDING) == 0);
    let random_numbers: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(status, interrupt_status, random_numbers),
        GenerationOutcome::Unseeded
    );
}

/// Falsification: replayable `falsifications/verification.a_128_bit_answer_is_never_served.patch`
/// **A 128-bit answer is never served as 32 bytes.** For every `status` with `STAT.R256` clear and
/// every `interrupt_status`, the answer is not `Ready`; on a seeded, idle core with `LFSR_LOCKUP`
/// clear it is `WrongOutputWidth`, whatever `RAND_RDY` says. In 128-bit mode four of the eight
/// words are not the generation's, so no path exists from that mode to bytes.
#[kani::proof]
fn a_128_bit_answer_is_never_served() {
    let status: u32 = kani::any();
    let interrupt_status: u32 = kani::any();
    kani::assume(status & STATUS_256_BIT_OUTPUT == 0);
    let random_numbers: [u32; 8] = kani::any();
    let outcome = generation_outcome(status, interrupt_status, random_numbers);
    assert!(
        !matches!(outcome, GenerationOutcome::Ready(_)),
        "a 128-bit answer was served as 32 bytes",
    );
    if interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP == 0
        && status & STATUS_SEEDED != 0
        && status & (STATUS_GENERATING | STATUS_RESEEDING) == 0
    {
        assert_eq!(outcome, GenerationOutcome::WrongOutputWidth);
    }
}

/// Falsification: replayable `falsifications/verification.work_in_flight_is_always_not_ready.patch`
/// **A generation or a reseed in flight is `NotReady`**, for every `status` with
/// `RAND_GENERATING` or `RAND_RESEEDING` set and every `interrupt_status` with `LFSR_LOCKUP` clear,
/// `RAND_RDY` included: the words may be mid-rewrite, and no other answer may tell a caller to
/// issue a command while one runs.
#[kani::proof]
fn work_in_flight_is_always_not_ready() {
    let status: u32 = kani::any();
    let interrupt_status: u32 = kani::any();
    kani::assume(status & (STATUS_GENERATING | STATUS_RESEEDING) != 0);
    kani::assume(interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP == 0);
    let random_numbers: [u32; 8] = kani::any();
    assert_eq!(
        generation_outcome(status, interrupt_status, random_numbers),
        GenerationOutcome::NotReady
    );
}

/// A `Pool` in any state `new` and `refill` can leave it in: empty, or holding one block with a
/// cursor anywhere from its start to its end, over any 32 bytes.
fn any_pool() -> Pool {
    let filled = if kani::any() { 32 } else { 0 };
    let cursor: usize = kani::any();
    kani::assume(cursor <= filled);
    Pool {
        buffer: kani::any(),
        cursor,
        filled,
    }
}

/// Falsification: replayable `falsifications/verification.take_serves_the_bytes_in_order_and_zeroes_them.patch`
/// **`take` serves exactly what was asked, in order, across a refill seam, and zeroes what it
/// served.** For any pool state, any `byte_count` and a generator that answers with any block:
/// the count is `min(byte_count, 8)`, the word holds the pool's unserved bytes followed by the
/// block's first bytes, little-endian, nothing above the count is set, and every byte served is
/// zero in the buffer afterwards. Kani also checks that no step panics or overflows.
#[kani::proof]
#[kani::unwind(10)]
fn take_serves_the_bytes_in_order_and_zeroes_them() {
    let mut pool = any_pool();
    let before = pool.buffer;
    let cursor = pool.cursor;
    let remaining = pool.remaining_bytes();
    let block: [u8; 32] = kani::any();
    let mut calls = 0u32;
    let mut generate = || {
        calls += 1;
        Some(block)
    };
    let byte_count: u64 = kani::any();
    let taken = pool.take(byte_count, &mut generate);
    let wanted = byte_count.min(WORD_BYTES) as usize;
    assert_eq!(
        taken.byte_count, wanted as u64,
        "a short count from a generator that answers"
    );
    assert!(
        calls <= 1,
        "one block always covers a word, so one refill is enough"
    );
    let bytes = taken.word.to_le_bytes();
    let mut i = 0;
    while i < 8 {
        let expected = if i >= wanted {
            0
        } else if i < remaining {
            before[cursor + i]
        } else {
            block[i - remaining]
        };
        assert_eq!(
            bytes[i], expected,
            "a byte reached the caller reordered or substituted"
        );
        i += 1;
    }
    // What was served is zero now: the old block's tail when no refill happened, else the head
    // of the new block, which the refill wrote over the old one.
    let (first, served_here) = if wanted <= remaining {
        (cursor, wanted)
    } else {
        (0, wanted - remaining)
    };
    let mut j = 0;
    while j < served_here {
        assert_eq!(
            pool.buffer[first + j],
            0,
            "a served byte is still in the buffer"
        );
        j += 1;
    }
}

/// Falsification: replayable `falsifications/verification.a_refusing_generator_shortens_the_count.patch`
/// **A generator that refuses shortens the count rather than padding it.** For any pool state and
/// any `byte_count`, with a generator that never answers, `take` serves `min(byte_count, 8,
/// remaining)` bytes, says so, and leaves every byte of the word above that count zero: four
/// bytes asked of a pool holding two come back as a count of two, never as four with two zeros.
#[kani::proof]
#[kani::unwind(10)]
fn a_refusing_generator_shortens_the_count() {
    let mut pool = any_pool();
    let remaining = pool.remaining_bytes() as u64;
    let byte_count: u64 = kani::any();
    let taken = pool.take(byte_count, &mut || None);
    let expected = byte_count.min(WORD_BYTES).min(remaining);
    assert_eq!(
        taken.byte_count, expected,
        "the count is not what the pool could serve"
    );
    assert_eq!(pool.remaining_bytes() as u64, remaining - expected);
    if expected < WORD_BYTES {
        assert_eq!(
            taken.word >> (8 * expected),
            0,
            "the word carries bytes above its count"
        );
    }
}
