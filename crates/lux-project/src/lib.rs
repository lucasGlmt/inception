//! Filesystem-facing Lux project model and build pipeline.
//!
//! The compiler remains content-in/content-out. This crate owns project
//! discovery, typed configuration, path resolution, and loading before it
//! invokes the shared compiler and linker APIs.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use inception_core::UniverseId;
use inception_linker::{
    Capability, CapabilitySet, FixtureDefinition, FixtureDefinitionError, FixtureLibrary,
    FixtureMappings, LinkError, Patch, RgbOffsets, RigBinding, RoleBinding, RuntimeImage, link,
};
use lux_compiler::Diagnostic;
use serde::Deserialize;

pub const MANIFEST_FILE: &str = "lux.toml";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub project: ProjectSection,
    pub source: SourceSection,
    pub rig: RigSection,
    #[serde(default)]
    pub fixtures: FixturesSection,
    #[serde(default)]
    pub runtime: RuntimeSection,
    #[serde(default)]
    pub output: OutputSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSection {
    pub entry: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigSection {
    pub patch: PathBuf,
    pub bindings: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixturesSection {
    #[serde(default = "default_fixtures_directory")]
    pub directory: PathBuf,
}

impl Default for FixturesSection {
    fn default() -> Self {
        Self {
            directory: default_fixtures_directory(),
        }
    }
}

fn default_fixtures_directory() -> PathBuf {
    PathBuf::from("fixtures")
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSection {
    #[serde(default = "default_frequency")]
    pub frequency: u32,
}

impl Default for RuntimeSection {
    fn default() -> Self {
        Self {
            frequency: default_frequency(),
        }
    }
}

const fn default_frequency() -> u32 {
    40
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSection {
    #[serde(default = "default_output_driver")]
    pub driver: String,
    pub device: Option<PathBuf>,
    #[serde(default = "default_universe")]
    pub universe: u16,
}

impl Default for OutputSection {
    fn default() -> Self {
        Self {
            driver: default_output_driver(),
            device: None,
            universe: default_universe(),
        }
    }
}

fn default_output_driver() -> String {
    "null".into()
}

const fn default_universe() -> u16 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub entry: PathBuf,
    pub patch: PathBuf,
    pub bindings: PathBuf,
    pub fixtures: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuxProject {
    pub manifest: ProjectManifest,
    pub paths: ProjectPaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildTimings {
    pub compile: Duration,
    pub link: Duration,
    pub total: Duration,
}

#[derive(Debug)]
pub struct BuiltProject {
    pub project: LuxProject,
    pub image: RuntimeImage,
    pub timings: BuildTimings,
}

#[derive(Debug)]
pub enum ProjectError {
    NotFound(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Manifest {
        path: PathBuf,
        source: toml::de::Error,
    },
    Config {
        path: PathBuf,
        source: toml::de::Error,
    },
    InvalidManifest(Vec<String>),
    Compile(Vec<Diagnostic>),
    FixtureDefinition {
        path: PathBuf,
        errors: Vec<FixtureDefinitionError>,
    },
    Link(Vec<LinkError>),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(start) => write!(
                f,
                "no {MANIFEST_FILE} found in {} or its parents",
                start.display()
            ),
            Self::Io { path, source } => write!(f, "failed to read {}: {source}", path.display()),
            Self::Manifest { path, source } => {
                write!(f, "invalid project manifest {}: {source}", path.display())
            }
            Self::Config { path, source } => {
                write!(
                    f,
                    "invalid project configuration {}: {source}",
                    path.display()
                )
            }
            Self::InvalidManifest(errors) => {
                write!(f, "invalid project manifest: {}", errors.join("; "))
            }
            Self::Compile(diagnostics) => write!(
                f,
                "compilation failed with {} diagnostic(s)",
                diagnostics.len()
            ),
            Self::FixtureDefinition { path, errors } => write!(
                f,
                "invalid fixture definition {}: {errors:?}",
                path.display()
            ),
            Self::Link(errors) => write!(f, "link failed: {errors:?}"),
        }
    }
}

impl std::error::Error for ProjectError {}

pub fn discover_project(start: impl AsRef<Path>) -> Result<PathBuf, ProjectError> {
    let start = start.as_ref();
    let mut directory = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = directory.join(MANIFEST_FILE);
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !directory.pop() {
            return Err(ProjectError::NotFound(start.to_path_buf()));
        }
    }
}

impl LuxProject {
    pub fn load(manifest_path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let manifest_path = manifest_path.as_ref();
        let contents = read_to_string(manifest_path)?;
        let manifest: ProjectManifest =
            toml::from_str(&contents).map_err(|source| ProjectError::Manifest {
                path: manifest_path.to_path_buf(),
                source,
            })?;
        validate_manifest(&manifest)?;
        let root = manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()
            .map_err(|source| ProjectError::Io {
                path: manifest_path.to_path_buf(),
                source,
            })?;
        let resolve = |path: &Path| root.join(path);
        let paths = ProjectPaths {
            root: root.clone(),
            manifest: root.join(MANIFEST_FILE),
            entry: resolve(&manifest.source.entry),
            patch: resolve(&manifest.rig.patch),
            bindings: resolve(&manifest.rig.bindings),
            fixtures: resolve(&manifest.fixtures.directory),
        };
        Ok(Self { manifest, paths })
    }

    pub fn build(&self) -> Result<BuiltProject, ProjectError> {
        let total_started = Instant::now();
        let source = read_to_string(&self.paths.entry)?;
        let compile_started = Instant::now();
        let bytecode = lux_compiler::compile_portable(&source).map_err(ProjectError::Compile)?;
        let compile = compile_started.elapsed();

        let library = load_fixture_library(&self.paths.fixtures)?;
        let patch = load_patch(&self.paths.patch)?;
        let rig = load_rig(&self.paths.bindings)?;
        let link_started = Instant::now();
        let image = link(&bytecode, &library, &patch, &rig).map_err(ProjectError::Link)?;
        let link = link_started.elapsed();
        Ok(BuiltProject {
            project: self.clone(),
            image,
            timings: BuildTimings {
                compile,
                link,
                total: total_started.elapsed(),
            },
        })
    }
}

pub fn load_and_build(manifest_path: impl AsRef<Path>) -> Result<BuiltProject, ProjectError> {
    LuxProject::load(manifest_path)?.build()
}

fn validate_manifest(manifest: &ProjectManifest) -> Result<(), ProjectError> {
    let mut errors = Vec::new();
    if manifest.project.name.trim().is_empty() {
        errors.push("project.name must not be empty".into());
    }
    if !is_project_relative(&manifest.source.entry) {
        errors.push("source.entry must be a non-empty relative path".into());
    }
    for (name, path) in [
        ("rig.patch", &manifest.rig.patch),
        ("rig.bindings", &manifest.rig.bindings),
        ("fixtures.directory", &manifest.fixtures.directory),
    ] {
        if !is_project_relative(path) {
            errors.push(format!("{name} must be a non-empty relative path"));
        }
    }
    if !(1..=1_000).contains(&manifest.runtime.frequency) {
        errors.push("runtime.frequency must be between 1 and 1000 Hz".into());
    }
    if !matches!(
        manifest.output.driver.as_str(),
        "null" | "recording" | "dev" | "dmx" | "enttec"
    ) {
        errors.push(format!(
            "unknown output driver `{}`",
            manifest.output.driver
        ));
    }
    if matches!(manifest.output.driver.as_str(), "dmx" | "enttec")
        && manifest.output.device.is_none()
    {
        errors.push("output.device is required for the real DMX driver".into());
    }
    if manifest
        .output
        .device
        .as_ref()
        .is_some_and(|path| !is_project_relative(path))
    {
        errors.push("output.device must be relative to the project directory".into());
    }
    if manifest.output.universe == 0 {
        errors.push("output.universe must be greater than zero".into());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ProjectError::InvalidManifest(errors))
    }
}

fn is_project_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn read_to_string(path: &Path) -> Result<String, ProjectError> {
    fs::read_to_string(path).map_err(|source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    fixture: FixtureRecord,
    #[serde(default)]
    mapping: MappingRecord,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureRecord {
    name: String,
    footprint: u16,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingRecord {
    intensity: Option<u16>,
    red: Option<u16>,
    green: Option<u16>,
    blue: Option<u16>,
}

fn load_fixture_library(directory: &Path) -> Result<FixtureLibrary, ProjectError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(|source| ProjectError::Io {
        path: directory.to_path_buf(),
        source,
    })? {
        let path = entry
            .map_err(|source| ProjectError::Io {
                path: directory.to_path_buf(),
                source,
            })?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut library = FixtureLibrary::new();
    for path in paths {
        let config: FixtureFile = parse_config(&path)?;
        let mut capabilities = Vec::new();
        for capability in &config.fixture.capabilities {
            capabilities.push(match capability.as_str() {
                "Intensity" | "intensity" => Capability::Intensity,
                "Color" | "color" => Capability::Color,
                other => {
                    return Err(ProjectError::InvalidManifest(vec![format!(
                        "unknown fixture capability `{other}` in {}",
                        path.display()
                    )]));
                }
            });
        }
        let color = match (
            config.mapping.red,
            config.mapping.green,
            config.mapping.blue,
        ) {
            (None, None, None) => None,
            (Some(red), Some(green), Some(blue)) => Some(RgbOffsets { red, green, blue }),
            _ => {
                return Err(ProjectError::InvalidManifest(vec![format!(
                    "color mapping in {} requires red, green, and blue offsets",
                    path.display()
                )]));
            }
        };
        let definition = FixtureDefinition::new(
            config.fixture.name,
            config.fixture.footprint,
            CapabilitySet::from_capabilities(capabilities),
            FixtureMappings {
                intensity: config.mapping.intensity,
                color,
            },
        )
        .map_err(|errors| ProjectError::FixtureDefinition {
            path: path.clone(),
            errors,
        })?;
        library.insert(definition);
    }
    Ok(library)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchFile {
    patch: NamedRecord,
    #[serde(default)]
    fixtures: Vec<PatchFixtureRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedRecord {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchFixtureRecord {
    name: String,
    definition: String,
    universe: u16,
    address: u16,
}

fn load_patch(path: &Path) -> Result<Patch, ProjectError> {
    let config: PatchFile = parse_config(path)?;
    let mut patch = Patch::new(config.patch.name);
    for fixture in config.fixtures {
        patch
            .add_fixture(
                fixture.name,
                fixture.definition,
                UniverseId(fixture.universe),
                fixture.address,
            )
            .map_err(|error| ProjectError::Link(vec![error]))?;
    }
    Ok(patch)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RigFile {
    rig: RigRecord,
    #[serde(default)]
    bindings: Vec<BindingRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RigRecord {
    name: String,
    contract: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingRecord {
    role: String,
    fixtures: Vec<String>,
}

fn load_rig(path: &Path) -> Result<RigBinding, ProjectError> {
    let config: RigFile = parse_config(path)?;
    Ok(RigBinding {
        name: config.rig.name,
        contract: config.rig.contract,
        bindings: config
            .bindings
            .into_iter()
            .map(|binding| RoleBinding {
                role: binding.role,
                fixtures: binding.fixtures,
            })
            .collect(),
    })
}

fn parse_config<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ProjectError> {
    toml::from_str(&read_to_string(path)?).map_err(|source| ProjectError::Config {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_manifest_upward_and_resolves_paths_from_its_directory() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("a/b")).unwrap();
        fs::write(
            temp.path().join(MANIFEST_FILE),
            r#"
[project]
name = "demo"
[source]
entry = "src/main.lux"
[rig]
patch = "rig/patch.lux"
bindings = "rig/rig.lux"
"#,
        )
        .unwrap();
        let found = discover_project(temp.path().join("a/b")).unwrap();
        let project = LuxProject::load(found).unwrap();
        assert_eq!(
            project.paths.entry,
            temp.path().canonicalize().unwrap().join("src/main.lux")
        );
        assert_eq!(project.manifest.runtime.frequency, 40);
        assert_eq!(project.manifest.output.driver, "null");
    }

    #[test]
    fn invalid_frequency_and_driver_are_diagnostics_not_panics() {
        let manifest: ProjectManifest = toml::from_str(
            r#"
[project]
name = "demo"
[source]
entry = "main.lux"
[rig]
patch = "patch.lux"
bindings = "rig.lux"
[runtime]
frequency = 0
[output]
driver = "mystery"
"#,
        )
        .unwrap();
        let ProjectError::InvalidManifest(errors) = validate_manifest(&manifest).unwrap_err()
        else {
            panic!("expected validation error")
        };
        assert_eq!(errors.len(), 2);
    }
}
