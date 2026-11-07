# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0,
minor versions may contain breaking changes.

## [Unreleased]

### Added

- `homing init`: write a `homing.toml` template and `homing/config/`.
- `homing keygen`: Ed25519 signing keys (`<name>.key` with mode 0600, `prod-`/`dev-` public
  keys). Creates the output directory if needed.
- `homing pack`: reproducible, signed `.homing` packages (zstd by default). Honors
  `SOURCE_DATE_EPOCH`, and reads the key from `--key` or `HOMING_SIGNING_KEY`.
- `homing verify`: full verification with optional device compatibility (`--engine-json`) and
  anti-rollback floor checks. Exit code 2 and the error code on failure; `--json` output.
- `homing inspect`: print a package's manifest (human-readable or `--json`).
- `homing index add | remove | verify | show`: maintain a signed update index (`index.json` +
  `index.json.sig`) for HTTP update sources. `add` can verify packages first (`--keys`) and
  refuses to re-sign an index that no longer verifies, unless `--force` is given. `verify` can
  show which build a given device would install.

### Changed

- Depends on homing-format v0.1.0.
- Exit code 1 now covers all usage and configuration errors (for example a missing signing
  key); 3 is reserved for I/O errors.
