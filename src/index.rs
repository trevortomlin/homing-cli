//! `homing index`: create, update, verify and show signed update indexes (`index.json` +
//! `index.json.sig`) for HTTP update sources.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use homing_format::{
    DeviceInfo, Index, IndexApp, IndexBuild, KeyRole, SecretKey, Selection, TrustStore,
    VerifyOptions, verify_index,
};

use crate::{EXIT_VERIFY, load_key};

#[derive(Subcommand)]
pub enum IndexCmd {
    /// Add packages to an index (created if missing), then re-sign it.
    ///
    /// Each package becomes a build of its app version. A build for the same device
    /// configuration (engine, arch, machines, graphics, base image) is replaced.
    Add {
        /// Index file (index.json); index.json.sig is written beside it.
        index: PathBuf,
        /// Package files (.homing).
        #[arg(required = true)]
        packages: Vec<PathBuf>,
        /// Prefix for each package's URL in the index: a path relative to the index
        /// (e.g. "packages/") or an absolute http(s) URL. Default: the file name only, for
        /// packages served next to the index.
        #[arg(long, default_value = "")]
        url_prefix: String,
        /// Trust store to fully verify each package against before adding it (recommended).
        #[arg(long)]
        keys: Option<PathBuf>,
        /// Index signing key file. Alternatively set HOMING_SIGNING_KEY to the base64 seed.
        #[arg(long)]
        key: Option<PathBuf>,
        /// Re-sign even if the existing index.json.sig does not verify (e.g. after a key change).
        #[arg(long)]
        force: bool,
    },
    /// Remove one app version from an index, then re-sign it.
    Remove {
        /// Index file (index.json).
        index: PathBuf,
        /// App id.
        id: String,
        /// Version code to remove.
        version_code: u64,
        /// Trust store with other keys allowed to have signed the existing index.
        #[arg(long)]
        keys: Option<PathBuf>,
        /// Index signing key file. Alternatively set HOMING_SIGNING_KEY to the base64 seed.
        #[arg(long)]
        key: Option<PathBuf>,
        /// Re-sign even if the existing index.json.sig does not verify (e.g. after a key change).
        #[arg(long)]
        force: bool,
    },
    /// Verify an index signature, and optionally show which build a device would pick.
    Verify {
        /// Index file (index.json); index.json.sig is read from beside it.
        index: PathBuf,
        /// Trust store directory (prod-*.pub, dev-*.pub).
        #[arg(long)]
        keys: PathBuf,
        /// Honor dev-*.pub keys.
        #[arg(long)]
        dev: bool,
        /// Device description; with --app, print the build this device would install.
        #[arg(long, requires = "app")]
        engine_json: Option<PathBuf>,
        /// App id to select an update for (needs --engine-json).
        #[arg(long, requires = "engine_json")]
        app: Option<String>,
        /// Version code currently installed on the device.
        #[arg(long, default_value_t = 0)]
        installed: u64,
        /// The device's anti-rollback floor for the app.
        #[arg(long, default_value_t = 0)]
        floor: u64,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print an index without checking its signature.
    Show {
        /// Index file (index.json).
        index: PathBuf,
        /// Print the index as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: IndexCmd) -> Result<ExitCode> {
    match cmd {
        IndexCmd::Add {
            index,
            packages,
            url_prefix,
            keys,
            key,
            force,
        } => add(
            &index,
            &packages,
            &url_prefix,
            keys.as_deref(),
            key.as_deref(),
            force,
        ),
        IndexCmd::Remove {
            index,
            id,
            version_code,
            keys,
            key,
            force,
        } => remove(
            &index,
            &id,
            version_code,
            keys.as_deref(),
            key.as_deref(),
            force,
        ),
        IndexCmd::Verify {
            index,
            keys,
            dev,
            engine_json,
            app,
            installed,
            floor,
            json,
        } => verify(
            &index,
            &keys,
            dev,
            engine_json.as_deref(),
            app.as_deref(),
            installed,
            floor,
            json,
        ),
        IndexCmd::Show { index, json } => show(&index, json),
    }
}

fn sig_path(index: &Path) -> PathBuf {
    let mut s = index.as_os_str().to_owned();
    s.push(".sig");
    PathBuf::from(s)
}

/// Read an existing index for editing, or start a new one.
///
/// If the index exists, its `index.json.sig` must verify against the signing key or a key in
/// `keys`, so that a modified index on the publishing side is never re-signed by accident.
/// `force` skips that check (for a deliberate key change).
fn load_for_edit(
    path: &Path,
    signing_key: &SecretKey,
    keys: Option<&Path>,
    force: bool,
) -> Result<Index> {
    let doc = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Index::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    if !force {
        let sig_file = sig_path(path);
        let sig = match fs::read(&sig_file) {
            Ok(sig) => sig,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
                "{} has no {}; pass --force to sign it anyway",
                path.display(),
                sig_file.display()
            ),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read {}", sig_file.display()));
            }
        };
        let mut trust = match keys {
            Some(k) => TrustStore::load_dir(k, false)?,
            None => TrustStore::new(false),
        };
        if trust.lookup(&signing_key.key_id()).is_err() {
            trust.add(signing_key.public_key("signing key"), KeyRole::Prod)?;
        }
        if let Err(e) = verify_index(&doc, &sig, &trust) {
            bail!(
                "{} does not verify ({e}): it was modified, or signed by a key that is neither \
                 the signing key nor in --keys. Check it, then pass --keys <dir> or --force",
                path.display()
            );
        }
    }
    Ok(Index::parse(&doc).with_context(|| format!("invalid {}", path.display()))?)
}

fn add(
    index_path: &Path,
    packages: &[PathBuf],
    url_prefix: &str,
    keys: Option<&Path>,
    key: Option<&Path>,
    force: bool,
) -> Result<ExitCode> {
    let key = load_key(key)?;
    let trust = keys.map(|k| TrustStore::load_dir(k, false)).transpose()?;
    let mut index = load_for_edit(index_path, &key, keys, force)?;
    for pkg in packages {
        let open = || fs::File::open(pkg).with_context(|| format!("cannot open {}", pkg.display()));
        let manifest = match &trust {
            Some(t) => {
                homing_format::verify(open()?, VerifyOptions::new(t))
                    .with_context(|| format!("{} failed verification", pkg.display()))?
                    .manifest
            }
            None => homing_format::inspect(open()?)?.0,
        };
        let (sha256, size) = homing_format::package::sha256_reader(open()?)?;
        let name = pkg
            .file_name()
            .and_then(|n| n.to_str())
            .context("package file name is not UTF-8")?;
        let c = &manifest.compat;
        let mut build = IndexBuild::new(
            c.engine_hash.clone(),
            c.arch,
            format!("{url_prefix}{name}"),
            sha256,
            size,
        );
        build.machines = c.machines.clone();
        build.graphics = c.graphics.clone();
        build.base_image_min = c.base_image_min.clone();

        let app = match index
            .apps
            .iter_mut()
            .find(|a| a.id == manifest.id && a.version_code == manifest.version_code)
        {
            Some(a) => {
                if a.version != manifest.version {
                    bail!(
                        "{}: version_code {} is already listed as version {}, not {}",
                        manifest.id,
                        manifest.version_code,
                        a.version,
                        manifest.version
                    );
                }
                a
            }
            None => {
                index.apps.push(IndexApp::new(
                    manifest.id.clone(),
                    manifest.version.clone(),
                    manifest.version_code,
                ));
                index.apps.last_mut().context("just pushed")?
            }
        };
        let same_config = |b: &IndexBuild| b.compat() == build.compat();
        let action = if let Some(slot) = app.builds.iter_mut().find(|b| same_config(b)) {
            *slot = build;
            "replaced"
        } else {
            app.builds.push(build);
            "added"
        };
        println!(
            "{action} {} {} (version_code {}) for {} {}",
            manifest.id,
            manifest.version,
            manifest.version_code,
            c.arch.as_str(),
            &c.engine_hash[..8]
        );
    }
    if trust.is_none() {
        eprintln!("note: packages were not verified; pass --keys to check their signatures");
    }
    write_signed(index_path, index, &key)
}

fn remove(
    index_path: &Path,
    id: &str,
    version_code: u64,
    keys: Option<&Path>,
    key: Option<&Path>,
    force: bool,
) -> Result<ExitCode> {
    let key = load_key(key)?;
    let mut index = load_for_edit(index_path, &key, keys, force)?;
    let before = index.apps.len();
    index
        .apps
        .retain(|a| !(a.id == id && a.version_code == version_code));
    if index.apps.len() == before {
        bail!(
            "{id} version_code {version_code} is not in {}",
            index_path.display()
        );
    }
    println!("removed {id} version_code {version_code}");
    write_signed(index_path, index, &key)
}

/// Sort for stable output, stamp `generated`, sign, and write both files atomically.
fn write_signed(index_path: &Path, mut index: Index, key: &SecretKey) -> Result<ExitCode> {
    index
        .apps
        .sort_by(|a, b| a.id.cmp(&b.id).then(b.version_code.cmp(&a.version_code)));
    index.generated = Some(rfc3339(crate::source_date_epoch()?.unwrap_or_else(now))?);
    let (doc, sig) = index.sign(key)?;
    write_atomic(index_path, &doc)?;
    write_atomic(&sig_path(index_path), &sig)?;
    println!(
        "{}: {} app versions, signed by {}",
        index_path.display(),
        index.apps.len(),
        key.key_id()
    );
    Ok(ExitCode::SUCCESS)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let res = (|| -> Result<()> {
        let mut f =
            fs::File::create(&tmp).with_context(|| format!("cannot create {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))?;
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

#[allow(clippy::too_many_arguments)] // mirrors the command-line flags
fn verify(
    index_path: &Path,
    keys: &Path,
    dev: bool,
    engine_json: Option<&Path>,
    app: Option<&str>,
    installed: u64,
    floor: u64,
    json: bool,
) -> Result<ExitCode> {
    let trust = TrustStore::load_dir(keys, dev)?;
    let doc =
        fs::read(index_path).with_context(|| format!("cannot read {}", index_path.display()))?;
    let sig_file = sig_path(index_path);
    let sig = fs::read(&sig_file).with_context(|| format!("cannot read {}", sig_file.display()))?;
    let verified = match verify_index(&doc, &sig, &trust) {
        Ok(v) => v,
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"result": "FAIL", "code": e.code().as_str(), "message": e.message()})
                );
            } else {
                println!("FAIL {}", e.code().as_str());
                eprintln!("{e}");
            }
            return Ok(ExitCode::from(EXIT_VERIFY));
        }
    };
    let selection = match (engine_json, app) {
        (Some(ej), Some(app)) => {
            let device = DeviceInfo::read(ej)?;
            Some(
                match verified.index.select(app, installed, floor, &device) {
                    Selection::Update { app, build } => serde_json::json!({
                    "result": "update", "version": app.version, "version_code": app.version_code,
                    "url": build.url, "sha256": build.sha256, "size": build.size }),
                    Selection::NoCompatibleBuild {
                        newest_version_code,
                    } => serde_json::json!({ "result": "no_compatible_build",
                                         "newest_version_code": newest_version_code }),
                    _ => serde_json::json!({ "result": "up_to_date" }),
                },
            )
        }
        _ => None,
    };
    if json {
        let mut out = serde_json::json!({"result": "PASS", "key_id": verified.key_id.to_string(),
                                         "app_versions": verified.index.apps.len()});
        if let Some(s) = selection {
            out["selection"] = s;
        }
        println!("{out}");
    } else {
        println!(
            "PASS {} app versions, signed by {}",
            verified.index.apps.len(),
            verified.key_id
        );
        if let Some(s) = selection {
            match s["result"].as_str() {
                Some("update") => println!(
                    "update: {} (version_code {}) from {}",
                    s["version"].as_str().unwrap_or("?"),
                    s["version_code"],
                    s["url"].as_str().unwrap_or("?")
                ),
                Some("no_compatible_build") => println!(
                    "newer versions exist (up to version_code {}), but none fits this device",
                    s["newest_version_code"]
                ),
                _ => println!("up to date"),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn show(index_path: &Path, json: bool) -> Result<ExitCode> {
    let doc =
        fs::read(index_path).with_context(|| format!("cannot read {}", index_path.display()))?;
    let index = Index::parse(&doc)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&index)?);
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "generated: {}  (signature not checked)",
        index.generated.as_deref().unwrap_or("-")
    );
    for a in &index.apps {
        println!("{} {} (version_code {})", a.id, a.version, a.version_code);
        for b in &a.builds {
            let machines = if b.machines.is_empty() {
                "any machine".to_owned()
            } else {
                b.machines.join(",")
            };
            println!(
                "  {} {} {}  {} ({} bytes)",
                b.arch.as_str(),
                &b.engine_hash[..8],
                machines,
                b.url,
                b.size
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format Unix seconds as an RFC 3339 UTC timestamp (`2026-11-06T12:00:00Z`).
pub fn rfc3339(secs: u64) -> Result<String> {
    let days = i64::try_from(secs / 86_400).context("timestamp out of range")?;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Ok(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::rfc3339;

    #[test]
    fn rfc3339_formats_known_dates() {
        assert_eq!(rfc3339(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400).unwrap(), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_793_966_400).unwrap(), "2026-11-06T12:00:00Z");
        assert_eq!(rfc3339(4_102_444_799).unwrap(), "2099-12-31T23:59:59Z");
    }
}
