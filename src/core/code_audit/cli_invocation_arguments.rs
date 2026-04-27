//! High-confidence Homeboy shell-out argument-shape drift detection.
//!
//! This intentionally stays structural: it extracts Swift command arrays that
//! look like Homeboy invocations, fills dynamic expressions with placeholder
//! values, and validates the resulting argv with Homeboy's own Clap parser.

use std::path::Path;

use clap::Parser;

use crate::cli_surface::{current_command_surface, Cli};
use crate::engine::codebase_scan::{self, ExtensionFilter, ScanConfig};

use super::conventions::AuditFinding;
use super::findings::{Finding, Severity};

pub(super) fn run(root: &Path) -> Vec<Finding> {
    let config = ScanConfig {
        extensions: ExtensionFilter::Only(vec!["swift".to_string()]),
        ..Default::default()
    };

    let mut findings = Vec::new();
    for path in codebase_scan::walk_files(root, &config) {
        let relative = match path.strip_prefix(root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        findings.extend(analyze_swift_file(&relative, &content));
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.description.cmp(&b.description)));
    findings
}

fn analyze_swift_file(file: &str, content: &str) -> Vec<Finding> {
    let lines: Vec<&str> = content.lines().collect();
    let mut findings = Vec::new();

    for idx in 0..lines.len() {
        if let Some(invocation) = invocation_tokens(&lines, idx) {
            let tokens = invocation.values();
            if let Some(error) = validate_invocation(&tokens) {
                findings.push(finding(
                    file,
                    idx + 1,
                    &display_shape(&tokens),
                    &invocation.source_summary(),
                    &error,
                ));
            }
        }
    }

    findings
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TokenSource {
    value: String,
    line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InvocationTokens {
    tokens: Vec<TokenSource>,
}

impl InvocationTokens {
    fn values(&self) -> Vec<String> {
        self.tokens
            .iter()
            .map(|token| token.value.clone())
            .collect()
    }

    fn source_summary(&self) -> String {
        self.tokens
            .iter()
            .map(|token| format!("{}@{}", token.value, token.line))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn finding(file: &str, line: usize, shape: &str, sources: &str, parser_error: &str) -> Finding {
    Finding {
        convention: "cli_invocation_arguments".to_string(),
        severity: Severity::Warning,
        file: file.to_string(),
        description: format!(
            "Homeboy shell-out uses an argument shape rejected by the current CLI parser at line {}: `{}` (token sources: {})",
            line, shape, sources
        ),
        suggestion: format!(
            "Update this shell-out to match Homeboy's current Clap command surface. Parser error: {}",
            parser_error.lines().next().unwrap_or(parser_error).trim()
        ),
        kind: AuditFinding::StaleCliArgumentShape,
    }
}

fn invocation_tokens(lines: &[&str], start: usize) -> Option<InvocationTokens> {
    let line = lines.get(start)?;
    if !looks_like_invocation_array(line) {
        return None;
    }

    let variable = invocation_variable_name(line);
    let mut tokens = token_sources_from_line(line, start + 1)?;
    let has_homeboy_binary = strip_homeboy_binary(&mut tokens);
    if !has_homeboy_binary && !has_homeboy_wrapper_provenance(lines, start) {
        return None;
    }

    let values = tokens
        .iter()
        .map(|token| token.value.clone())
        .collect::<Vec<_>>();
    if values.is_empty() || !is_homeboy_command_candidate(&values) {
        return None;
    }

    let mut brace_depth = lines
        .iter()
        .take(start + 1)
        .map(|line| brace_delta(line))
        .sum::<isize>();
    let end = (start + 25).min(lines.len().saturating_sub(1));
    for (idx, next) in lines.iter().enumerate().take(end + 1).skip(start + 1) {
        let previous_depth = brace_depth;
        brace_depth += brace_delta(next);

        if previous_depth <= 0 {
            break;
        }

        if !looks_like_argument_append(next, variable.as_deref()) {
            continue;
        }
        if let Some(extra) = token_sources_from_line(next, idx + 1) {
            tokens.extend(extra);
        }
    }

    Some(InvocationTokens { tokens })
}

fn strip_homeboy_binary(tokens: &mut Vec<TokenSource>) -> bool {
    let Some(first) = tokens.first() else {
        return false;
    };

    if is_homeboy_binary_token(&first.value) {
        tokens.remove(0);
        return true;
    }

    false
}

fn is_homeboy_binary_token(token: &str) -> bool {
    token
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name == "homeboy")
}

fn has_homeboy_wrapper_provenance(lines: &[&str], start: usize) -> bool {
    let Some(line) = lines.get(start) else {
        return false;
    };

    if calls_homeboy_wrapper(line) {
        return true;
    }

    if !line.contains("var args") && !line.contains("let args") {
        return false;
    }

    let end = (start + 25).min(lines.len().saturating_sub(1));
    start < end
        && lines[start + 1..=end]
            .iter()
            .any(|next| calls_homeboy_wrapper(next) && next.contains("args"))
}

fn calls_homeboy_wrapper(line: &str) -> bool {
    line.contains("executeCommand(")
        || line.contains("executeWithStdin(")
        || line.contains("cli.execute(")
}

fn validate_invocation(tokens: &[String]) -> Option<String> {
    if should_skip_for_stale_command_detector(tokens) {
        return None;
    }

    let argv = std::iter::once("homeboy".to_string())
        .chain(tokens.iter().cloned())
        .collect::<Vec<_>>();

    Cli::try_parse_from(argv)
        .err()
        .map(|error| error.to_string())
}

fn should_skip_for_stale_command_detector(tokens: &[String]) -> bool {
    let Some(command) = tokens.first() else {
        return true;
    };

    let surface = current_command_surface();
    if !surface.contains_path(&[command.as_str()]) {
        return true;
    }

    if let Some(second) = tokens.get(1) {
        if !second.starts_with('-') && !surface.contains_path(&[command.as_str(), second.as_str()])
        {
            return true;
        }
    }

    false
}

fn is_homeboy_command_candidate(tokens: &[String]) -> bool {
    let Some(command) = tokens.first() else {
        return false;
    };
    current_command_surface().contains_path(&[command.as_str()])
}

fn looks_like_invocation_array(line: &str) -> bool {
    line.contains("args") || line.contains("execute") || line.contains("arguments")
}

fn looks_like_argument_append(line: &str, variable: Option<&str>) -> bool {
    let Some(variable) = variable else {
        return false;
    };

    let trimmed = line.trim_start();
    trimmed.contains(&format!("{variable} +="))
        || trimmed.contains(&format!("{variable}.append(contentsOf:"))
}

fn swift_string_array_items(line: &str) -> Option<Vec<String>> {
    let start = line.find('[')?;
    let end = line[start..].find(']')? + start;
    let inner = &line[start + 1..end];
    let mut items = Vec::new();

    for raw in split_swift_array_items(inner) {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }

        if let Some(stripped) = item.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            items.push(stripped.to_string());
        } else {
            items.push("value".to_string());
        }
    }

    Some(items)
}

fn token_sources_from_line(line: &str, line_number: usize) -> Option<Vec<TokenSource>> {
    swift_string_array_items(line).map(|items| {
        items
            .into_iter()
            .map(|value| TokenSource {
                value,
                line: line_number,
            })
            .collect()
    })
}

fn invocation_variable_name(line: &str) -> Option<String> {
    let before_array = line.split('[').next()?;
    let before_equals = before_array.rsplit('=').nth(1)?;
    before_equals
        .split_whitespace()
        .last()
        .map(|name| name.trim().trim_start_matches("var ").trim().to_string())
        .filter(|name| !name.is_empty())
}

fn brace_delta(line: &str) -> isize {
    let mut delta = 0;
    let mut in_string = false;
    let mut escaped = false;

    for ch in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }

    delta
}

fn split_swift_array_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in inner.chars() {
        if in_string {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                current.push(ch);
            }
            ',' => {
                items.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }

    items
}

fn display_shape(tokens: &[String]) -> String {
    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn detects_known_stale_desktop_argument_shapes() {
        let source = r#"
func fleetCreate(id: String, projectIds: [String]) {
    var args = ["fleet", "create", id]
    for pid in projectIds {
        args += ["--project", pid]
    }
    try await cli.executeCommand(args)
}

func componentCreate(name: String, localPath: String, remotePath: String) {
    var args = ["component", "create", name, localPath, remotePath]
    try await cli.executeCommand(args)
}
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert_eq!(findings.len(), 2);
        assert!(findings
            .iter()
            .all(|f| f.kind == AuditFinding::StaleCliArgumentShape));
        assert!(findings
            .iter()
            .any(|f| f.description.contains("component create")));
        assert!(findings
            .iter()
            .any(|f| f.description.contains("fleet create")));
    }

    #[test]
    fn ignores_current_shapes_and_unrelated_arrays() {
        let source = r#"
func currentFleetCreate(id: String) {
    let args = ["fleet", "create", id, "--projects", "site-a,site-b"]
}

func currentComponentCreate(localPath: String, remotePath: String) {
    let args = ["component", "create", "--local-path", localPath, "--remote-path", remotePath]
}

let unrelated = ["component", "list", "value"]
let another = ["fleet", "add", "prod", "--project", "site-a"]
let fixtureOnly = ["component", "create", "name", "local", "remote"]
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert!(findings.is_empty());
    }

    #[test]
    fn detects_inline_fleet_create_project_flag() {
        let source = r#"
func staleInline(id: String) {
    try await cli.executeCommand(["fleet", "create", id, "--project", "site-a"])
}
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert_eq!(findings.len(), 1);
        assert!(findings[0].suggestion.contains("unexpected argument"));
    }

    #[test]
    fn validates_direct_homeboy_binary_argv() {
        let source = r#"
func directHomeboy(component: String) {
    try await process.execute(["homeboy", "fleet", "create", component, "--project", "site-a"])
}
"#;

        let findings = analyze_swift_file("Process.swift", source);

        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("fleet create"));
        assert!(findings[0].suggestion.contains("unexpected argument"));
    }

    #[test]
    fn swift_wrapper_single_optional_path_does_not_synthesize_repeated_path() {
        let source = r#"
func benchList(componentID: String, path: String?) async throws {
    var args = ["bench", "list", componentID]
    if let path {
        args += ["--path", path]
    }
    try await cli.executeCommand(args)
}

func versionShow(componentID: String, path: String?) async throws {
    var args = ["version", "show", componentID]
    if let path {
        args += ["--path", path]
    }
    try await cli.executeCommand(args)
}
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn swift_arguments_from_unrelated_functions_do_not_leak() {
        let source = r#"
func status() async throws {
    let args = ["status", "--full"]
    try await cli.executeCommand(args)
}

func versionShow(componentID: String, path: String?) async throws {
    var args = ["version", "show", componentID]
    if let path {
        args += ["--path", path]
    }
    try await cli.executeCommand(args)
}
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn current_status_full_shape_does_not_pick_up_nonexistent_path() {
        let source = r#"
func workspaceStatus() async throws {
    let args = ["status", "--full"]
    try await cli.executeCommand(args)
}

func componentVersion(componentID: String, path: String?) async throws {
    var arguments = ["version", "show", componentID]
    if let path {
        arguments.append(contentsOf: ["--path", path])
    }
    try await cli.executeCommand(arguments)
}
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn stale_argument_shape_reports_token_source_lines() {
        let source = r#"
func fleetCreate(id: String, projectID: String) async throws {
    var args = ["fleet", "create", id]
    args += ["--project", projectID]
    try await cli.executeCommand(args)
}
"#;

        let findings = analyze_swift_file("HomeboyCLI.swift", source);

        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("token sources:"));
        assert!(findings[0].description.contains("fleet@3"));
        assert!(findings[0].description.contains("--project@4"));
    }

    #[test]
    fn ignores_external_command_arrays_that_overlap_homeboy_commands() {
        let source = r#"
func remoteUrl(path: String) async throws {
    try await process.execute(["git", "-C", path, "remote", "get-url", "origin"])
}
"#;

        let findings = analyze_swift_file("GitOperationsViewModel.swift", source);

        assert!(findings.is_empty());
    }

    #[test]
    fn ignores_ambiguous_homeboy_shaped_arrays_without_provenance() {
        let source = r#"
func buildArgs(id: String) -> [String] {
    let args = ["fleet", "create", id, "--project", "site-a"]
    return args
}
"#;

        let findings = analyze_swift_file("ArgumentFixtures.swift", source);

        assert!(findings.is_empty());
    }

    #[test]
    fn audit_path_reports_swift_invocations_without_fingerprinting_extension() {
        let root = temp_dir("homeboy-cli-arg-shape");
        fs::create_dir_all(root.join("Homeboy/Core/CLI")).unwrap();
        fs::write(
            root.join("Homeboy/Core/CLI/HomeboyCLI.swift"),
            r#"
func componentCreate(name: String, localPath: String, remotePath: String) {
    var args = ["component", "create", name, localPath, remotePath]
    try await cli.executeCommand(args)
}
"#,
        )
        .unwrap();

        let result = crate::code_audit::audit_path(root.to_str().unwrap()).unwrap();

        fs::remove_dir_all(&root).unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.summary.outliers_found, 1);
        assert_eq!(result.findings[0].kind, AuditFinding::StaleCliArgumentShape);
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }
}
