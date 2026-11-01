# The crate carries facts from its sources, not their text

This is an engineering audit, written by an AI agent that is not a lawyer, and it is not legal
advice. It records what the crate takes from each source it cites, compared mechanically against
that source at the cited revision, and what each source's licence header says. A lawyer can check
every row against the files named here. The audit covers this repository only.

The question, asked 2026-10-31: does the crate break the licence of Linux, or of any other source
it cites? It transcribes register offsets, bit positions, names and device-tree facts from them.

## Verdict

Found and fixed. Two kinds of copied text were present, and both are
now rewritten in the crate's own words. One was an 11-token excerpt of the vendor U-Boot device-tree
node in a doc comment; the other was five short runs of 4 to 8 tokens from the JH7110 Technical
Reference Manual's register descriptions. No text from either Linux driver or from NetBSD's driver
was present: the longest run shared with any of them is 4 tokens, and every such run is a hardware
name or a stock phrase.

## How the comparison was made

The audit downloaded each source file at the revision the crate cites, into a directory of its
own, and compared every text file in this repository with each of them. It reduced both sides to
lower-case word tokens, dropping punctuation, comment markers and whitespace, and normalised
hexadecimal numbers so that `0x1600_C000` and `0x1600C000` compare equal. The check reports every
maximal run of identical tokens of length 4 or more. A second pass split identifiers at underscores,
so that `STARFIVE_CTRL_GENE_RANDNUM` and `CTRL_GENE_RANDNUM` would meet.

## The sources and their licence headers

The table reproduces the header lines exactly and nothing else from the files.

| Source | Revision compared | Licence header |
|---|---|---|
| Linux `drivers/char/hw_random/jh7110-trng.c` | `torvalds/linux` at `db9deec5a345` (master on 2026-08-24; the file last changed in `cad76142aaa4`) | `// SPDX-License-Identifier: GPL-2.0`, `Copyright (C) 2022 StarFive Technology Co.` |
| Linux `Documentation/devicetree/bindings/rng/starfive,jh7110-trng.yaml` | same tree (the file last changed in `f1b2fe908467`) | `# SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)`, with no copyright line |
| Linux `arch/riscv/boot/dts/starfive/jh7110.dtsi` | same tree | `// SPDX-License-Identifier: GPL-2.0 OR MIT`, `Copyright (C) 2022 StarFive Technology Co., Ltd.`, `Copyright (C) 2022 Emil Renner Berthing` |
| Vendor U-Boot `arch/riscv/dts/jh7110.dtsi` | `starfive-tech/u-boot` at `bfbdce9b86a2` | `// SPDX-License-Identifier: GPL-2.0 OR MIT`, `Copyright (C) 2022 StarFive Technology Co., Ltd.`, `Copyright (C) 2022 Hal Feng` |
| Vendor Linux `drivers/char/hw_random/starfive-trng.c` | `starfive-tech/linux` at `202b558ae34c` | `// SPDX-License-Identifier: GPL-2.0`, `Copyright (C) 2022 StarFive Technology Co.` |
| NetBSD `sys/arch/riscv/starfive/jh7110_trng.c` | revision 1.2 (`$NetBSD: jh7110_trng.c,v 1.2 2025/02/09 09:09:49 skrll Exp $`) | No SPDX line. `Copyright (c) 2025 The NetBSD Foundation, Inc.`, followed by the NetBSD Foundation's two-clause BSD licence |
| JH7110 Technical Reference Manual, "TRNG > Control Registers" | the HTML edition at `doc-en.rvspace.org`, which states "Preliminary V2", "2023/4/24", Doc ID `JH7110-TRMEN-001` | No licence. The page footer reads `© 2022 Sponsored by StarFive Technology . All rights reserved.` |

About the manual, the audit could not establish whether the site has terms of use beyond that
footer, or whether a PDF edition carries different terms. The crate cites the manual by edition
and section title, which matches the HTML edition compared here.

## Inventory

Classes: fact (an offset, bit position, compatible string, interrupt number or register name),
own words, near-verbatim (the source's sentence, lightly changed) and verbatim (an
identical run longer than a few tokens).

| Place | What it holds | Drawn from | Class |
|---|---|---|---|
| `src/lib.rs`, `registers` | 20 byte offsets, `CTRL` to `BUILD_CONFIGURATION` | driver, manual, NetBSD | fact |
| `src/lib.rs`, bit constants | command values 0 to 3 and every `MODE`, `SMODE`, `STAT`, `IE` and `ISTAT` bit | driver, manual, NetBSD | fact |
| `src/lib.rs`, `COMPATIBLE_MAINLINE`, `COMPATIBLE_VENDOR` | `starfive,jh7110-trng`, `starfive,trng` | binding, vendor tree | fact |
| `src/lib.rs`, register and bit doc comments | write-1-to-clear, zero disables the auto alarms, the seeded gate, the drivers' bring-up order | manual, driver, NetBSD | own words |
| `src/lib.rs`, `COMPATIBLE_VENDOR` doc | the vendor node, once as one line of device-tree source | vendor tree | was verbatim, 11 tokens; now own words |
| `src/lib.rs`, five register doc comments | `SEED0`, `AUTO_RQSTS`, `SMODE_MISSION_MODE`, `FEATURES`, `BUILD_CONFIGURATION` | manual | were near-verbatim, 4 to 8 tokens; now own words |
| `src/lib.rs`, `IE_GLBL_EN` doc | the manual's phrase "interrupt and/or status contributions", in quotation marks | manual | a 5-token quotation, kept because the comment argues about that wording |
| `src/lib.rs`, `GenerationOutcome::Unseeded` doc | the label of `STAT.LAST_RESEED` value `0x7`, in quotation marks | manual | fact (a value's name) |
| `src/lib.rs`, unit tests | the bit table, the two strings, interrupt 30 as four bytes | as above | fact |
| `src/verification.rs`, `falsifications/` | proofs and the patches that break them | none | own words |
| `README.md` | the two strings, the window, interrupt 30, `status = "disabled"` | binding, vendor tree | fact, in own words |
| `CHANGELOG.md`, `docs/stranger-test.md` | nothing from any source | none | own words |

## The longest runs found

Before the fixes, against this repository:

- 11 tokens, vendor tree: the `trng@1600C000` node's label, name, `compatible` and `reg`.
- 8 tokens, manual: the `SEEDx` description, twice.
- 7 tokens, manual: the test-mode description and the `RQSTS` field values.
- 5 tokens, manual: "interrupt and/or status contributions", which the crate keeps.
- 4 tokens, Linux driver: "true random number generator", "compatible starfive jh7110 trng", and
  `IE` field names such as `IE_RAND_RDY_EN`.
- Nothing of 4 tokens or more from NetBSD's driver or the vendor Linux driver.

After the fixes, the 5-token quotation is the longest run with any source, and no run from either
GPL-2.0 driver exceeds 4 tokens.

## What each licence allows here

- GPL-2.0, both Linux drivers. Nobody could relicense text copied from them as `MIT OR
  Apache-2.0`, and no notice would cure that. The crate holds none. What it takes is offsets, bit
  positions and register names, which are facts about the silicon; whether a jurisdiction protects
  such a list is a question for a lawyer.
- GPL-2.0-only OR BSD-2-Clause, the binding. You may reuse the text under the BSD-2-Clause choice,
  keeping its conditions and disclaimer in source copies. The file states no copyright holder. The
  crate holds only the compatible string.
- GPL-2.0 OR MIT, both device trees. You may reuse the text under the MIT choice, keeping the
  copyright lines and the permission notice. The crate now holds none.
- NetBSD's two-clause BSD. You may reuse the text, keeping the copyright, the two conditions and the
  disclaimer. The crate holds none of its text; it cites the seeded gate and the alarm bit
  positions as facts and describes them in its own words.
- The manual, all rights reserved. It grants no licence, so nobody may copy its prose beyond
  short quotation. Its register names and values are facts. The crate quotes 5 words of it, in
  quotation marks, with the source named.

The crate needs no `NOTICE` file and no third-party licence text.

## Identifiers that match a source name

The audit found five constant names that were Linux's `#define` names rather than the manual's.
Four are command values: `CTRL_EXEC_NOP`, `CTRL_GENE_RANDNUM`, `CTRL_EXEC_RANDRESEED` and
`CTRL_EXEC_NONCE_RESEED`. They are Linux's names without their `STARFIVE_` prefix. The manual
describes the four values in words and names none of them. The fifth is `STAT_RAND_SEEDING`, where
the manual calls bit 31 `RAND_RESEEDING`. The crate now
names them `CTRL_COMMAND_NONE`, `CTRL_COMMAND_GENERATE`, `CTRL_COMMAND_RESEED`,
`CTRL_COMMAND_NONCE_RESEED` and `STAT_RESEEDING`. Each old spelling stays in a doc comment and in a
`#[doc(alias)]`, so a search for it still lands on the constant. The other constant names are the
manual's own register and field names (`CTRL`, `ISTAT`, `RAND_RDY`, `GLBL_EN` and the rest).
