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
