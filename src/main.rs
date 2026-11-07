//! `homing`: host tool for homing app packages.

mod index;
mod project;

use std::fs;
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use homing_format::manifest::{BuildMode, Manifest};
use homing_format::{DeviceInfo, PackageWriter, SecretKey, TrustStore, VerifyOptions};

/// Exit codes (documented in the README).
const EXIT_USAGE: u8 = 1;
const EXIT_VERIFY: u8 = 2;
const EXIT_IO: u8 = 3;

#[derive(Parser)]
#[command(
    name = "homing",
    version,
    about = "Create, sign and verify homing app packages"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a homing.toml template and homing/config/ in the current directory.
    Init,
    /// Generate a signing key pair: <name>.key (0600) and prod-<name>.pub / dev-<name>.pub.
    Keygen {
        /// Key name, used in the file names and as the public key comment.
        name: String,
        /// Create a dev key (only honored by devices in dev mode).
        #[arg(long)]
        dev: bool,
        /// Output directory.
        #[arg(long, default_value = ".")]
        out_dir: PathBuf,
    },
    /// Collect the payload, write the manifest, sign and archive.
    Pack {
        /// Project directory containing homing.toml.
        #[arg(short = 'C', long, default_value = ".")]
        project: PathBuf,
        /// Output file (default: <id>-<version>-<arch>-<engine8>.homing in the current dir).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// engine.json of the target image (supplies engine_hash).
        #[arg(long)]
        engine_json: Option<PathBuf>,
        /// Secret key file. Alternatively set HOMING_SIGNING_KEY to the base64 seed.
        #[arg(long)]
        key: Option<PathBuf>,
        /// Write an uncompressed tar.
        #[arg(long)]
        no_compress: bool,
    },
    /// Fully verify a package. Prints PASS or the error code.
    Verify {
        /// Package file (.homing).
        file: PathBuf,
        /// Trust store directory (prod-*.pub, dev-*.pub).
        #[arg(long)]
        keys: PathBuf,
        /// Honor dev-*.pub keys.
        #[arg(long)]
        dev: bool,
        /// Check compat against this engine.json.
        #[arg(long)]
        engine_json: Option<PathBuf>,
        /// Anti-rollback floor.
        #[arg(long, default_value_t = 0)]
        floor: u64,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create, update, verify and show signed update indexes for HTTP sources.
    Index {
        #[command(subcommand)]
        cmd: index::IndexCmd,
    },
    /// Print a package's manifest without trust checks.
    Inspect {
        /// Package file (.homing).
        file: PathBuf,
        /// Print the manifest as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(EXIT_USAGE)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    match run(cli.cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(exit_code(&e))
        }
    }
}

fn run(cmd: Cmd) -> Result<ExitCode> {
    match cmd {
        Cmd::Init => init(),
        Cmd::Keygen { name, dev, out_dir } => keygen(&name, dev, &out_dir),
        Cmd::Pack {
            project,
            output,
            engine_json,
            key,
            no_compress,
        } => pack(
            &project,
            output,
            engine_json.as_deref(),
            key.as_deref(),
            no_compress,
        ),
        Cmd::Verify {
            file,
            keys,
            dev,
            engine_json,
            floor,
            json,
        } => verify(&file, &keys, dev, engine_json.as_deref(), floor, json),
        Cmd::Inspect { file, json } => inspect(&file, json),
        Cmd::Index { cmd } => index::run(cmd),
    }
}

fn init() -> Result<ExitCode> {
    let p = Path::new(project::FILE);
    if p.exists() {
        bail!("{} already exists", p.display());
    }
    fs::write(p, project::TEMPLATE)?;
    fs::create_dir_all("homing/config")?;
    println!("wrote {} and homing/config/", p.display());
    Ok(ExitCode::SUCCESS)
}

fn keygen(name: &str, dev: bool, out_dir: &Path) -> Result<ExitCode> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        bail!("key name may contain only letters, digits, '-' and '_'");
    }
    fs::create_dir_all(out_dir).with_context(|| format!("cannot create {}", out_dir.display()))?;
    let sk = SecretKey::generate()?;
    let key_path = out_dir.join(format!("{name}.key"));
    let role = if dev { "dev" } else { "prod" };
    let pub_path = out_dir.join(format!("{role}-{name}.pub"));
    if pub_path.exists() {
        bail!("{} already exists", pub_path.display());
    }
    sk.write_new(&key_path)?;
    fs::write(&pub_path, sk.public_key(name).to_file_string())?;
    println!(
        "secret key: {}  (keep private; never copy to devices)",
        key_path.display()
    );
    println!(
        "public key: {}  (install in the device trust store)",
        pub_path.display()
    );
    println!("key id:     {}", sk.key_id());
    Ok(ExitCode::SUCCESS)
}

/// Exit code for an error: 2 for verification failures (a homing error code other than
/// `E_IO`), 3 for I/O errors, 1 for everything else (bad arguments or configuration).
fn exit_code(e: &anyhow::Error) -> u8 {
    for cause in e.chain() {
        if let Some(fe) = cause.downcast_ref::<homing_format::Error>() {
            return if fe.code() == homing_format::ErrorCode::Io {
                EXIT_IO
            } else {
                EXIT_VERIFY
            };
        }
        if cause.is::<std::io::Error>() {
            return EXIT_IO;
        }
    }
    EXIT_USAGE
}

/// `SOURCE_DATE_EPOCH` (reproducible builds), if set.
fn source_date_epoch() -> Result<Option<u64>> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(v) => Ok(Some(
            v.trim()
                .parse()
                .context("SOURCE_DATE_EPOCH is not an integer")?,
        )),
        Err(_) => Ok(None),
    }
}

fn load_key(key: Option<&Path>) -> Result<SecretKey> {
    if let Some(p) = key {
        return Ok(SecretKey::read(p)?);
    }
    match std::env::var("HOMING_SIGNING_KEY") {
        Ok(v) => Ok(SecretKey::from_base64_seed(&v)?),
        Err(_) => bail!("no signing key: pass --key <file> or set HOMING_SIGNING_KEY"),
    }
}

fn pack(
    dir: &Path,
    output: Option<PathBuf>,
    engine_json: Option<&Path>,
    key: Option<&Path>,
    no_compress: bool,
) -> Result<ExitCode> {
    let proj = project::Project::load(dir)?;
    let engine_hash = match (engine_json, &proj.compat.engine_hash) {
        (Some(p), _) => DeviceInfo::read(p)?.engine_hash,
        (None, Some(h)) => h.clone(),
        (None, None) => bail!("engine hash unknown: pass --engine-json or set compat.engine_hash"),
    };
    let key = load_key(key)?;
    let manifest = proj.manifest(engine_hash)?;

    let bundle = dir.join(&proj.paths.bundle);
    if !bundle.join("flutter_assets").is_dir() {
        bail!(
            "{}/flutter_assets not found (build the app first)",
            bundle.display()
        );
    }
    if matches!(manifest.build_mode, BuildMode::Release | BuildMode::Profile)
        && !bundle.join("lib/libapp.so").is_file()
    {
        bail!(
            "{}/lib/libapp.so not found (needed for release/profile)",
            bundle.display()
        );
    }
    if !bundle.join("icudtl.dat").is_file() {
        eprintln!("note: no icudtl.dat in bundle; the device's engine copy will be used");
    }

    let mtime = source_date_epoch()?.unwrap_or(0);
    let mut w = PackageWriter::new(mtime).compression(if no_compress { None } else { Some(12) });
    w.add_dir(&bundle, "bundle")?;
    let config = dir.join(&proj.paths.config);
    if config.is_dir() {
        w.add_dir(&config, "config")?;
    }
    if let Some(sbom) = &proj.paths.sbom {
        w.add_dir(&dir.join(sbom), "sbom")?;
    }

    let out = output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}-{}-{}-{}.homing",
            manifest.id,
            manifest.version,
            manifest.compat.arch.as_str(),
            &manifest.compat.engine_hash[..8]
        ))
    });
    let tmp = out.with_extension("homing.tmp");
    let res = (|| -> Result<Manifest> {
        let f =
            fs::File::create(&tmp).with_context(|| format!("cannot create {}", tmp.display()))?;
        let mut bw = BufWriter::new(f);
        let m = w.write(manifest, &key, &mut bw)?;
        bw.flush()?;
        bw.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        fs::rename(&tmp, &out)?;
        Ok(m)
    })();
    let m = res.inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    let total: u64 = m.files.iter().map(|f| f.size).sum();
    println!(
        "{}: {} {} (version_code {}), {} files, {} bytes payload, signed by {}",
        out.display(),
        m.id,
        m.version,
        m.version_code,
        m.files.len(),
        total,
        key.key_id()
    );
    Ok(ExitCode::SUCCESS)
}

fn verify(
    file: &Path,
    keys: &Path,
    dev: bool,
    engine_json: Option<&Path>,
    floor: u64,
    json: bool,
) -> Result<ExitCode> {
    let trust = TrustStore::load_dir(keys, dev)?;
    let device = engine_json.map(DeviceInfo::read).transpose()?;
    let f = fs::File::open(file).with_context(|| format!("cannot open {}", file.display()))?;
    let res = homing_format::verify(
        f,
        match &device {
            Some(d) => VerifyOptions::new(&trust).device(d).floor(floor),
            None => VerifyOptions::new(&trust).floor(floor),
        },
    );
    match res {
        Ok(v) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"result": "PASS", "id": v.manifest.id,
                        "version_code": v.manifest.version_code, "key_id": v.key_id.to_string()})
                );
            } else {
                println!(
                    "PASS {} {} signed by {}",
                    v.manifest.id, v.manifest.version, v.key_id
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"result": "FAIL", "code": e.code().as_str(), "message": e.message()})
                );
            } else {
                println!("FAIL {}", e.code());
            }
            eprintln!("{e}");
            Ok(ExitCode::from(EXIT_VERIFY))
        }
    }
}

/// The name a value has in manifest.json / homing.toml (its serde name), e.g. `flutter-pi`.
fn wire_name<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => "?".into(),
    }
}

fn inspect(file: &Path, json: bool) -> Result<ExitCode> {
    let f = fs::File::open(file).with_context(|| format!("cannot open {}", file.display()))?;
    let (m, s) = homing_format::inspect(f)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&m)?);
        return Ok(ExitCode::SUCCESS);
    }
    let total: u64 = m.files.iter().map(|f| f.size).sum();
    println!("id:            {}", m.id);
    println!("name:          {}", m.name);
    println!(
        "version:       {} (code {}, min {})",
        m.version, m.version_code, m.min_version_code
    );
    println!("build mode:    {}", wire_name(&m.build_mode));
    println!("embedder:      {}", wire_name(&m.runtime.embedder));
    if !m.runtime.args.is_empty() {
        println!("args:          {}", m.runtime.args.join(" "));
    }
    println!("engine:        {}", m.compat.engine_hash);
    println!("arch:          {}", m.compat.arch.as_str());
    println!("machines:      {}", m.compat.machines.join(", "));
    let h = &m.health;
    match wire_name(&h.mode).as_str() {
        "ready" => println!(
            "health:        ready (app.ready within {} s)",
            h.ready_timeout_s
        ),
        mode => println!("health:        {mode} (stay up for {} s)", h.trial_s),
    }
    println!("signed by:     {} (not verified)", s.key_id);
    println!("files:         {} ({} bytes)", m.files.len(), total);
    for f in &m.files {
        println!(
            "  {} {:>10}  {}{}",
            &f.sha256[..12],
            f.size,
            f.path,
            if f.exec { " *" } else { "" }
        );
    }
    Ok(ExitCode::SUCCESS)
}
