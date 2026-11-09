#![cfg_attr(not(test), no_std)]
//! The `StarFive` JH7110 true random number generator (TRNG), as pure logic: the register layout,
//! what each of the chip's two device trees calls the block, and the decision of whether a
//! generation succeeded, failed or is running.
//!
//! It never dereferences a pointer, so it is host-testable and Kani-reachable; the caller maps the
//! register window and does the volatile reads. `ISTAT.RAND_RDY` counts only when `STAT.SEEDED` is
//! set (NetBSD's rule), and `ISTAT.LFSR_LOCKUP`, the one hardware fault signal, is
//! [`GenerationOutcome::Lockup`]. The README's "Hardware evidence" section says which commit ran on a
//! board and what it established; the quality of the randomness is unmeasured.

//! # Sources
//!
//! The bracketed labels in the documentation below name these:
//!
//! - \[binding\]: Linux's `starfive,jh7110-trng.yaml`, mainline as of 2026-08-24: the compatible
//!   string, the `reg` window, two clocks, one reset and one interrupt.
//! - \[driver\]: Linux's `drivers/char/hw_random/jh7110-trng.c`, mainline as of 2026-08-24. The
//!   register offsets and bit positions are transcribed from its `#define`s.
//! - \[trm\]: the *JH7110 Technical Reference Manual*, Preliminary V2 (2023-04-24), "TRNG > Control
//!   Registers". It settles that `ISTAT` is write-1-to-clear and that zero disables `AUTO_RQSTS`
//!   and `AUTO_AGE`, and names two `ISTAT` bits no Linux driver defines.
//! - \[netbsd\]: NetBSD's `sys/arch/riscv/starfive/jh7110_trng.c`, revision 1.2: the only driver
//!   that polls, and the source of the interrupt bit positions mainline omits.

//! # The two device trees
//!
//! Mainline Linux describes the block as `compatible = "starfive,jh7110-trng"` ([`COMPATIBLE_MAINLINE`]).
//! The vendor U-Boot's control tree, which a `VisionFive 2` boots with, spells the same block
//! `"starfive,trng"` ([`COMPATIBLE_VENDOR`]) at the same window (`0x1600_C000`, `0x4000` bytes)
//! with the same interrupt (30), and marks it `status = "disabled"` because U-Boot has no driver
//! for it. [`COMPATIBLES`] lists both in the order to try them. A caller walks its own tree for a
//! node carrying one of them, decodes `interrupts` with [`interrupt_line`] and `status` with
//! [`is_status_okay`], and fills in a [`Discovered`].

//! # Examples
//!
//! ```
//! use jh7110_entropy::{GenerationOutcome, generation_outcome, INTERRUPT_STATUS_RANDOM_NUMBER_READY, INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP, STATUS_256_BIT_OUTPUT, STATUS_GENERATING, STATUS_SEEDED};
//!
//! // A seeded core in 256-bit mode, the state a bring-up leaves it in.
//! let idle = STATUS_SEEDED | STATUS_256_BIT_OUTPUT;
//!
//! // Seeded, but the generation has not finished.
//! assert_eq!(generation_outcome(idle, 0, [0; 8]), GenerationOutcome::NotReady);
//!
//! // The eight output words assemble little-endian, word 0 first.
//! let random_numbers = [0x0403_0201, 0x0807_0605, 0, 0, 0, 0, 0, 0];
//! match generation_outcome(idle, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers) {
//!     GenerationOutcome::Ready(bytes) => assert_eq!(&bytes[..8], &[1, 2, 3, 4, 5, 6, 7, 8]),
//!     other => panic!("expected Ready, got {other:?}"),
//! }
//!
//! // A latched RAND_RDY on a core never seeded is not an answer, and a fault beats a stale one.
//! assert_eq!(generation_outcome(0, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers), GenerationOutcome::Unseeded);
//! let both = INTERRUPT_STATUS_RANDOM_NUMBER_READY | INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP;
//! assert_eq!(generation_outcome(idle, both, [0; 8]), GenerationOutcome::Lockup);
//!
//! // A 128-bit answer fills only four words, so it is refused rather than padded to 32 bytes.
//! let narrow = STATUS_SEEDED;
//! assert_eq!(generation_outcome(narrow, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers), GenerationOutcome::WrongOutputWidth);
//!
//! // While a generation is still in flight the words may be mid-rewrite, whatever ISTAT says.
//! let busy = idle | STATUS_GENERATING;
//! assert_eq!(generation_outcome(busy, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers), GenerationOutcome::NotReady);
//! ```

/// Register byte offsets from the device's base, transcribed from `jh7110-trng.c`'s `#define`s
/// (\[driver\]). `RAND0..RAND7` (here `RANDOM_NUMBER_0_REGISTER..RANDOM_NUMBER_7_REGISTER`) are the eight 32-bit words a completed generation leaves behind;
/// [`output_bytes`] is the order they combine in.
pub mod registers {
    /// Command register: write [`super::CONTROL_COMMAND_GENERATE`] or [`super::CONTROL_COMMAND_RESEED`].
    #[doc(alias = "JH7110_TRNG_CTRL")]
    #[doc(alias = "CTRL")]
    pub const CONTROL_REGISTER: u64 = 0x00;
    /// Status register: read-only, [`super::STATUS_SEEDED`] / `RAND_GENERATING` /
    /// [`super::STATUS_RESEEDING`].
    #[doc(alias = "STAT")]
    pub const STATUS_REGISTER: u64 = 0x04;
    /// Output width mode: [`super::MODE_256_BIT_OUTPUT`] selects a 256-bit rather than a 128-bit `RAND`
    /// answer.
    ///
    /// **A driver that never writes this register does not get 32 usable bytes.** The width after
    /// reset is a build-time parameter (\[trm\]'s `FEATURES.MAX_RAND_LENGTH` and
    /// `BUILD_CONFIG.PRNG_LEN_AFTER_RST`), and in 128-bit mode only `RAND0..RAND3` carry the
    /// answer, which is why \[driver\] clamps its read to four words without `R256`.
    /// `STAT.R256` ([`super::STATUS_256_BIT_OUTPUT`]) reports the current width, and all three cited drivers
    /// write `MODE` during bring-up.
    #[doc(alias = "MODE")]
    pub const MODE_REGISTER: u64 = 0x08;
    /// Status-mode register: `MISSION_MODE`, `NONCE_MODE` and `MAX_REJECTS` per \[trm\]. None of
    /// the three drivers cited here writes a value of its own into it (mainline leaves it alone
    /// entirely and the vendor driver writes back what it read), so this crate names the offset
    /// and [`super::STATUS_MODE_MISSION_MODE`] without proposing a write.
    #[doc(alias = "SMODE")]
    pub const STATUS_MODE_REGISTER: u64 = 0x0C;
    /// Interrupt enable: the four `super::INTERRUPT_ENABLE_*` contribution bits plus [`super::INTERRUPT_ENABLE_GLOBAL`].
    /// **Enabling a contribution here is about the interrupt request (IRQ) pin, not about whether [`INTERRUPT_STATUS_REGISTER`] latches**;
    /// see [`super::INTERRUPT_STATUS_DOCUMENTED_BITS_MASK`].
    #[doc(alias = "JH7110_TRNG_IENABLE")]
    #[doc(alias = "IE")]
    pub const INTERRUPT_ENABLE_REGISTER: u64 = 0x10;
    /// Interrupt status, and **\[trm\]'s register map marks it `R/W1C`**: a bit is acknowledged by
    /// writing a one to it, as \[driver\] and \[netbsd\] both do. A driver that only ever reads
    /// this register sees `RAND_RDY` stay set forever after the first generation.
    #[doc(alias = "JH7110_TRNG_ISTATUS")]
    #[doc(alias = "ISTAT")]
    pub const INTERRUPT_STATUS_REGISTER: u64 = 0x14;
    /// Read-only fields fixed when the block was built: `MISSION_MODE_RESET_STATE`,
    /// `RAND_SEED_AVAIL` and `MAX_RAND_LENGTH` \[trm\], decoded by the `super::FEATURES_*`
    /// constants. Named so a bench session can dump and decode it; nothing here reads it.
    #[doc(alias = "FEATURES")]
    pub const FEATURES_REGISTER: u64 = 0x1C;
    /// The eight output words. Populated once `ISTAT.RAND_RDY` is set. See [`RANDOM_NUMBER_1_REGISTER`]..[`RANDOM_NUMBER_7_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA0")]
    #[doc(alias = "RAND0")]
    pub const RANDOM_NUMBER_0_REGISTER: u64 = 0x20;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA1")]
    #[doc(alias = "RAND1")]
    pub const RANDOM_NUMBER_1_REGISTER: u64 = 0x24;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA2")]
    #[doc(alias = "RAND2")]
    pub const RANDOM_NUMBER_2_REGISTER: u64 = 0x28;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA3")]
    #[doc(alias = "RAND3")]
    pub const RANDOM_NUMBER_3_REGISTER: u64 = 0x2C;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA4")]
    #[doc(alias = "RAND4")]
    pub const RANDOM_NUMBER_4_REGISTER: u64 = 0x30;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA5")]
    #[doc(alias = "RAND5")]
    pub const RANDOM_NUMBER_5_REGISTER: u64 = 0x34;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA6")]
    #[doc(alias = "RAND6")]
    pub const RANDOM_NUMBER_6_REGISTER: u64 = 0x38;
    /// See [`RANDOM_NUMBER_0_REGISTER`].
    #[doc(alias = "JH7110_TRNG_DATA7")]
    #[doc(alias = "RAND7")]
    pub const RANDOM_NUMBER_7_REGISTER: u64 = 0x3C;
    /// The eight seed words. The host writes a seed of its own choosing here for a nonce reseed,
    /// and \[trm\] says the test modes also read internal state through them. This crate never
    /// seeds by nonce (it asks the ring oscillator for a random reseed instead), so these are
    /// offsets only.
    #[doc(alias = "SEED0")]
    pub const SEED_0_REGISTER: u64 = 0x40;
    /// See [`SEED_0_REGISTER`].
    #[doc(alias = "SEED1")]
    pub const SEED_1_REGISTER: u64 = 0x44;
    /// See [`SEED_0_REGISTER`].
    #[doc(alias = "SEED2")]
    pub const SEED_2_REGISTER: u64 = 0x48;
    /// See [`SEED_0_REGISTER`].
    #[doc(alias = "SEED3")]
    pub const SEED_3_REGISTER: u64 = 0x4C;
    /// See [`SEED_0_REGISTER`].
    #[doc(alias = "SEED4")]
    pub const SEED_4_REGISTER: u64 = 0x50;
    /// See [`SEED_0_REGISTER`].
    #[doc(alias = "SEED5")]
    pub const SEED_5_REGISTER: u64 = 0x54;
    /// See [`SEED_0_REGISTER`].
    #[doc(alias = "SEED6")]
    pub const SEED_6_REGISTER: u64 = 0x58;
    /// See [`SEED_0_REGISTER`]. The set runs `SEED0`..`SEED7` at `0x40`..`0x5C`.
    #[doc(alias = "SEED7")]
    pub const SEED_7_REGISTER: u64 = 0x5C;
    /// Auto-reseed request-count threshold, and **zero is what disables it**: per \[trm\], a
    /// `RQSTS` field of 0 turns the `AUTO_RQSTS` alarm off, and any other value is what the
    /// block's internal countdown starts from. \[driver\] writes its `autoreq` module parameter
    /// here during init, which defaults to 0, so upstream's default bring-up disables the alarm
    /// rather than leaving whatever the reset value is in place.
    #[doc(alias = "AUTO_RQSTS")]
    pub const AUTOMATIC_RESEED_REQUESTS_REGISTER: u64 = 0x60;
    /// Auto-reseed age (time) threshold. Same shape as [`AUTOMATIC_RESEED_REQUESTS_REGISTER`]: 0 disables the `AUTO_AGE`
    /// alarm, and \[driver\] writes 0 there at init via its `autoage` parameter.
    #[doc(alias = "AUTO_AGE")]
    pub const AUTOMATIC_RESEED_AGE_REGISTER: u64 = 0x64;
    /// Read-only fields describing how the block was configured when built \[trm\] (the register
    /// \[trm\] itself calls `BUILD_CONFIG`), decoded by the `super::BUILD_CONFIGURATION_*`
    /// constants. The highest offset the register map defines, which is
    /// why one 4 KiB page covers this device with room to spare.
    #[doc(alias = "JH7110_TRNG_BCONF")]
    #[doc(alias = "BUILD_CONFIG")]
    #[doc(alias = "BUILD_CONFIGURATION")]
    pub const BUILD_CONFIGURATION_REGISTER: u64 = 0x68;
}

/// `CTRL`: execute a no-operation (NOP) command (`EXEC_NOP` in the vendor driver). \[trm\] lists it as one of the command
/// encodings; the vendor driver issues it once between programming the mode registers and asking
/// for a reseed. Named for completeness rather than used, since \[driver\] and \[netbsd\] both
/// skip it.
#[doc(alias = "JH7110_TRNG_CTRL_NOP")]
#[doc(alias = "CTRL_EXEC_NOP")]
pub const CONTROL_COMMAND_NONE: u32 = 0x0;
/// `CTRL`: ask the device for a fresh random number (`GENE_RANDNUM` in the vendor driver),
/// 256-bit or 128-bit according to [`MODE_256_BIT_OUTPUT`].
#[doc(alias = "JH7110_TRNG_CTRL_RANDOMIZE")]
#[doc(alias = "CTRL_GENE_RANDNUM")]
pub const CONTROL_COMMAND_GENERATE: u32 = 0x1;
/// `CTRL`: force a reseed from the ring oscillator (`EXEC_RANDRESEED` in the vendor driver). The
/// driver issues this once at init and again on every [`GenerationOutcome::Lockup`].
#[doc(alias = "JH7110_TRNG_CTRL_RANDOM_RESEED")]
#[doc(alias = "CTRL_EXEC_RANDRESEED")]
pub const CONTROL_COMMAND_RESEED: u32 = 0x2;
/// `CTRL`: reseed from the host-written `SEED0..SEED7` words instead of from the ring oscillator
/// \[trm\] (`EXEC_NONCE_RESEED` in the vendor driver). **Named so it is visibly not used**: a
/// nonce reseed makes the output a function of bytes the host chose, which is the opposite of what
/// an entropy source is for, and nothing in this tree should reach for it.
#[doc(alias = "JH7110_TRNG_CTRL_NONCE_RESEED")]
#[doc(alias = "CTRL_EXEC_NONCE_RESEED")]
pub const CONTROL_COMMAND_NONCE_RESEED: u32 = 0x3;

/// `MODE` bit 3: select a 256-bit `RAND0..RAND7` answer rather than a 128-bit `RAND0..RAND3` one.
///
/// **A bring-up that does not set this may be assembling 32 bytes out of a 16-byte answer.** The
/// width after reset is a build-time parameter of the silicon (\[trm\]'s
/// `BUILD_CONFIG.PRNG_LEN_AFTER_RST`), so it cannot be assumed either way from documentation;
/// [`STATUS_256_BIT_OUTPUT`] is how a driver finds out what it actually got.
#[doc(alias = "R256")]
pub const MODE_256_BIT_OUTPUT: u32 = 1 << 3;

/// `SMODE` bit 8: mission mode (1) rather than test mode (0). In test mode \[trm\] lets the host
/// see the generator's internal state, which is not a mode an entropy source should serve
/// from. None of the three drivers cited writes this bit, so this crate names it and leaves the
/// reset value alone; [`STATUS_MISSION_MODE`] reports which mode the block is in.
#[doc(alias = "MISSION_MODE")]
pub const STATUS_MODE_MISSION_MODE: u32 = 1 << 8;

/// `STAT` bit 2: nonce mode is enabled, reflecting `SMODE.NONCE_MODE`.
#[doc(alias = "NONCE_MODE")]
pub const STATUS_NONCE_MODE: u32 = 1 << 2;
/// `STAT` bit 3: the `RAND` register set is currently 256 bits wide, reflecting [`MODE_256_BIT_OUTPUT`].
/// [`generation_outcome`] answers [`GenerationOutcome::WrongOutputWidth`] while it is clear.
#[doc(alias = "R256")]
pub const STATUS_256_BIT_OUTPUT: u32 = 1 << 3;
/// `STAT` bit 8: the block is in mission mode, reflecting [`STATUS_MODE_MISSION_MODE`].
#[doc(alias = "MISSION_MODE")]
pub const STATUS_MISSION_MODE: u32 = 1 << 8;
/// `STAT` bit 9: the device has been seeded at least once and may be asked to generate.
#[doc(alias = "SEEDED")]
pub const STATUS_SEEDED: u32 = 1 << 9;
/// `STAT` bit 27: there is an unacknowledged service request \[trm\].
#[doc(alias = "SRVC_RQST")]
pub const STATUS_SERVICE_REQUEST: u32 = 1 << 27;
/// `STAT` bit 30: a generation is in flight. The driver's `wait_idle` polls this (and
/// [`STATUS_RESEEDING`]) clear before issuing a new command, so two commands are never in flight at
/// once. [`generation_outcome`] answers [`GenerationOutcome::NotReady`] while it is set, because
/// the `RAND` words may be mid-rewrite.
#[doc(alias = "RAND_GENERATING")]
pub const STATUS_GENERATING: u32 = 1 << 30;
/// `STAT` bit 31, `RAND_RESEEDING` in \[trm\] (`RAND_SEEDING` in the vendor driver): a reseed is
/// in progress. This is not the opposite of [`STATUS_SEEDED`], which records only that the device
/// has been seeded at least once. [`generation_outcome`] answers [`GenerationOutcome::NotReady`]
/// while it is set.
#[doc(alias = "RAND_RESEEDING")]
#[doc(alias = "STAT_RAND_SEEDING")]
pub const STATUS_RESEEDING: u32 = 1 << 31;

/// `IE` bit 0: include `RAND_RDY` in the interrupt the block drives.
#[doc(alias = "RAND_RDY_EN")]
pub const INTERRUPT_ENABLE_RANDOM_NUMBER_READY: u32 = 1 << 0;
/// `IE` bit 1: include `SEED_DONE`.
#[doc(alias = "SEED_DONE_EN")]
pub const INTERRUPT_ENABLE_SEED_DONE: u32 = 1 << 1;
/// `IE` bit 2: include `AGE_ALARM`.
#[doc(alias = "AGE_ALARM_EN")]
pub const INTERRUPT_ENABLE_AGE_ALARM: u32 = 1 << 2;
/// `IE` bit 3: include `RQST_ALARM`.
#[doc(alias = "RQST_ALARM_EN")]
pub const INTERRUPT_ENABLE_REQUEST_ALARM: u32 = 1 << 3;
/// `IE` bit 4: include `LFSR_LOCKUP`.
#[doc(alias = "LFSR_LOCKUP_EN")]
pub const INTERRUPT_ENABLE_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP: u32 = 1 << 4;
/// `IE` bit 31: the global interrupt enable.
///
/// **The `IE_*` bits gate the IRQ pin, not [`registers::INTERRUPT_STATUS_REGISTER`]**, so a polling driver may leave this
/// register at zero. \[trm\]'s wording ("interrupt **and/or status** contributions") is not
/// decisive; a reseed and a generation both showed up by polling `ISTAT`
/// with `IE` never written, on a `VisionFive 2` (the README's "Hardware evidence" section says what ran). The completion interrupt needs this
/// bit plus the contribution bits, at Platform-Level Interrupt Controller (PLIC) line 30 (\[binding\]'s `interrupts = <30>`); a polling
/// driver needs none of them.
#[doc(alias = "GLBL_EN")]
pub const INTERRUPT_ENABLE_GLOBAL: u32 = 1 << 31;

/// `ISTAT` bit 0: a generation completed and `RAND0..RAND7` hold a fresh answer.
#[doc(alias = "RAND_RDY")]
pub const INTERRUPT_STATUS_RANDOM_NUMBER_READY: u32 = 1 << 0;
/// `ISTAT` bit 1: a reseed completed.
#[doc(alias = "SEED_DONE")]
pub const INTERRUPT_STATUS_SEED_DONE: u32 = 1 << 1;
/// `ISTAT` bit 2: the auto-reseed **age** alarm, a reminder that `AUTO_AGE` counted down to zero
/// and the seed is stale \[trm\], \[netbsd\].
///
/// Mainline's `jh7110-trng.c` does not define it, and a bit with no name leaves a status word
/// undecodable. [`generation_outcome`] does not act on it, and a bring-up that writes 0 to
/// [`registers::AUTOMATIC_RESEED_AGE_REGISTER`] never raises it.
#[doc(alias = "AGE_ALARM")]
#[doc(alias = "JH7110_TRNG_ISTATUS_AGE_ALARM")]
pub const INTERRUPT_STATUS_AGE_ALARM: u32 = 1 << 2;
/// `ISTAT` bit 3: the auto-reseed **request-count** alarm, the `AUTO_RQSTS` counterpart of
/// [`INTERRUPT_STATUS_AGE_ALARM`] \[trm\], \[netbsd\] (which calls it `RQST_LOCKUP`). Same status:
/// named, not acted on, and disabled by a bring-up that writes 0 to
/// [`registers::AUTOMATIC_RESEED_REQUESTS_REGISTER`].
#[doc(alias = "RQST_ALARM")]
#[doc(alias = "JH7110_TRNG_ISTATUS_RQST_LOCKUP")]
pub const INTERRUPT_STATUS_REQUEST_ALARM: u32 = 1 << 3;
/// `ISTAT` bit 4: the hardware's own fault signal, an SEU (single-event upset) in the LFSR-based
/// post-processing stage. The Linux driver treats this as "reseed and try again", never as "trust
/// the `RAND` registers anyway"; [`generation_outcome`] gives it priority over `RAND_RDY` in the same
/// snapshot for the same reason (a register file that says both "ready" and "faulted" at once
/// must not be read as an answer). This is the one hardware self-check the datasheet documents. It
/// is not a statistical health test over the bitstream, which the datasheet does not claim either.
/// This crate has no such test, and whether it needs one is open and not decided in this repository.
#[doc(alias = "LFSR_LOCKUP")]
pub const INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP: u32 = 1 << 4;

/// Every `ISTAT` bit this crate can name, for the write-1-to-clear that starts a bring-up.
///
/// The init clear discards whatever the block latched before this kernel ran (U-Boot, a previous
/// boot, a stale `RAND_RDY`). \[netbsd\] writes `~0`; this names the five documented bits instead,
/// so the constant says what it acknowledges and an undocumented bit shows up as a residue a
/// driver can report.
pub const INTERRUPT_STATUS_DOCUMENTED_BITS_MASK: u32 = INTERRUPT_STATUS_RANDOM_NUMBER_READY
    | INTERRUPT_STATUS_SEED_DONE
    | INTERRUPT_STATUS_AGE_ALARM
    | INTERRUPT_STATUS_REQUEST_ALARM
    | INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP;

/// `STAT` bits 16 to 18, `LAST_RESEED`: why the last reseed happened, or [`STATUS_LAST_RESEED_UNSEEDED`].
/// Shift a masked `STAT` right by [`STATUS_LAST_RESEED_SHIFT`] to read the field's value.
///
/// \[driver\] places the field at bit 16; its width, three bits, is what holds the value 7 that
/// \[trm\] gives the unseeded state. Neither cited driver reads it, so it is decoded for a bench
/// dump and nothing here acts on it.
#[doc(alias = "LAST_RESEED")]
pub const STATUS_LAST_RESEED_MASK: u32 = 0b111 << STATUS_LAST_RESEED_SHIFT;
/// Where `STAT.LAST_RESEED` starts: see [`STATUS_LAST_RESEED_MASK`].
pub const STATUS_LAST_RESEED_SHIFT: u32 = 16;
/// The `STAT.LAST_RESEED` value \[trm\] gives as "unseeded (zeroized state)": the field reads 7
/// on a core that has not been seeded since it was zeroized.
pub const STATUS_LAST_RESEED_UNSEEDED: u32 = 0b111;

/// `FEATURES` bit 3, `MISSION_MODE_RESET_STATE` in \[trm\]: the block leaves reset in mission mode
/// (1) rather than test mode (0) \[netbsd\]. See [`STATUS_MISSION_MODE`] for the mode it is in now.
#[doc(alias = "MM_RESET_STATE")]
#[doc(alias = "MISSION_MODE_RESET_STATE")]
#[doc(alias = "JH7110_TRNG_FEATURES_MM_RESET_STATE")]
pub const FEATURES_MISSION_MODE_RESET_STATE: u32 = 1 << 3;
/// `FEATURES` bit 2: the block was built with its ring-oscillator entropy source present (1)
/// \[netbsd\].
#[doc(alias = "RAND_SEED_AVAIL")]
#[doc(alias = "JH7110_TRNG_FEATURES_RAND_SEED_AVAIL")]
pub const FEATURES_RANDOM_SEED_AVAILABLE: u32 = 1 << 2;
/// `FEATURES` bits 0 and 1: the widest answer the block was built to give, 128 bits (0) or 256
/// bits ([`FEATURES_WIDEST_OUTPUT_256_BIT`]) \[netbsd\]. A block built for 128 bits
/// cannot give the 32 bytes a [`GenerationOutcome::Ready`] carries, whatever a driver writes to
/// [`registers::MODE_REGISTER`].
#[doc(alias = "MAX_RAND_LENGTH")]
#[doc(alias = "JH7110_TRNG_FEATURES_MAX_RAND_LENGTH")]
pub const FEATURES_WIDEST_OUTPUT_MASK: u32 = 0b11;
/// The [`FEATURES_WIDEST_OUTPUT_MASK`] value for a block built to give 256 bits.
pub const FEATURES_WIDEST_OUTPUT_256_BIT: u32 = 0b01;

/// `BCONF` bit 5: the block was built with the automatic-reseed loopback present (1) \[netbsd\].
#[doc(alias = "AUTO_RESEED_LOOPBACK")]
#[doc(alias = "JH7110_TRNG_BCONF_AUTO_RESEED_LOOPBACK")]
pub const BUILD_CONFIGURATION_AUTOMATIC_RESEED_LOOPBACK: u32 = 1 << 5;
/// `BCONF` bit 4: the block leaves reset in mission mode (1) rather than test mode (0)
/// \[netbsd\].
#[doc(alias = "MODE_AFTER_RST")]
#[doc(alias = "JH7110_TRNG_BCONF_MODE_AFTER_RST")]
pub const BUILD_CONFIGURATION_MISSION_MODE_AFTER_RESET: u32 = 1 << 4;
/// `BCONF` bit 3, `PRNG_LEN_AFTER_RST`: the output width the block leaves reset with \[trm\],
/// which is why a bring-up writes [`MODE_256_BIT_OUTPUT`] rather than assuming it. Which value
/// means 256 bits is not stated in any source cited here (\[netbsd\]'s description of this bit
/// reads "not present" and "present"), so a bench dump settles it against [`STATUS_256_BIT_OUTPUT`]
/// read before the mode write.
#[doc(alias = "PRNG_LEN_AFTER_RST")]
#[doc(alias = "JH7110_TRNG_BCONF_PRNG_LEN_AFTER_RST")]
pub const BUILD_CONFIGURATION_OUTPUT_WIDTH_AFTER_RESET: u32 = 1 << 3;
/// `BCONF` bit 2: the widest answer the block was built to give, 128 bits (0) or 256 bits (1)
/// \[netbsd\].
#[doc(alias = "MAX_PRNG_LEN")]
#[doc(alias = "JH7110_TRNG_BCONF_MAX_PRNG_LEN")]
pub const BUILD_CONFIGURATION_WIDEST_OUTPUT_256_BIT: u32 = 1 << 2;

/// The device tree `compatible` string \[binding\] defines. `starfive,jh8100-trng` also lists this
/// as a fallback compatible (the `JH8100` reuses the same TRNG intellectual property (IP) block), which is out of scope: this
/// crate names the JH7110's own string only, never the fallback, so it says nothing about a `SoC`
/// it has no board for.
pub const COMPATIBLE_MAINLINE: &[u8] = b"starfive,jh7110-trng";

/// **The vendor U-Boot's spelling for the same device**, which the `VisionFive 2`'s control tree
/// uses.
///
/// Same window (`0x1600_C000`), interrupt and clocks, a different string: a stale fork, not
/// different silicon. Accepting it claims the [`registers`] layout, and `StarFive`'s own Linux driver
/// for the block (`drivers/char/hw_random/starfive-trng.c` in `starfive-tech/linux` at
/// `202b558ae34c`) defines byte-for-byte the same offsets and bits as mainline's. The node is in
/// `arch/riscv/dts/jh7110.dtsi` of `starfive-tech/u-boot` at `bfbdce9b86a2`. Its unit address is
/// written with an upper-case `C`, its `reg` uses two address cells and two size cells for the
/// same base and size, its `interrupts` names line 30, and its `status` is `disabled`.
pub const COMPATIBLE_VENDOR: &[u8] = b"starfive,trng";

/// Both spellings, in the order a caller should try them: mainline's first. A tree that carries
/// the standardized binding is describing itself in that language, and that is the one to
/// believe.
pub const COMPATIBLES: [&[u8]; 2] = [COMPATIBLE_MAINLINE, COMPATIBLE_VENDOR];

/// Where a tree says the TRNG is: the register window's physical base and size (from `reg`), and
/// the PLIC interrupt number if the tree gives one (`interrupts`, \[binding\]'s single cell).
///
/// A caller fills one in from its own device-tree walk, reading every property from the one node
/// whose `compatible` matched, so an unrelated node under the other spelling never contributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discovered {
    /// Physical base address of the register window (`reg`'s address cell).
    #[doc(alias = "reg")]
    pub registers_physical_address: u64,
    /// Size of the register window in bytes (`reg`'s size cell). \[binding\]'s example claims
    /// `0x4000`; this crate's [`registers`] only uses the low `0x68` of it.
    #[doc(alias = "reg")]
    pub registers_bytes: u64,
    /// The PLIC interrupt line, if the tree names one. `None` is not an error: a tree with `reg`
    /// but no `interrupts` still names a real device, one a poll-only driver could still use.
    #[doc(alias = "interrupts")]
    pub interrupt_line: Option<u32>,
    /// Which `compatible` string matched: [`COMPATIBLE_MAINLINE`] or [`COMPATIBLE_VENDOR`]. Carried rather
    /// than discarded because the two spellings say which firmware's tree is in hand: the vendor
    /// U-Boot's control tree, or something fuller.
    pub compatible: &'static [u8],
    /// What the node's `status` property says, decoded by [`is_status_okay`] to the one question a
    /// driver has: may this device be used?
    ///
    /// **A caller reports this and need not act on it.** The vendor control tree marks the TRNG
    /// disabled because U-Boot has no driver for it, and the same tree misdescribes another core
    /// in the other direction, so the caller decides. A node the firmware calls disabled is the
    /// first suspect when the window reads all zeros.
    pub status_okay: bool,
}

/// Decode a node's `interrupts` property to the PLIC line, \[binding\]'s single big-endian cell.
/// `None` when the property is shorter than one cell.
pub fn interrupt_line(value: &[u8]) -> Option<u32> {
    value
        .get(0..4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_be_bytes)
}

/// Decode a node's `status` property: `true` when it is absent (`None`, since the device-tree
/// specification's default is `"okay"`) or spells `"okay"` or `"ok"`; `false` for `"disabled"`,
/// `"fail"`, `"fail-sss"` and every other value. `"ok"` is a legacy spelling of `"okay"` that trees
/// in the wild still carry, so it is accepted rather than read as a fourth kind of "no".
///
/// The value is a null (NUL) terminated string in the tree, so both the terminated and the unterminated
/// forms are matched rather than trimmed: a four-way `matches!` says exactly which byte strings
/// count, where a trim would have to decide what an empty or multi-string value means.
pub fn is_status_okay(value: Option<&[u8]>) -> bool {
    value.is_none_or(|v| matches!(v, b"okay\0" | b"ok\0" | b"okay" | b"ok"))
}

/// The result of looking at one `ISTAT` snapshot (and the `RAND` words, if it looked ready).
///
/// Deliberately not `Result`: no variant is this crate's idea of an error. `NotReady` is the
/// expected answer on every poll but the last; `Lockup` is a real, documented hardware condition a
/// caller is expected to handle (retry after reseed), not a bug; `Unseeded` and `WrongOutputWidth`
/// each name the bring-up step a caller repeats before asking again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationOutcome {
    /// A generation completed cleanly. The 32 bytes are `RAND0..RAND7`, little-endian, word 0
    /// first: see [`output_bytes`].
    Ready([u8; 32]),
    /// `ISTAT.LFSR_LOCKUP` was set: an SEU, per \[driver\]'s own comment. The caller should issue
    /// [`CONTROL_COMMAND_RESEED`] and retry, bounded: ask again a few times, then tell the truth.
    Lockup,
    /// `RAND_RDY` was clear, or `STAT` reported a generation ([`STATUS_GENERATING`]) or a reseed
    /// ([`STATUS_RESEEDING`]) in flight: still working, or not yet asked. While either is in flight
    /// the `RAND` words may be mid-rewrite, so a latched `RAND_RDY` beside one is not an answer.
    /// Not a failure on its own; a caller polls again, bounded by its own timeout (this crate does
    /// not impose one, because how long "too long" is is a fact about the real device's timing
    /// this crate cannot know without it).
    NotReady,
    /// **`STAT.SEEDED` was clear**, so whatever the `RAND` words hold did not come from a seeded
    /// pseudo-random number generator (PRNG) core and must not be served as randomness.
    ///
    /// This is \[netbsd\]'s gate: its poll path looks at `RAND_RDY` only when [`STATUS_SEEDED`] is
    /// set, reseeding otherwise. \[trm\]'s `STAT.LAST_RESEED` value `0x7`, "Unseeded (zeroized
    /// state)", shows the block can really be in this state. A caller reseeds and retries,
    /// bounded, rather than reading the words anyway.
    Unseeded,
    /// **`STAT.R256` ([`STATUS_256_BIT_OUTPUT`]) was clear**, so the block answers 128 bits: only
    /// `RAND0..RAND3` carry the answer, and serving all eight words would hand a caller 16 bytes
    /// the generation never wrote.
    ///
    /// The width after reset is a build-time parameter of the silicon, so a bring-up that never
    /// writes [`MODE_256_BIT_OUTPUT`] to [`registers::MODE_REGISTER`] may meet this on every
    /// generation. A caller writes that bit and asks for a new generation; polling again without
    /// the write cannot change the answer.
    #[doc(alias = "R256")]
    WrongOutputWidth,
}

/// Decide what one `(STAT, ISTAT)` snapshot means, given the `RAND` words alongside it.
///
/// A pure decode: the same `(status, interrupt_status, random_numbers)` always decides the same [`GenerationOutcome`]. The
/// precedence is fault, then work in flight, then seeded, then width, then ready:
///
/// 1. A lockup beats everything, because a register file the hardware is re-seeding must not be
///    read as a clean answer.
/// 2. A generation or a reseed in flight ([`STATUS_GENERATING`], [`STATUS_RESEEDING`]) is
///    `NotReady`, because the words may be mid-rewrite, and because the other refusals tell a
///    caller to issue a command, which it must not do while one is in flight.
/// 3. `SEEDED` beats `RAND_RDY`, because `RAND_RDY` is a latched write-1-to-clear bit that can be
///    standing from before this driver ran, while `STAT.SEEDED` is live state; that is why
///    `status` is a parameter.
/// 4. A clear `STAT.R256` is `WrongOutputWidth`, because a 128-bit answer leaves four of the
///    eight words unwritten.
/// 5. Only then does `RAND_RDY` decide between `Ready` and `NotReady`.
pub fn generation_outcome(
    status: u32,
    interrupt_status: u32,
    random_numbers: [u32; 8],
) -> GenerationOutcome {
    if interrupt_status & INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP != 0 {
        GenerationOutcome::Lockup
    } else if status & STATUS_GENERATING != 0 || status & STATUS_RESEEDING != 0 {
        GenerationOutcome::NotReady
    } else if status & STATUS_SEEDED == 0 {
        GenerationOutcome::Unseeded
    } else if status & STATUS_256_BIT_OUTPUT == 0 {
        GenerationOutcome::WrongOutputWidth
    } else if interrupt_status & INTERRUPT_STATUS_RANDOM_NUMBER_READY != 0 {
        GenerationOutcome::Ready(output_bytes(random_numbers))
    } else {
        GenerationOutcome::NotReady
    }
}

/// Lay the eight `RAND` words out as 32 bytes, little-endian, word 0 first: `RAND0`'s low byte is
/// byte 0. There is no cryptographic transform here: without a one-way function, any reshuffling
/// would change the bytes without adding unpredictability an attacker could not undo. Conditioning,
/// if a caller wants it, belongs to the caller's entropy pool. This is data movement, not
/// conditioning.
pub fn output_bytes(random_numbers: [u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, word) in random_numbers.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// How many bytes fit in the one word [`Pool::take`] answers with. Eight, one
/// `u64`: this crate is the device's logic and knows nothing about any wire format, so a caller
/// whose protocol carries a different count clamps to its own.
pub const WORD_BYTES: u64 = 8;

/// What one [`Pool::take`] served: how many bytes, and the bytes themselves as one little-endian
/// word, the first byte served in the low byte.
///
/// The bytes of `word` above `byte_count` are zero, so a short count never reads as more bytes
/// than it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakenBytes {
    /// How many bytes were served, at most [`WORD_BYTES`]. Fewer than asked for means the
    /// generator stopped answering.
    pub byte_count: u64,
    /// The bytes served, little-endian, the first one in the low byte.
    pub word: u64,
}

/// **The 32 bytes in hand, and how many are still ours to give**, testable here without a
/// register. [`take`](Pool::take) is handed a generator: a device poll in a driver, a counter in
/// the tests. `cursor` only moves forward, so no byte is served twice, and each byte is zeroed as
/// it leaves.
///
/// A `Pool` is neither `Clone` nor `Copy`, because a copy would serve the same bytes twice. Its
/// `Debug` output prints [`remaining_bytes`](Pool::remaining_bytes) and never the buffer, so a
/// diagnostic line cannot carry the bytes into a log. Dropping it zeroes the buffer, and
/// [`refill`](Pool::refill) zeroes the bytes it discards. Both zeroing stores go through
/// [`core::hint::black_box`] so the compiler keeps them; that is a best effort, not a guarantee,
/// because a guaranteed store needs a volatile write and this crate holds no `unsafe`. Copies the
/// generator's answer passed through on the way in are the caller's to clear.
///
/// # Examples
///
/// ```
/// # // A worker with a deadline, as in `take_returns_rather_than_spinning`: a `take` that stops
/// # // making progress spins, and a doctest that spins never fails, it keeps running.
/// # let (done, finished) = std::sync::mpsc::channel();
/// # std::thread::spawn(move || {
/// use jh7110_entropy::{Pool, TakenBytes};
///
/// // A device that answers with 32 bytes of 0xAB, forever.
/// let mut generate = || Some([0xab; 32]);
/// let mut pool = Pool::new();
/// assert_eq!(pool.take(4, &mut generate), TakenBytes { byte_count: 4, word: 0xabab_abab });
/// assert_eq!(pool.remaining_bytes(), 28);
///
/// // A device that never answers: the caller is told, rather than spun on.
/// let mut dry = || None;
/// let mut pool = Pool::new();
/// assert_eq!(pool.take(4, &mut dry), TakenBytes { byte_count: 0, word: 0 });
/// # let _ = done.send(());
/// # });
/// # finished
/// #     .recv_timeout(std::time::Duration::from_secs(5))
/// #     .expect("the example panicked (see above), or take never returned");
/// ```
#[derive(PartialEq, Eq)]
pub struct Pool {
    buffer: [u8; 32],
    cursor: usize,
    filled: usize,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for Pool {
    /// Prints [`remaining_bytes`](Pool::remaining_bytes) only: the buffer holds bytes still to be
    /// served, and a log line is not where they go.
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Pool")
            .field("remaining_bytes", &self.remaining_bytes())
            .finish_non_exhaustive()
    }
}

impl Drop for Pool {
    /// Zeroes the buffer, so the bytes still unserved do not outlive the pool in memory.
    fn drop(&mut self) {
        self.zero_buffer();
    }
}

impl Pool {
    /// An empty pool: the first [`take`](Pool::take) generates.
    #[must_use]
    pub const fn new() -> Self {
        Pool {
            buffer: [0; 32],
            cursor: 0,
            filled: 0,
        }
    }

    /// How many bytes are left before the next generation. Test and diagnostic support; the
    /// program reports it once, in its readiness message.
    #[must_use]
    pub const fn remaining_bytes(&self) -> usize {
        self.filled - self.cursor
    }

    /// Fill from `generate`, discarding whatever was left. `false` when the generator would not
    /// answer, which the caller reports rather than retries forever; the pool is then unchanged.
    ///
    /// The bytes discarded are zeroed before the new ones are written over them.
    pub fn refill(&mut self, generate: &mut impl FnMut() -> Option<[u8; 32]>) -> bool {
        match generate() {
            Some(bytes) => {
                self.zero_buffer();
                self.buffer = bytes;
                self.cursor = 0;
                self.filled = 32;
                true
            }
            None => false,
        }
    }

    /// Take `byte_count` bytes, as a [`TakenBytes`]. Gathers across a refill boundary, so a request
    /// can straddle two generations without the client ever seeing the seam, and returns a
    /// **short count** rather than zeros when the device stops answering mid-gather.
    ///
    /// **`byte_count` is clamped to [`WORD_BYTES`]**, because the answer is one 64-bit word and
    /// there is nowhere to put a ninth byte. A caller whose protocol already clamps to 8 never
    /// reaches this clamp; it is here because a public function that shifts by `8 * byte_count`
    /// must not be callable into an overflow, and the first host test written against this API
    /// found exactly that edge.
    pub fn take(
        &mut self,
        byte_count: u64,
        generate: &mut impl FnMut() -> Option<[u8; 32]>,
    ) -> TakenBytes {
        let wanted = byte_count.min(WORD_BYTES);
        let mut word = 0u64;
        let mut served = 0u64;
        while served < wanted {
            if self.cursor == self.filled && !self.refill(generate) {
                break;
            }
            let run = (wanted - served).min((self.filled - self.cursor) as u64);
            for i in 0..run {
                let at = self.cursor + i as usize;
                word |= u64::from(self.buffer[at]) << (8 * (served + i));
                self.buffer[at] = 0;
            }
            self.cursor += run as usize;
            served += run;
        }
        TakenBytes {
            byte_count: served,
            word,
        }
    }

    /// Zero every byte of the buffer, served or not. `black_box` keeps the compiler from dropping
    /// the stores as dead, which it may otherwise do before a drop or an overwrite.
    fn zero_buffer(&mut self) {
        self.buffer = [0; 32];
        core::hint::black_box(&mut self.buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded core in 256-bit mode with nothing in flight: the `STAT` a bring-up leaves.
    const IDLE: u32 = STATUS_SEEDED | STATUS_256_BIT_OUTPUT;

    #[test]
    fn not_ready_when_neither_bit_is_set() {
        assert_eq!(
            generation_outcome(IDLE, 0, [0; 8]),
            GenerationOutcome::NotReady
        );
        assert_eq!(
            generation_outcome(IDLE, INTERRUPT_STATUS_SEED_DONE, [0xffff_ffff; 8]),
            GenerationOutcome::NotReady
        );
    }

    #[test]
    fn ready_assembles_little_endian_word_zero_first() {
        let random_numbers = [
            0x0403_0201,
            0x0807_0605,
            0x0c0b_0a09,
            0x100f_0e0d,
            0x1413_1211,
            0x1817_1615,
            0x1c1b_1a19,
            0x201f_1e1d,
        ];
        match generation_outcome(IDLE, INTERRUPT_STATUS_RANDOM_NUMBER_READY, random_numbers) {
            GenerationOutcome::Ready(bytes) => {
                let expected: [u8; 32] = core::array::from_fn(|i| (i + 1) as u8);
                assert_eq!(bytes, expected);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn an_unseeded_core_is_not_read_as_an_answer() {
        // The failure this prevents: `RAND_RDY` is latched and write-1-to-clear, so it can be
        // standing from before this driver ran. Believing it on a core whose `STAT` says it was
        // never seeded is how a driver serves a register file nobody put entropy into.
        assert_eq!(
            generation_outcome(0, INTERRUPT_STATUS_RANDOM_NUMBER_READY, [0xffff_ffff; 8]),
            GenerationOutcome::Unseeded
        );
        // ...and a fault still outranks it, because a reseeding register file is not a "just
        // reseed it" situation, it is one already in progress.
        assert_eq!(
            generation_outcome(
                0,
                INTERRUPT_STATUS_RANDOM_NUMBER_READY
                    | INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
                [0; 8]
            ),
            GenerationOutcome::Lockup
        );
    }

    #[test]
    fn every_istat_bit_the_trm_names_is_in_the_acknowledge_mask() {
        // `INTERRUPT_STATUS_DOCUMENTED_BITS_MASK` is what a bring-up writes to clear the register. A bit named in this crate
        // but missing from the mask would be one the init clear silently leaves latched.
        for bit in [
            INTERRUPT_STATUS_RANDOM_NUMBER_READY,
            INTERRUPT_STATUS_SEED_DONE,
            INTERRUPT_STATUS_AGE_ALARM,
            INTERRUPT_STATUS_REQUEST_ALARM,
            INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
        ] {
            assert_eq!(
                INTERRUPT_STATUS_DOCUMENTED_BITS_MASK & bit,
                bit,
                "{bit:#x} is not acknowledged"
            );
        }
        // Bits 0..=4, which is every bit [trm] documents in this register.
        assert_eq!(INTERRUPT_STATUS_DOCUMENTED_BITS_MASK, 0b1_1111);
    }

    #[test]
    fn lockup_beats_ready_in_the_same_snapshot() {
        assert_eq!(
            generation_outcome(
                IDLE,
                INTERRUPT_STATUS_RANDOM_NUMBER_READY
                    | INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
                [0xffff_ffff; 8]
            ),
            GenerationOutcome::Lockup
        );
    }

    #[test]
    fn lockup_alone_is_reported_even_with_no_rand_rdy() {
        assert_eq!(
            generation_outcome(
                IDLE,
                INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
                [0; 8]
            ),
            GenerationOutcome::Lockup
        );
    }

    #[test]
    fn seed_done_alone_is_not_confused_with_rand_rdy() {
        // SEED_DONE (bit 1) and RAND_RDY (bit 0) are adjacent; a shift-by-one bug here would pass
        // every other test in this file and fail silently on real hardware after a reseed.
        assert_eq!(
            generation_outcome(IDLE, INTERRUPT_STATUS_SEED_DONE, [0; 8]),
            GenerationOutcome::NotReady
        );
    }

    /// **A 128-bit answer is refused.** With `STAT.R256` clear only four words carry the
    /// generation, so a seeded, idle core with `RAND_RDY` set answers `WrongOutputWidth`, and so
    /// does one still waiting, because no amount of polling changes the width.
    #[test]
    fn a_128_bit_answer_is_refused() {
        for interrupt_status in [0, INTERRUPT_STATUS_RANDOM_NUMBER_READY] {
            assert_eq!(
                generation_outcome(STATUS_SEEDED, interrupt_status, [0xffff_ffff; 8]),
                GenerationOutcome::WrongOutputWidth
            );
        }
        // A lockup still outranks it, and an unseeded core is still Unseeded first.
        assert_eq!(
            generation_outcome(
                STATUS_SEEDED,
                INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
                [0; 8]
            ),
            GenerationOutcome::Lockup
        );
        assert_eq!(
            generation_outcome(0, INTERRUPT_STATUS_RANDOM_NUMBER_READY, [0; 8]),
            GenerationOutcome::Unseeded
        );
    }

    /// **Work in flight is `NotReady`, whatever `ISTAT` says.** A `RAND_RDY` latched from the last
    /// generation beside a new generation or a reseed in flight is not an answer: the words may be
    /// mid-rewrite.
    #[test]
    fn a_generation_or_reseed_in_flight_is_not_ready() {
        for in_flight in [
            STATUS_GENERATING,
            STATUS_RESEEDING,
            STATUS_GENERATING | STATUS_RESEEDING,
        ] {
            for status in [IDLE | in_flight, in_flight, STATUS_SEEDED | in_flight] {
                assert_eq!(
                    generation_outcome(
                        status,
                        INTERRUPT_STATUS_RANDOM_NUMBER_READY,
                        [0xffff_ffff; 8]
                    ),
                    GenerationOutcome::NotReady,
                    "STAT {status:#010x}"
                );
            }
        }
        // A lockup is still reported while a reseed runs.
        assert_eq!(
            generation_outcome(
                IDLE | STATUS_RESEEDING,
                INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
                [0; 8]
            ),
            GenerationOutcome::Lockup
        );
    }

    /// `status` decoding, at the four values a tree can carry, one it cannot, and the absent
    /// property. This is a three-line function guarding a fact a bench session will read off the
    /// boot tour, and the failure it would cause is the expensive kind: a `status` misread as okay
    /// turns "the firmware never ungated this device's clocks" into an unexplained window of zeros.
    #[test]
    fn status_is_decoded_the_way_the_specification_defines_it() {
        assert!(is_status_okay(None));
        assert!(is_status_okay(Some(b"okay\0")));
        assert!(is_status_okay(Some(b"ok\0")));
        assert!(is_status_okay(Some(b"okay")));
        assert!(is_status_okay(Some(b"ok")));
        assert!(!is_status_okay(Some(b"disabled\0")));
        assert!(!is_status_okay(Some(b"fail\0")));
        assert!(!is_status_okay(Some(b"")));
    }

    /// `interrupts` is one big-endian cell; a short property names no line, and a longer one
    /// is read from its first cell only.
    #[test]
    fn the_interrupt_is_the_first_big_endian_cell() {
        assert_eq!(interrupt_line(&[0, 0, 0, 30]), Some(30));
        assert_eq!(
            interrupt_line(&[0, 0, 1, 2, 0xff, 0xff, 0xff, 0xff]),
            Some(0x102)
        );
        assert_eq!(interrupt_line(&[0, 0, 30]), None);
        assert_eq!(interrupt_line(&[]), None);
    }

    /// Mainline's spelling is tried first, and the two spellings are the two trees' own.
    #[test]
    fn both_trees_spellings_in_mainline_first_order() {
        assert_eq!(
            COMPATIBLES,
            [&b"starfive,jh7110-trng"[..], &b"starfive,trng"[..]]
        );
    }

    /// **No byte is ever served twice**, across as many refills as it takes to drain several
    /// generations. The generator hands out a distinct byte per call, so a cursor that wrapped
    /// instead of refilling, or a refill that did not reset the cursor, shows up as a repeat.
    #[test]
    fn no_byte_is_served_twice_across_refills() {
        let seen = within_a_deadline(|| {
            let mut next = 1u8;
            let mut generate = || {
                let block = core::array::from_fn(|i| next.wrapping_add(i as u8));
                next = next.wrapping_add(32);
                Some(block)
            };
            let mut pool = Pool::new();
            let mut seen = [0u32; 256];
            // 40 draws of 8 bytes is 320 bytes, ten generations, so the seam is crossed nine times.
            for _ in 0..40 {
                let taken = pool.take(8, &mut generate);
                assert_eq!(taken.byte_count, 8);
                for b in taken.word.to_le_bytes() {
                    seen[b as usize] += 1;
                }
            }
            seen
        });
        // The generator's byte stream is 1, 2, 3, ... wrapping, so 320 bytes visits every value
        // once or twice and never three times; a repeat from the pool would push one to three.
        for (value, &count) in seen.iter().enumerate() {
            assert!(count <= 2, "byte {value} came back {count} times");
        }
    }

    /// **A request that straddles a generation gets all of its bytes**, in order, with the seam
    /// invisible: the last four of one block and the first four of the next.
    #[test]
    fn a_request_straddles_the_seam_without_losing_bytes() {
        within_a_deadline(|| {
            let mut which = 0u8;
            let mut generate = || {
                which += 1;
                Some([which; 32])
            };
            let mut pool = Pool::new();
            // Five-byte draws: the seventh starts at byte 30 of a 32-byte block, so it takes two
            // bytes from block 1 and three from block 2.
            for _ in 0..6 {
                assert_eq!(pool.take(5, &mut generate).byte_count, 5);
            }
            let taken = pool.take(5, &mut generate);
            assert_eq!(taken.byte_count, 5);
            assert_eq!(taken.word.to_le_bytes()[..5], [1, 1, 2, 2, 2]);
        });
    }

    /// **A device that stops answering produces a short count, not zeros.** The distinction is the
    /// whole of an entropy service's honesty: a client that asked for eight bytes and got four must
    /// be told four, or it will treat four zeros as entropy.
    #[test]
    fn a_dry_device_shortens_the_count_rather_than_padding() {
        within_a_deadline(|| {
            let mut answers = 1;
            let mut generate = || {
                if answers > 0 {
                    answers -= 1;
                    Some([0xff; 32])
                } else {
                    None
                }
            };
            let mut pool = Pool::new();
            assert_eq!(pool.take(8, &mut generate).byte_count, 8);
            // Drain the one block this generator will ever give: 32 bytes, so three more eights.
            for _ in 0..3 {
                assert_eq!(pool.take(8, &mut generate).byte_count, 8);
            }
            assert_eq!(
                pool.take(8, &mut generate),
                TakenBytes {
                    byte_count: 0,
                    word: 0
                }
            );
        });
    }

    /// **A served byte is not still sitting in the buffer.** Reading the same region back after a
    /// take must not reproduce it; the pool zeroes behind its cursor.
    #[test]
    fn a_served_byte_is_zeroed_behind_the_cursor() {
        within_a_deadline(|| {
            let mut generate = || Some([0xa5; 32]);
            let mut pool = Pool::new();
            assert_eq!(
                pool.take(8, &mut generate),
                TakenBytes {
                    byte_count: 8,
                    word: 0xa5a5_a5a5_a5a5_a5a5
                }
            );
            assert_eq!(pool.buffer[..8], [0u8; 8]);
            assert_eq!(pool.remaining_bytes(), 24);
        });
    }

    /// **A refill zeroes what it discards, and so does a drop.** Both go through `zero_buffer`,
    /// which is what this checks; that `Drop` calls it is visible only in memory a safe test cannot
    /// read after the drop.
    #[test]
    fn zero_buffer_clears_every_byte() {
        let mut pool = Pool::new();
        assert!(pool.refill(&mut || Some([0x5a; 32])));
        pool.zero_buffer();
        assert_eq!(pool.buffer, [0u8; 32]);
    }

    /// **A refill that gets no answer leaves the pool as it was**, so a dry device never turns
    /// unserved bytes into zeros a later take would serve.
    #[test]
    fn a_refused_refill_leaves_the_pool_unchanged() {
        let mut pool = Pool::new();
        assert!(pool.refill(&mut || Some([0x5a; 32])));
        assert!(!pool.refill(&mut || None));
        assert_eq!(pool.buffer, [0x5a; 32]);
        assert_eq!(pool.remaining_bytes(), 32);
    }

    /// **`Debug` prints the count and never the bytes.** A pool full of `0xa5` prints no `165`,
    /// the byte's decimal spelling that a derived `Debug` would repeat 32 times.
    #[test]
    fn debug_prints_the_remaining_count_and_not_the_buffer() {
        let mut pool = Pool::new();
        assert!(pool.refill(&mut || Some([0xa5; 32])));
        let printed = format!("{pool:?}");
        assert_eq!(printed, "Pool { remaining_bytes: 32, .. }");
        assert!(!printed.contains("165"));
    }

    /// **A caller asking for more than a word gets a word**, not a shift-left overflow. The wire
    /// format clamps first, so this is defense at the API rather than a live path.
    #[test]
    fn a_request_larger_than_the_word_is_clamped() {
        within_a_deadline(|| {
            let mut generate = || Some([0x11; 32]);
            let mut pool = Pool::new();
            assert_eq!(
                pool.take(64, &mut generate),
                TakenBytes {
                    byte_count: 8,
                    word: 0x1111_1111_1111_1111
                }
            );
        });
    }

    /// **Every named bit is the bit its own documentation says it is, and no two bits in a
    /// register are the same bit.**
    ///
    /// A transcription check: the constants came off \[trm\]'s tables by hand, and a bit copied to
    /// the wrong line, or a `<<` turned `>>` that takes a bit to zero, passes every test that does
    /// not name the bit it exercises. A mutation sweep found fifteen survivors of that shape. A
    /// duplicated bit inside one register is the copy-and-paste mistake the table invites.
    #[test]
    fn every_named_bit_is_the_bit_the_datasheet_names() {
        // One row per named bit: (register, name, bit index, constant). The indices are \[trm\]'s
        // own numbering, repeated from each constant's doc comment so the two cannot drift apart
        // silently. Flat rather than grouped by register, so the table reads down the page the way
        // the datasheet's own does.
        let named: [(&str, &str, u32, u32); 26] = [
            ("MODE", "R256", 3, MODE_256_BIT_OUTPUT),
            ("SMODE", "MISSION_MODE", 8, STATUS_MODE_MISSION_MODE),
            ("STAT", "NONCE_MODE", 2, STATUS_NONCE_MODE),
            ("STAT", "R256", 3, STATUS_256_BIT_OUTPUT),
            ("STAT", "MISSION_MODE", 8, STATUS_MISSION_MODE),
            ("STAT", "SEEDED", 9, STATUS_SEEDED),
            ("STAT", "SRVC_RQST", 27, STATUS_SERVICE_REQUEST),
            ("STAT", "RAND_GENERATING", 30, STATUS_GENERATING),
            ("STAT", "RAND_RESEEDING", 31, STATUS_RESEEDING),
            ("IE", "RAND_RDY_EN", 0, INTERRUPT_ENABLE_RANDOM_NUMBER_READY),
            ("IE", "SEED_DONE_EN", 1, INTERRUPT_ENABLE_SEED_DONE),
            ("IE", "AGE_ALARM_EN", 2, INTERRUPT_ENABLE_AGE_ALARM),
            ("IE", "RQST_ALARM_EN", 3, INTERRUPT_ENABLE_REQUEST_ALARM),
            (
                "IE",
                "LFSR_LOCKUP_EN",
                4,
                INTERRUPT_ENABLE_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
            ),
            ("IE", "GLBL_EN", 31, INTERRUPT_ENABLE_GLOBAL),
            ("ISTAT", "RAND_RDY", 0, INTERRUPT_STATUS_RANDOM_NUMBER_READY),
            ("ISTAT", "SEED_DONE", 1, INTERRUPT_STATUS_SEED_DONE),
            ("ISTAT", "AGE_ALARM", 2, INTERRUPT_STATUS_AGE_ALARM),
            ("ISTAT", "RQST_ALARM", 3, INTERRUPT_STATUS_REQUEST_ALARM),
            (
                "ISTAT",
                "LFSR_LOCKUP",
                4,
                INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
            ),
            (
                "FEATURES",
                "MM_RESET_STATE",
                3,
                FEATURES_MISSION_MODE_RESET_STATE,
            ),
            (
                "FEATURES",
                "RAND_SEED_AVAIL",
                2,
                FEATURES_RANDOM_SEED_AVAILABLE,
            ),
            (
                "BCONF",
                "AUTO_RESEED_LOOPBACK",
                5,
                BUILD_CONFIGURATION_AUTOMATIC_RESEED_LOOPBACK,
            ),
            (
                "BCONF",
                "MODE_AFTER_RST",
                4,
                BUILD_CONFIGURATION_MISSION_MODE_AFTER_RESET,
            ),
            (
                "BCONF",
                "PRNG_LEN_AFTER_RST",
                3,
                BUILD_CONFIGURATION_OUTPUT_WIDTH_AFTER_RESET,
            ),
            (
                "BCONF",
                "MAX_PRNG_LEN",
                2,
                BUILD_CONFIGURATION_WIDEST_OUTPUT_256_BIT,
            ),
        ];
        // The `CTRL_*` constants are deliberately absent: they are an enumerated command value
        // (0, 1, 2, 3) written to one field, not bits in a mask, so neither assertion below is
        // true of them and neither would mean anything if it were.

        for &(register, name, index, value) in &named {
            assert_eq!(
                value.count_ones(),
                1,
                "{register}.{name} is not one bit: {value:#010x}"
            );
            assert_eq!(
                value.trailing_zeros(),
                index,
                "{register}.{name} is at bit {}, and its own documentation says {index}",
                value.trailing_zeros()
            );
        }

        for (a, &(register_a, name_a, _, value_a)) in named.iter().enumerate() {
            for &(register_b, name_b, _, value_b) in &named[a + 1..] {
                if register_a == register_b {
                    assert_ne!(
                        value_a, value_b,
                        "{register_a}.{name_a} and {register_b}.{name_b} are the same bit"
                    );
                }
            }
        }
    }

    /// **The multi-bit fields sit where their sources put them**: `STAT.LAST_RESEED` at bits 16 to
    /// 18, clear of every single `STAT` bit this crate names, and `FEATURES.MAX_RAND_LENGTH` at
    /// bits 0 and 1, clear of the two `FEATURES` bits.
    #[test]
    fn the_multi_bit_fields_are_where_their_sources_put_them() {
        assert_eq!(STATUS_LAST_RESEED_MASK, 0x0007_0000);
        assert_eq!(
            STATUS_LAST_RESEED_MASK >> STATUS_LAST_RESEED_SHIFT,
            STATUS_LAST_RESEED_UNSEEDED
        );
        for bit in [
            STATUS_NONCE_MODE,
            STATUS_256_BIT_OUTPUT,
            STATUS_MISSION_MODE,
            STATUS_SEEDED,
            STATUS_SERVICE_REQUEST,
            STATUS_GENERATING,
            STATUS_RESEEDING,
        ] {
            assert_eq!(STATUS_LAST_RESEED_MASK & bit, 0, "{bit:#010x}");
        }
        assert_eq!(FEATURES_WIDEST_OUTPUT_MASK, 0b11);
        assert_eq!(
            FEATURES_WIDEST_OUTPUT_256_BIT & !FEATURES_WIDEST_OUTPUT_MASK,
            0
        );
        assert_eq!(FEATURES_WIDEST_OUTPUT_256_BIT, 1);
        for bit in [
            FEATURES_MISSION_MODE_RESET_STATE,
            FEATURES_RANDOM_SEED_AVAILABLE,
        ] {
            assert_eq!(FEATURES_WIDEST_OUTPUT_MASK & bit, 0, "{bit:#010x}");
        }
    }

    /// **The eight seed words are four bytes apart from `SEED0` at `0x40`**, as the eight `RAND`
    /// words are from `RAND0` at `0x20`.
    #[test]
    fn the_seed_and_random_number_words_are_consecutive() {
        use registers::*;
        let seeds = [
            SEED_0_REGISTER,
            SEED_1_REGISTER,
            SEED_2_REGISTER,
            SEED_3_REGISTER,
            SEED_4_REGISTER,
            SEED_5_REGISTER,
            SEED_6_REGISTER,
            SEED_7_REGISTER,
        ];
        let random_numbers = [
            RANDOM_NUMBER_0_REGISTER,
            RANDOM_NUMBER_1_REGISTER,
            RANDOM_NUMBER_2_REGISTER,
            RANDOM_NUMBER_3_REGISTER,
            RANDOM_NUMBER_4_REGISTER,
            RANDOM_NUMBER_5_REGISTER,
            RANDOM_NUMBER_6_REGISTER,
            RANDOM_NUMBER_7_REGISTER,
        ];
        for (i, (&seed, &random_number)) in seeds.iter().zip(&random_numbers).enumerate() {
            assert_eq!(seed, 0x40 + 4 * i as u64);
            assert_eq!(random_number, 0x20 + 4 * i as u64);
        }
    }

    /// **[`INTERRUPT_STATUS_DOCUMENTED_BITS_MASK`] is five distinct bits**, which is what makes the write-1-to-clear an
    /// acknowledgement of exactly the bits this crate can name rather than a `!0` that swallows
    /// whatever else the block latched. Disjointness is the reason it may be written with either
    /// `|` or `^`, which a mutation sweep had to argue for four surviving mutants and would rather
    /// a reader could simply check.
    #[test]
    fn the_clear_mask_is_every_named_istat_bit_and_nothing_else() {
        assert_eq!(
            INTERRUPT_STATUS_DOCUMENTED_BITS_MASK.count_ones(),
            5,
            "{INTERRUPT_STATUS_DOCUMENTED_BITS_MASK:#010x}"
        );
        for bit in [
            INTERRUPT_STATUS_RANDOM_NUMBER_READY,
            INTERRUPT_STATUS_SEED_DONE,
            INTERRUPT_STATUS_AGE_ALARM,
            INTERRUPT_STATUS_REQUEST_ALARM,
            INTERRUPT_STATUS_LINEAR_FEEDBACK_SHIFT_REGISTER_LOCKUP,
        ] {
            assert_eq!(
                INTERRUPT_STATUS_DOCUMENTED_BITS_MASK & bit,
                bit,
                "{bit:#010x} is not in the clear mask"
            );
        }
    }

    /// **`take` returns**, and what this asserts is that it returns at all rather than what it
    /// returns.
    ///
    /// Every pass of the gather loop must make progress (refill, copy, advance cursor and count);
    /// break one and the call never answers. A mutation sweep found four such mutants, each of
    /// which hung the suite, so this test runs `take` under a deadline.
    #[test]
    fn take_returns_rather_than_spinning() {
        assert_eq!(
            take_within_a_deadline(4, || Some([0xab; 32])),
            TakenBytes {
                byte_count: 4,
                word: 0xabab_abab
            }
        );
        assert_eq!(
            take_within_a_deadline(8, || None),
            TakenBytes {
                byte_count: 0,
                word: 0
            }
        );

        // One answer and then nothing, so a single call crosses from "has bytes" to "dry".
        let mut answers = 1;
        assert_eq!(
            take_within_a_deadline(8, move || {
                if answers > 0 {
                    answers -= 1;
                    Some([0x5a; 32])
                } else {
                    None
                }
            })
            .byte_count,
            8
        );
    }

    /// A fresh [`Pool`] per call, because the point is the call and not the history.
    fn take_within_a_deadline(
        byte_count: u64,
        mut generate: impl FnMut() -> Option<[u8; 32]> + Send + 'static,
    ) -> TakenBytes {
        within_a_deadline(move || Pool::new().take(byte_count, &mut generate))
    }

    /// The deadline itself: run `body` on a worker and fail if it has not finished in five
    /// seconds. Every test that calls [`Pool::take`] goes through this, because a `take` that
    /// stops making progress hangs the whole test binary otherwise, and a mutation sweep then
    /// reports a timeout instead of a caught mutant. A panic inside `body` is reported by the
    /// worker's own default hook and surfaces here as a closed channel.
    fn within_a_deadline<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(body());
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the body panicked (see above), or take never returned: its gather loop made no progress")
    }
}

#[cfg(kani)]
mod verification;
