# The stranger cadence

The release's whole claim is that a stranger, given this repository and nothing else, can build
the crate and run its proofs from the README alone. The maintainers run that check with a real agent on a
cadence, and the weekly `stranger cadence` job fails when the last run is past 28 days.

What a run is:

1. Clone the repository fresh.
2. Follow the README and nothing else: build, test, and run the proofs.
3. Note every place the README lied, omitted, or assumed. Those are findings against the README,
   not against the stranger.
4. Record the date on the line below.

Last run: 2026-10-31

Result of that run: the stranger reached a passing build, 18 unit tests and 2 doctests, and all
four Kani proofs, in 144 seconds and `$0.20`, with no question that blocked it. It did not have to
install Kani, because the machine already had it, so the run could not show whether the README
tells a newcomer how; it did not, and the README now does. The full record is in
[the run appendix](stranger-test-run-2026-10-31.md).
