//! `homing.toml` project file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use homing_format::manifest::{
    Arch, BuildMode, Compat, Embedder, Graphics, Health, Manifest, Permission, Runtime,
};
use serde::Deserialize;

/// File name of the project file.
pub const FILE: &str = "homing.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub app: App,
    pub runtime: RuntimeCfg,
    pub compat: CompatCfg,
    #[serde(default)]
    pub health: Option<Health>,
    #[serde(default)]
    pub paths: Paths,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Default: derived from `version` as major*10000 + minor*100 + patch.
    pub version_code: Option<u64>,
    #[serde(default)]
    pub min_version_code: u64,
    #[serde(default = "release")]
    pub build_mode: BuildMode,
    #[serde(default)]
    pub permissions: Vec<Permission>,
}

fn release() -> BuildMode {
    BuildMode::Release
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCfg {
    pub embedder: Embedder,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatCfg {
    pub arch: Arch,
    #[serde(default)]
    pub machines: Vec<String>,
    pub graphics: Option<Vec<Graphics>>,
    pub base_image_min: Option<String>,
    /// Usually supplied by `--engine-json`; set here only if you know it.
    pub engine_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    #[serde(default = "default_bundle")]
    pub bundle: PathBuf,
    #[serde(default = "default_config")]
    pub config: PathBuf,
    #[serde(default)]
    pub sbom: Option<PathBuf>,
}

fn default_bundle() -> PathBuf {
    "build/linux-embedded/bundle".into()
}
fn default_config() -> PathBuf {
    "homing/config".into()
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            bundle: default_bundle(),
            config: default_config(),
            sbom: None,
        }
    }
}

impl Project {
    pub fn load(dir: &Path) -> Result<Self> {
        let p = dir.join(FILE);
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("cannot read {}", p.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid {}", p.display()))
    }

    pub fn version_code(&self) -> Result<u64> {
        if let Some(vc) = self.app.version_code {
            return Ok(vc);
        }
        let v = semver::Version::parse(&self.app.version)
            .with_context(|| format!("app.version {:?} is not semver", self.app.version))?;
        if v.minor > 99 || v.patch > 99 {
            bail!("cannot derive version_code from {v} (minor/patch > 99); set app.version_code");
        }
        Ok(v.major * 10_000 + v.minor * 100 + v.patch)
    }

    /// Build the manifest (without `files`) for an engine hash.
    pub fn manifest(&self, engine_hash: String) -> Result<Manifest> {
        Ok(Manifest {
            schema: 1,
            id: self.app.id.clone(),
            name: self.app.name.clone(),
            version: self.app.version.clone(),
            version_code: self.version_code()?,
            min_version_code: self.app.min_version_code,
            build_mode: self.app.build_mode,
            runtime: Runtime {
                embedder: self.runtime.embedder,
                args: self.runtime.args.clone(),
                env: self.runtime.env.clone(),
            },
            compat: Compat {
                engine_hash,
                arch: self.compat.arch,
                machines: self.compat.machines.clone(),
                graphics: self.compat.graphics.clone(),
                base_image_min: self.compat.base_image_min.clone(),
            },
            health: self.health.unwrap_or_default(),
            permissions: self.app.permissions.clone(),
            files: vec![],
            created: None,
        })
    }
}

pub const TEMPLATE: &str = r#"# homing project file. See https://github.com/trevortomlin/homing-cli
[app]
id = "com.example.myapp"
name = "My App"
version = "1.0.0"
# version_code = 10000      # default: major*10000 + minor*100 + patch
# min_version_code = 0      # raises the device anti-rollback floor once committed
# build_mode = "release"    # release | profile | debug (debug: dev-mode devices only)
# permissions = []          # confirm_updates, observe_updates

[runtime]
embedder = "flutter-pi"     # flutter-pi | ivi-homescreen
args = []

[compat]
arch = "aarch64"
machines = ["raspberrypi3-64"]
graphics = ["drm-gles"]
# base_image_min = "1.0.0"
# engine_hash: pass --engine-json <engine.json from the target image> to `homing pack`

[health]
mode = "ready"              # ready: app must call app.ready; alive: must stay up for trial_s
ready_timeout_s = 30
trial_s = 60

[paths]
bundle = "build/linux-embedded/bundle"   # contains flutter_assets/ and lib/libapp.so
config = "homing/config"
"#;
