//! CLI round trip: keygen -> pack -> verify -> inspect, plus failure exit codes.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;

const ENGINE: &str = "0123456789abcdef0123456789abcdef01234567";

fn homing(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("homing").unwrap();
    c.current_dir(dir)
        .env_remove("HOMING_SIGNING_KEY")
        .env_remove("SOURCE_DATE_EPOCH");
    c
}

fn setup(dir: &Path) {
    let app = dir.join("app");
    fs::create_dir_all(app.join("bundle/flutter_assets")).unwrap();
    fs::create_dir_all(app.join("bundle/lib")).unwrap();
    fs::create_dir_all(app.join("homing/config")).unwrap();
    fs::write(app.join("bundle/flutter_assets/AssetManifest.json"), "{}").unwrap();
    fs::write(app.join("bundle/lib/libapp.so"), vec![1u8; 4096]).unwrap();
    fs::write(app.join("homing/config/app.json"), r#"{"greeting":"hi"}"#).unwrap();
    fs::write(
        app.join("homing.toml"),
        r#"
[app]
id = "com.example.hello"
name = "Hello"
version = "1.2.3"

[runtime]
embedder = "flutter-pi"

[compat]
arch = "aarch64"
machines = ["raspberrypi3-64"]

[paths]
bundle = "bundle"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("engine.json"),
        format!(
            r#"{{"schema":1,"engine_hash":"{ENGINE}","arch":"aarch64","machine":"raspberrypi3-64","graphics":["drm-gles"],"base_image_version":"1.0.0"}}"#
        ),
    )
    .unwrap();
    fs::create_dir(dir.join("keys")).unwrap();
    homing(dir)
        .args(["keygen", "release", "--out-dir", "keys"])
        .assert()
        .success();
}

fn pack(dir: &Path, out: &str) {
    homing(dir)
        .args([
            "pack",
            "-C",
            "app",
            "--engine-json",
            "engine.json",
            "--key",
            "keys/release.key",
            "-o",
            out,
        ])
        .assert()
        .success();
}

#[test]
fn round_trip() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    setup(dir);
    pack(dir, "a.homing");

    let out = homing(dir)
        .args([
            "verify",
            "a.homing",
            "--keys",
            "keys",
            "--engine-json",
            "engine.json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("PASS com.example.hello 1.2.3")
    );

    let out = homing(dir)
        .args(["inspect", "a.homing", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let m: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(m["version_code"], 10203);
    assert_eq!(m["files"].as_array().unwrap().len(), 3);

    // Human-readable output uses the same names as homing.toml.
    let out = homing(dir)
        .args(["inspect", "a.homing"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("build mode:    release"), "{text}");
    assert!(text.contains("embedder:      flutter-pi"), "{text}");
    assert!(text.contains("health:        ready"), "{text}");

    // Reproducible.
    pack(dir, "b.homing");
    assert_eq!(
        fs::read(dir.join("a.homing")).unwrap(),
        fs::read(dir.join("b.homing")).unwrap()
    );
}

#[test]
fn verify_failures_exit_2_with_code() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    setup(dir);
    pack(dir, "a.homing");

    // Wrong engine.
    let other = fs::read_to_string(dir.join("engine.json"))
        .unwrap()
        .replace(ENGINE, &"f".repeat(40));
    fs::write(dir.join("other.json"), other).unwrap();
    let o = homing(dir)
        .args([
            "verify",
            "a.homing",
            "--keys",
            "keys",
            "--engine-json",
            "other.json",
            "--json",
        ])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&o).unwrap();
    assert_eq!(v["code"], "E_ENGINE");

    // Untrusted key (keygen also creates a missing --out-dir, including parents).
    homing(dir)
        .args(["keygen", "x", "--out-dir", "other/keys"])
        .assert()
        .success();
    let o = homing(dir)
        .args(["verify", "a.homing", "--keys", "other/keys"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(o).unwrap().trim(), "FAIL E_KEY_UNKNOWN");

    // Floor.
    homing(dir)
        .args(["verify", "a.homing", "--keys", "keys", "--floor", "99999"])
        .assert()
        .code(2);

    // Tampered (uncompressed so we can flip a payload byte).
    homing(dir)
        .args([
            "pack",
            "-C",
            "app",
            "--engine-json",
            "engine.json",
            "--key",
            "keys/release.key",
            "-o",
            "p.homing",
            "--no-compress",
        ])
        .assert()
        .success();
    let mut bytes = fs::read(dir.join("p.homing")).unwrap();
    let pos = bytes.windows(4).rposition(|w| w == [1, 1, 1, 1]).unwrap();
    bytes[pos] = 2;
    fs::write(dir.join("p.homing"), bytes).unwrap();
    let o = homing(dir)
        .args(["verify", "p.homing", "--keys", "keys"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(o).unwrap().trim(), "FAIL E_HASH");
}

#[test]
fn usage_and_missing_key_errors() {
    let d = tempfile::tempdir().unwrap();
    homing(d.path()).arg("frobnicate").assert().code(1);
    setup(d.path());
    homing(d.path())
        .args(["pack", "-C", "app", "--engine-json", "engine.json"])
        .assert()
        .code(3);
}
