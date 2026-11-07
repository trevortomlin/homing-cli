# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0,
minor versions may contain breaking changes.

## [Unreleased]

## [0.1.0] - 2026-11-06

First release, based on homing-format v0.1.0. Prebuilt binaries for Linux (x86_64, aarch64)
and macOS (arm64, x86_64).

### Added

- `homing init`: write a `homing.toml` template and `homing/config/`.
- `homing keygen`: Ed25519 signing keys (`<name>.key` with mode 0600, `prod-`/`dev-` public
  keys). Creates the output directory if needed.
- `homing pack`: reproducible, signed `.homing` packages (zstd by default). Honors
  `SOURCE_DATE_EPOCH`, and reads the key from `--key` or `HOMING_SIGNING_KEY`.
- `homing verify`: full verification with optional device compatibility (`--engine-json`) and
  anti-rollback floor checks. Exit code 2 and the error code on failure; `--json` output.
- `homing inspect`: print a package's manifest (human-readable or `--json`).
- `homing.toml` may leave out `app.name` and `app.version`; they are then read from the Flutter
  app's `pubspec.yaml`.
- `homing index add | remove | verify | show`: maintain a signed update index (`index.json` +
  `index.json.sig`) for HTTP update sources. `add` can verify packages first (`--keys`) and
  refuses to re-sign an index that no longer verifies, unless `--force` is given. `verify` can
  show which build a given device would install.
- Exit codes: 0 success, 1 usage or configuration error, 2 verification failure, 3 I/O error.

[Unreleased]: https://github.com/trevortomlin/homing-cli/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/trevortomlin/homing-cli/releases/tag/v0.1.0
