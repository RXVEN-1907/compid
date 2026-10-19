# Security policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | yes       |

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting for this repository
(Security tab, then "Report a vulnerability"). Do not open a public issue for
an unpatched vulnerability.

Include what you can of the following:

- the compid version (`compid --version`) or commit you tested against
- the input that triggers the problem - the binary itself if you can share it,
  or a description of its shape (architecture, malformed structure) if you
  cannot
- what you expected versus what actually happened

You should hear back within a week. There is no promised fix timeline, but
reports are acknowledged, triaged, and credited in the changelog on request.

## Scope

compid parses untrusted input. The extractor reads ELF files chosen by whoever
hands them to it; it never executes the binary it is reading, but malformed
input that crashes, hangs, or drives unbounded resource use through the
parsing path is squarely in scope. So is a crafted `model.json` that makes
inference behave in a way the documented schema does not permit.

The corpus and training tooling runs against binaries built locally, as
documented; problems there are in scope when they affect a user following the
documented workflow.

Out of scope: the accuracy of predictions (that is a quality report - use a
regular issue), and resource exhaustion from someone deliberately feeding a
multi-gigabyte file to their own machine.

## Safe use

Treat every input binary as untrusted data - the same posture you would take
with a disassembler. When samples come from the wild, run triage in a
disposable environment.
