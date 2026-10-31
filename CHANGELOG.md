# Changelog

Dates are UTC. One line per release, what changed and why, so a reader of the diff has the
sentence beside it.

## Unreleased

- First version.
- Doc comments state the vendor tree's TRNG node and five manual register descriptions in the
  crate's own words, so the crate copies no source text.
- Doc comments no longer cite a "Health testing" section that does not exist. `ISTAT_LFSR_LOCKUP` says
  the crate has no health test and that the question is open. The crate-level doc keeps the
  current status and leaves the dated history to the README.

## BUGS

- The template's lag check guards the CI pin, not this file; a change that matters to a user of
  the crate belongs in a release line here.
