# Security Policy

homing-cli creates signing keys and signs packages, so security reports are taken
seriously.

## Reporting a vulnerability

Please **do not** open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/trevortomlin/homing-cli/security/advisories/new)
(the "Security" tab, then "Report a vulnerability"), or email **trevortomlin2@outlook.com**.

Please include a description, the affected version or commit, and a reproducer if possible
(for example, the commands and files needed to trigger the issue). You can expect an
acknowledgement within 7 days. Once a fix is available, we will coordinate disclosure with you
and credit you, unless you prefer otherwise.

## Scope

In scope: secret key handling (generation, file permissions, the `HOMING_SIGNING_KEY`
environment variable), packing (what ends up in a signed package), and `homing verify` reporting
PASS for a package that a device would reject, or the reverse. Format-level issues belong to
[homing-format](https://github.com/trevortomlin/homing-format/security).

## Supported versions

homing-cli has not been audited. Until 1.0, only the latest release receives fixes.
