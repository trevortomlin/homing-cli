//! `homing verify` and `homing index verify` against every homing-format test vector: the CLI
//! must report exactly the expected result and error code (and exit code) for each.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;

/// `test-vectors/` of the homing-format version this crate depends on: from
/// `$HOMING_FORMAT_VECTORS` if set (e.g. when the test binary runs on a device without cargo),
/// else located with `cargo metadata`.
fn vectors_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HOMING_FORMAT_VECTORS") {
        return PathBuf::from(dir);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(out.status.success(), "cargo metadata failed");
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    let manifest = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "homing-format")
        .expect("homing-format in metadata")["manifest_path"]
        .as_str()
        .unwrap()
        .to_owned();
    let dir = PathBuf::from(manifest)
        .parent()
        .unwrap()
        .join("test-vectors");
    assert!(
        dir.join("vectors.json").is_file(),
        "{} has no vectors",
        dir.display()
    );
    dir
}

fn homing() -> Command {
    let mut c = Command::cargo_bin("homing").unwrap();
    c.env_remove("HOMING_SIGNING_KEY");
    c
}

fn run(c: &mut Command) -> (i32, Value) {
    let out = c.output().unwrap();
    let code = out.status.code().unwrap();
    let v = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("bad JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)));
    (code, v)
}

#[test]
fn package_vectors() {
    let dir = vectors_dir();
    let spec: Value =
        serde_json::from_slice(&std::fs::read(dir.join("vectors.json")).unwrap()).unwrap();
    let mut n = 0;
    for v in spec["packages"].as_array().unwrap() {
        let mut c = homing();
        c.arg("verify")
            .arg(dir.join(v["file"].as_str().unwrap()))
            .arg("--keys")
            .arg(dir.join("trust"))
            .args(["--floor", &v["floor"].to_string(), "--json"]);
        if v["dev_mode"].as_bool().unwrap() {
            c.arg("--dev");
        }
        if v["check_device"].as_bool().unwrap() {
            c.arg("--engine-json").arg(dir.join("engine.json"));
        }
        let (code, out) = run(&mut c);
        let expect = v["expect"].as_str().unwrap();
        let what = v["description"].as_str().unwrap();
        if expect == "PASS" {
            assert_eq!(
                (code, out["result"].as_str()),
                (0, Some("PASS")),
                "{what}: {out}"
            );
        } else {
            assert_eq!(code, 2, "{what}: {out}");
            assert_eq!(out["code"].as_str(), Some(expect), "{what}: {out}");
        }
        n += 1;
    }
    assert!(n >= 30, "only {n} package vectors");
}

#[test]
fn index_vectors() {
    let dir = vectors_dir();
    let spec: Value =
        serde_json::from_slice(&std::fs::read(dir.join("vectors.json")).unwrap()).unwrap();
    for v in spec["indexes"].as_array().unwrap() {
        let index = dir.join(v["file"].as_str().unwrap());
        // `homing index verify` reads the signature from <index>.sig.
        assert_eq!(
            dir.join(v["sig"].as_str().unwrap()),
            PathBuf::from(format!("{}.sig", index.display()))
        );
        let what = v["description"].as_str().unwrap();
        let verify = || {
            let mut c = homing();
            c.args(["index", "verify"])
                .arg(&index)
                .arg("--keys")
                .arg(dir.join("trust"))
                .arg("--json");
            c
        };
        let (code, out) = run(&mut verify());
        let expect = v["expect"].as_str().unwrap();
        if expect != "PASS" {
            assert_eq!(
                (code, out["code"].as_str()),
                (2, Some(expect)),
                "{what}: {out}"
            );
            continue;
        }
        assert_eq!(
            (code, out["result"].as_str()),
            (0, Some("PASS")),
            "{what}: {out}"
        );
        for s in v["select"].as_array().unwrap() {
            let mut c = verify();
            c.arg("--engine-json")
                .arg(dir.join("engine.json"))
                .args(["--app", s["app"].as_str().unwrap()])
                .args(["--installed", &s["installed_version_code"].to_string()])
                .args(["--floor", &s["floor"].to_string()]);
            let (code, out) = run(&mut c);
            assert_eq!(code, 0);
            let (got, want) = (&out["selection"], &s["expect"]);
            assert_eq!(got["result"], want["result"], "{s}: {out}");
            for k in ["version_code", "url", "newest_version_code"] {
                if !want[k].is_null() {
                    assert_eq!(got[k], want[k], "{k} for {s}: {out}");
                }
            }
        }
    }
}
