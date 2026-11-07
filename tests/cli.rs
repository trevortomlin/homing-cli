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
    // No key given at all: a configuration error.
    homing(d.path())
        .args(["pack", "-C", "app", "--engine-json", "engine.json"])
        .assert()
        .code(1);
    // A key file that cannot be read: an I/O error.
    homing(d.path())
        .args([
            "pack",
            "-C",
            "app",
            "--engine-json",
            "engine.json",
            "--key",
            "nope.key",
        ])
        .assert()
        .code(3);
}

fn set_version(dir: &Path, version: &str) {
    let p = dir.join("app/homing.toml");
    let t = fs::read_to_string(&p).unwrap();
    let t = t
        .lines()
        .map(|l| {
            if l.starts_with("version = ") {
                format!("version = \"{version}\"")
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(p, t).unwrap();
}

fn stdout(c: &mut Command, code: i32) -> String {
    String::from_utf8(c.assert().code(code).get_output().stdout.clone()).unwrap()
}

#[test]
fn index_add_verify_select_remove() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    setup(dir);
    fs::create_dir(dir.join("site")).unwrap();
    pack(dir, "site/v1.homing");
    set_version(dir, "1.3.0");
    pack(dir, "site/v2.homing");
    let key = ["--key", "keys/release.key"];

    // Create the index with both versions, verifying the packages first.
    homing(dir)
        .args([
            "index",
            "add",
            "site/index.json",
            "site/v1.homing",
            "site/v2.homing",
        ])
        .args(["--keys", "keys"])
        .args(key)
        .env("SOURCE_DATE_EPOCH", "1793966400")
        .assert()
        .success();
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("site/index.json")).unwrap()).unwrap();
    assert_eq!(doc["generated"], "2026-11-06T12:00:00Z");
    assert_eq!(doc["apps"][0]["version_code"], 10300, "newest first");
    assert_eq!(doc["apps"][0]["builds"][0]["url"], "v2.homing");
    assert_eq!(doc["apps"][1]["version_code"], 10203);

    // Verify and select: a device on 1.2.3 gets 1.3.0.
    let out = stdout(
        homing(dir).args([
            "index",
            "verify",
            "site/index.json",
            "--keys",
            "keys",
            "--engine-json",
            "engine.json",
            "--app",
            "com.example.hello",
            "--installed",
            "10203",
            "--json",
        ]),
        0,
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["result"], "PASS");
    assert_eq!(v["selection"]["result"], "update");
    assert_eq!(v["selection"]["url"], "v2.homing");

    // Re-adding the same package replaces its build instead of duplicating it.
    homing(dir)
        .args([
            "index",
            "add",
            "site/index.json",
            "site/v2.homing",
            "--url-prefix",
            "pkgs/",
        ])
        .args(key)
        .assert()
        .success();
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("site/index.json")).unwrap()).unwrap();
    assert_eq!(doc["apps"][0]["builds"].as_array().unwrap().len(), 1);
    assert_eq!(doc["apps"][0]["builds"][0]["url"], "pkgs/v2.homing");

    // Remove a version.
    homing(dir)
        .args([
            "index",
            "remove",
            "site/index.json",
            "com.example.hello",
            "10300",
        ])
        .args(key)
        .assert()
        .success();
    let out = stdout(homing(dir).args(["index", "show", "site/index.json"]), 0);
    assert!(
        out.contains("com.example.hello 1.2.3 (version_code 10203)"),
        "{out}"
    );
    assert!(!out.contains("10300"), "{out}");
    // Removing it again is a usage error.
    homing(dir)
        .args([
            "index",
            "remove",
            "site/index.json",
            "com.example.hello",
            "10300",
        ])
        .args(key)
        .assert()
        .code(1);
}

#[test]
fn index_tampering_is_detected_and_never_re_signed() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    setup(dir);
    pack(dir, "v1.homing");
    let key = ["--key", "keys/release.key"];
    homing(dir)
        .args(["index", "add", "index.json", "v1.homing"])
        .args(key)
        .assert()
        .success();
    let doc = fs::read_to_string(dir.join("index.json")).unwrap();
    fs::write(
        dir.join("index.json"),
        doc.replace("v1.homing", "evil.homing"),
    )
    .unwrap();

    let out = stdout(
        homing(dir).args(["index", "verify", "index.json", "--keys", "keys"]),
        2,
    );
    assert_eq!(out.trim(), "FAIL E_SIG");
    // Editing a tampered index is refused, so the change is never signed by accident...
    homing(dir)
        .args(["index", "add", "index.json", "v1.homing"])
        .args(key)
        .assert()
        .code(1)
        .stderr(predicates::str::contains("does not verify"));
    // ...unless explicitly forced.
    homing(dir)
        .args(["index", "add", "index.json", "v1.homing", "--force"])
        .args(key)
        .assert()
        .success();
    homing(dir)
        .args(["index", "verify", "index.json", "--keys", "keys"])
        .assert()
        .success();

    // An untrusted signer and a missing signature file are rejected too.
    homing(dir)
        .args(["keygen", "other", "--out-dir", "other"])
        .assert()
        .success();
    homing(dir)
        .args([
            "index",
            "add",
            "index.json",
            "v1.homing",
            "--key",
            "other/other.key",
        ])
        .assert()
        .code(1);
    fs::remove_file(dir.join("index.json.sig")).unwrap();
    homing(dir)
        .args(["index", "add", "index.json", "v1.homing"])
        .args(key)
        .assert()
        .code(1)
        .stderr(predicates::str::contains("--force"));
}

#[test]
fn index_add_rejects_untrusted_packages_with_keys() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    setup(dir);
    pack(dir, "v1.homing");
    homing(dir)
        .args(["keygen", "other", "--out-dir", "other"])
        .assert()
        .success();
    let out = homing(dir)
        .args(["index", "add", "index.json", "v1.homing", "--keys", "other"])
        .args(["--key", "keys/release.key"])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8(out).unwrap().contains("E_KEY_UNKNOWN"));
    assert!(!dir.join("index.json").exists());
}
