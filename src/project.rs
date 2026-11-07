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
    /// Default: `name` from pubspec.yaml.
    pub name: Option<String>,
    /// Default: `version` from pubspec.yaml.
    pub version: Option<String>,
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
    /// Load `homing.toml` from `dir`. A missing `app.name` or `app.version` is taken from the
    /// Flutter app's `pubspec.yaml` in the same directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let p = dir.join(FILE);
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("cannot read {}", p.display()))?;
        let mut proj: Project =
            toml::from_str(&text).with_context(|| format!("invalid {}", p.display()))?;
        if proj.app.name.is_none() || proj.app.version.is_none() {
            let pubspec = dir.join("pubspec.yaml");
            let text = match std::fs::read_to_string(&pubspec) {
                Ok(t) => t,
                // Nothing to fall back to: a configuration error, not an I/O failure.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
                    "app.name or app.version is not set in {FILE}, and there is no {}",
                    pubspec.display()
                ),
                Err(e) => {
                    return Err(e).with_context(|| format!("cannot read {}", pubspec.display()));
                }
            };
            for (field, value) in [
                ("name", &mut proj.app.name),
                ("version", &mut proj.app.version),
            ] {
                if value.is_none() {
                    *value = Some(pubspec_field(&text, field).with_context(|| {
                        format!("set app.{field} in {FILE} or `{field}:` in pubspec.yaml")
                    })?);
                }
            }
        }
        Ok(proj)
    }

    /// Display name (resolved by [`Project::load`]).
    pub fn name(&self) -> &str {
        self.app.name.as_deref().unwrap_or_default()
    }

    /// Version (resolved by [`Project::load`]).
    pub fn version(&self) -> &str {
        self.app.version.as_deref().unwrap_or_default()
    }

    pub fn version_code(&self) -> Result<u64> {
        if let Some(vc) = self.app.version_code {
            return Ok(vc);
        }
        let v = semver::Version::parse(self.version())
            .with_context(|| format!("app.version {:?} is not semver", self.version()))?;
        if v.minor > 99 || v.patch > 99 {
            bail!("cannot derive version_code from {v} (minor/patch > 99); set app.version_code");
        }
        Ok(v.major * 10_000 + v.minor * 100 + v.patch)
    }

    /// Build the manifest (without `files`) for an engine hash.
    pub fn manifest(&self, engine_hash: String) -> Result<Manifest> {
        let mut compat = Compat::new(engine_hash, self.compat.arch);
        compat.machines = self.compat.machines.clone();
        compat.graphics = self.compat.graphics.clone();
        compat.base_image_min = self.compat.base_image_min.clone();
        let mut runtime = Runtime::new(self.runtime.embedder);
        runtime.args = self.runtime.args.clone();
        runtime.env = self.runtime.env.clone();
        let mut m = Manifest::new(
            self.app.id.clone(),
            self.name(),
            self.version(),
            self.version_code()?,
            compat,
        );
        m.min_version_code = self.app.min_version_code;
        m.build_mode = self.app.build_mode;
        m.runtime = runtime;
        m.health = self.health.unwrap_or_default();
        m.permissions = self.app.permissions.clone();
        Ok(m)
    }
}

/// A top-level scalar from pubspec.yaml (`key: value`, optionally quoted, with an optional
/// trailing comment). pubspec.yaml is simple enough that a full YAML parser is not needed.
fn pubspec_field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.strip_prefix(':')?;
        let value = rest.split(" #").next().unwrap_or_default().trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        (!value.is_empty()).then(|| value.to_owned())
    })
}

pub const TEMPLATE: &str = r#"# homing project file. See https://github.com/trevortomlin/homing-cli
[app]
id = "com.example.myapp"
name = "My App"             # default: name from pubspec.yaml
version = "1.0.0"           # default: version from pubspec.yaml
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

#[cfg(test)]
mod tests {
    use super::pubspec_field;

    #[test]
    fn reads_pubspec_fields() {
        let text = "# comment\nname: hello_app\ndescription: x\nversion: 1.2.3+45 # build\n\nenvironment:\n  sdk: ^3.8.0\n";
        assert_eq!(pubspec_field(text, "name").as_deref(), Some("hello_app"));
        assert_eq!(pubspec_field(text, "version").as_deref(), Some("1.2.3+45"));
        assert_eq!(pubspec_field(text, "sdk"), None, "only top-level keys");
        assert_eq!(
            pubspec_field("version: \"2.0.0\"\n", "version").as_deref(),
            Some("2.0.0")
        );
        assert_eq!(
            pubspec_field("version: '2.0.1'\n", "version").as_deref(),
            Some("2.0.1")
        );
        assert_eq!(pubspec_field("version:\n", "version"), None);
        assert_eq!(pubspec_field("versions: 1.0.0\n", "version"), None);
    }
}
