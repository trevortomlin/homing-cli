# homing-cli

[![CI](https://github.com/trevortomlin/homing-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/trevortomlin/homing-cli/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-orange.svg)](#minimum-supported-rust-version)

`homing` is the host-side tool for **homing**: signed, versioned over-the-air app packages for
embedded Linux devices that run Flutter apps (flutter-pi, ivi-homescreen). It creates signing
keys, packs a built Flutter app into a reproducible, signed `.homing` package, and verifies and
inspects packages.

On the device, [homing-agent](https://github.com/trevortomlin/homing-agent) installs the
packages into A/B slots and rolls back automatically if a new version fails its health check.
The package format is implemented in
[homing-format](https://github.com/trevortomlin/homing-format).

homing is an independent project, not affiliated with Google or the Flutter project.

> **Status: pre-alpha.** Commands and the package format may still change in incompatible ways.

## Installation

You need Rust 1.85 or newer. The tool is not on crates.io yet:

```sh
cargo install --locked --git https://github.com/trevortomlin/homing-cli
```

This installs the `homing` binary.

## Quick start

```console
$ homing init                         # writes homing.toml and homing/config/
$ homing keygen release --out-dir keys
secret key: keys/release.key  (keep private; never copy to devices)
public key: keys/prod-release.pub  (install in the device trust store)
key id:     d772bf35f0726cb8

$ homing pack --engine-json engine.json --key keys/release.key
com.example.myapp-1.0.0-aarch64-5a2a6a42.homing: com.example.myapp 1.0.0 (version_code 10000), 2 files, 1003 bytes payload, signed by d772bf35f0726cb8

$ homing verify com.example.myapp-1.0.0-aarch64-5a2a6a42.homing --keys trust --engine-json engine.json
PASS com.example.myapp 1.0.0 signed by d772bf35f0726cb8
```

- `engine.json` describes the target device image: Flutter engine commit, architecture,
  machine, graphics backends and base image version. Get it from the device
  (`/etc/homing/engine.json`) or from your image build.
- `trust/` is a directory holding the public keys that devices trust (`prod-*.pub`, and
  `dev-*.pub` on development devices).

Copy the package to a device and install it with `homingctl install <file> --wait` (see
homing-agent).

## Commands

| Command | Description |
|---|---|
| `homing init` | Write a `homing.toml` template and `homing/config/` in the current directory. |
| `homing keygen <name> [--dev] [--out-dir DIR]` | Create `<name>.key` (mode 0600) and `prod-<name>.pub`, or `dev-<name>.pub` with `--dev`. |
| `homing pack [-C DIR] [-o FILE] [--engine-json FILE] [--key FILE] [--no-compress]` | Collect the payload, write and sign the manifest, and archive it (zstd by default). |
| `homing verify <file> --keys DIR [--dev] [--engine-json FILE] [--floor N] [--json]` | Full verification: signature, manifest, hashes and, with `--engine-json`, device compatibility. Prints `PASS` or the error code. |
| `homing inspect <file> [--json]` | Print the manifest without checking trust. |

Run `homing <command> --help` for details.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage or configuration error |
| 2 | Verification failed. The error code (`E_SIG`, `E_HASH`, `E_ENGINE`, `E_ROLLBACK`, ...) goes to stdout, or into the JSON with `--json`. |
| 3 | I/O error |

```console
$ homing verify app.homing --keys trust --json
{"id":"com.example.myapp","key_id":"d772bf35f0726cb8","result":"PASS","version_code":10000}
$ homing verify app.homing --keys trust --floor 20000
FAIL E_ROLLBACK
```

## Project file: `homing.toml`

`homing init` writes this next to your app's `pubspec.yaml`:

```toml
[app]
id = "com.example.myapp"        # reverse-DNS app id
name = "My App"
version = "1.0.0"
# version_code = 10000          # default: major*10000 + minor*100 + patch; must only grow
# min_version_code = 0          # raises the device's anti-rollback floor once committed
# build_mode = "release"        # release | profile | debug (debug: dev-mode devices only)
# permissions = []              # confirm_updates, observe_updates

[runtime]
embedder = "flutter-pi"         # flutter-pi | ivi-homescreen
args = []                       # extra embedder args; devices only pass allowlisted ones

[compat]
arch = "aarch64"
machines = ["raspberrypi3-64"]
graphics = ["drm-gles"]
# base_image_min = "1.0.0"
# engine_hash = "..."           # usually supplied by --engine-json

[health]
mode = "ready"                  # ready: the app calls app.ready; alive: it stays up for trial_s
ready_timeout_s = 30
trial_s = 60

[paths]
bundle = "build/linux-embedded/bundle"   # contains flutter_assets/ and lib/libapp.so
config = "homing/config"                 # shipped to the device as the app's config dir
# sbom = "sbom"                          # optional, e.g. an SPDX JSON file
```

### Preparing the bundle for flutter-pi

`paths.bundle` must contain `flutter_assets/`, plus `lib/libapp.so` (the AOT snapshot) for
release and profile builds. With [flutterpi_tool](https://pub.dev/packages/flutterpi_tool)
0.12, the release output is one flat directory that holds both the assets and the engine:

```sh
flutterpi_tool build --arch=arm64 --cpu=generic --release
out=build/flutter-pi/aarch64-generic
mkdir -p build/linux-embedded/bundle/lib
rsync -a --exclude app.so --exclude libflutter_engine.so --exclude icudtl.dat \
      --exclude flutter-pi --exclude '.*' --exclude /linux/ \
      "$out/" build/linux-embedded/bundle/flutter_assets/
cp "$out/app.so" build/linux-embedded/bundle/lib/libapp.so
```

The engine (`libflutter_engine.so`, `icudtl.dat`) belongs to the device image, not to the
package. The engine commit in `engine.json` must match the one the app was compiled against
(`bin/internal/engine.version` in the Flutter SDK). Otherwise the package is rejected with
`E_ENGINE`.

## Signing in CI

Never commit secret keys. In CI, pass the key's base64 seed (the second field of the `.key`
file) through a secret:

```sh
HOMING_SIGNING_KEY="$SIGNING_SEED" homing pack --engine-json engine.json
```

## Reproducible builds

`homing pack` is deterministic: entries are sorted and owners and mtimes are fixed. Set
`SOURCE_DATE_EPOCH` and the same inputs give byte-identical packages.

## Roadmap

- [x] `init`, `keygen`, `pack`, `verify`, `inspect`
- [ ] Prebuilt release binaries (Linux x86_64/aarch64, macOS)
- [ ] `sign`, `bundle` (multi-app `.homingb`), `index add` (signed update index), `usb-stage`, `report`, `push`
- [ ] `build`: wrap the Flutter AOT build for the target, then pack

## Minimum supported Rust version

Rust **1.85** (edition 2024). CI tests stable and 1.85.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues
privately, as described in [SECURITY.md](SECURITY.md). This project follows a
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under the [MIT License](LICENSE).
