use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuditConfig {
    /// Class/base names whose public methods are invoked by a runtime dispatcher.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_entrypoint_extends: Vec<String>,
    /// Source markers that indicate public methods are runtime-dispatched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_entrypoint_markers: Vec<String>,
    /// Paths whose guards run outside normal production runtime assumptions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifecycle_path_globs: Vec<String>,
    /// Type suffixes that mark convention outliers as intentional utilities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub utility_suffixes: Vec<String>,
    /// Files exempt from convention outlier checks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub convention_exception_globs: Vec<String>,
    /// Symbols that are known to exist when component metadata proves a runtime
    /// floor, package, or bootstrap file is present.
    #[serde(default, skip_serializing_if = "KnownSymbolsConfig::is_empty")]
    pub known_symbols: KnownSymbolsConfig,
    /// Extension-owned text detector rules that emit audit findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_detectors: Vec<RequestedDetectorRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct KnownSymbolsConfig {
    /// Header-version providers keyed by an extension-owned marker and header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_versions: Vec<KnownSymbolHeaderVersionProvider>,
    /// Composer package providers keyed by package name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composer_packages: Vec<KnownSymbolPackageProvider>,
    /// Bootstrap path providers keyed by a normalized path substring or suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bootstrap_paths: Vec<KnownSymbolBootstrapPathProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownSymbolHeaderVersionProvider {
    /// Marker used to locate the component entry file.
    pub file_marker: String,
    /// Header key whose value contains the runtime version floor.
    pub version_header: String,
    /// Symbols introduced by runtime version.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<KnownSymbolVersionedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownSymbolPackageProvider {
    pub package: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<KnownSymbolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownSymbolBootstrapPathProvider {
    pub path_contains: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<KnownSymbolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownSymbolVersionedEntry {
    pub name: String,
    pub kind: KnownSymbolKind,
    pub introduced: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownSymbolEntry {
    pub name: String,
    pub kind: KnownSymbolKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KnownSymbolKind {
    Function,
    Class,
    Constant,
}

impl KnownSymbolsConfig {
    pub fn is_empty(&self) -> bool {
        self.header_versions.is_empty()
            && self.composer_packages.is_empty()
            && self.bootstrap_paths.is_empty()
    }

    fn merge(&mut self, other: &KnownSymbolsConfig) {
        extend_unique(&mut self.header_versions, &other.header_versions);
        extend_unique(&mut self.composer_packages, &other.composer_packages);
        extend_unique(&mut self.bootstrap_paths, &other.bootstrap_paths);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestedDetectorRule {
    /// Human-readable detector label used for logging/debugging.
    pub id: String,
    /// Audit finding kind in snake_case, e.g. `json_like_exact_match`.
    pub kind: String,
    /// `warning` or `info`. Defaults to `warning`.
    #[serde(default = "default_requested_detector_severity")]
    pub severity: String,
    /// Report convention label. Defaults to `requested_detectors`.
    #[serde(default = "default_requested_detector_convention")]
    pub convention: String,
    /// Optional language filter using Homeboy's lowercase language names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Optional path-extension filter, without leading dots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_extensions: Vec<String>,
    /// Path substrings that opt files out of this detector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_path_contains: Vec<String>,
    /// Detector body. Core owns the execution primitives; extensions own the rules.
    #[serde(flatten)]
    pub rule: RequestedDetectorRuleBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestedDetectorRuleBody {
    /// Emit one finding for each regex match in a file.
    Regex {
        pattern: String,
        description: String,
        suggestion: String,
    },
    /// Emit regex findings only when extracted comments match a trigger pattern.
    CommentRegex {
        comment_pattern: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment_exclude_pattern: Option<String>,
        pattern: String,
        description: String,
        suggestion: String,
    },
    /// Collect values with one regex, then flag matching literals in other files.
    DerivedLiteral {
        source_pattern: String,
        value_capture: String,
        label: String,
        literal_pattern: String,
        description: String,
        suggestion: String,
    },
}

fn default_requested_detector_severity() -> String {
    "warning".to_string()
}

fn default_requested_detector_convention() -> String {
    "requested_detectors".to_string()
}

impl AuditConfig {
    pub fn is_empty(&self) -> bool {
        self.runtime_entrypoint_extends.is_empty()
            && self.runtime_entrypoint_markers.is_empty()
            && self.lifecycle_path_globs.is_empty()
            && self.utility_suffixes.is_empty()
            && self.convention_exception_globs.is_empty()
            && self.known_symbols.is_empty()
            && self.requested_detectors.is_empty()
    }

    pub fn merge(&mut self, other: &AuditConfig) {
        extend_unique(
            &mut self.runtime_entrypoint_extends,
            &other.runtime_entrypoint_extends,
        );
        extend_unique(
            &mut self.runtime_entrypoint_markers,
            &other.runtime_entrypoint_markers,
        );
        extend_unique(&mut self.lifecycle_path_globs, &other.lifecycle_path_globs);
        extend_unique(&mut self.utility_suffixes, &other.utility_suffixes);
        extend_unique(
            &mut self.convention_exception_globs,
            &other.convention_exception_globs,
        );
        self.known_symbols.merge(&other.known_symbols);
        for rule in &other.requested_detectors {
            if !self
                .requested_detectors
                .iter()
                .any(|existing| existing.id == rule.id)
            {
                self.requested_detectors.push(rule.clone());
            }
        }
    }
}

fn extend_unique<T: Clone + PartialEq>(target: &mut Vec<T>, source: &[T]) {
    for value in source {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}
