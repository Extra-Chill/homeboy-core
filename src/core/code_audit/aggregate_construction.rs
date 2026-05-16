//! Direct aggregate construction detector.
//!
//! Finds repeated direct construction of aggregate values when the codebase also
//! exposes a canonical construction seam for the same type. The detector stays
//! project-agnostic by inferring seams from generic builder/factory/newtype
//! naming and by reporting only repeated direct literals.

use std::collections::{BTreeMap, BTreeSet};

use super::conventions::{AuditFinding, Language};
use super::findings::{Finding, Severity};
use super::fingerprint::FileFingerprint;

const MIN_OCCURRENCES: usize = 3;
const MIN_FILES: usize = 2;
const MIN_FIELDS: usize = 2;

pub(super) fn run(fingerprints: &[&FileFingerprint]) -> Vec<Finding> {
    detect_direct_aggregate_construction(fingerprints)
}

#[derive(Debug, Clone)]
struct AggregateLiteral {
    type_name: String,
    fields: BTreeSet<String>,
    file: String,
}

#[derive(Debug, Clone)]
struct ConstructionSeam {
    type_name: String,
    method: String,
}

fn detect_direct_aggregate_construction(fingerprints: &[&FileFingerprint]) -> Vec<Finding> {
    let mut seams_by_type: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut literals_by_type: BTreeMap<String, Vec<AggregateLiteral>> = BTreeMap::new();

    for fp in fingerprints {
        if fp.language != Language::Rust || super::walker::is_test_path(&fp.relative_path) {
            continue;
        }

        for seam in extract_construction_seams(&fp.content) {
            seams_by_type
                .entry(seam.type_name)
                .or_default()
                .insert(seam.method);
        }

        for mut literal in extract_aggregate_literals(&fp.content) {
            literal.file = fp.relative_path.clone();
            literals_by_type
                .entry(literal.type_name.clone())
                .or_default()
                .push(literal);
        }
    }

    let mut findings = Vec::new();

    for (type_name, literals) in literals_by_type {
        let Some(seams) = seams_by_type.get(&type_name) else {
            continue;
        };

        let file_count = literals
            .iter()
            .map(|literal| literal.file.as_str())
            .collect::<BTreeSet<_>>()
            .len();

        if literals.len() < MIN_OCCURRENCES || file_count < MIN_FILES {
            continue;
        }

        let mut field_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut file_counts: BTreeMap<String, usize> = BTreeMap::new();
        for literal in &literals {
            *file_counts.entry(literal.file.clone()).or_insert(0) += 1;
            for field in &literal.fields {
                *field_counts.entry(field.clone()).or_insert(0) += 1;
            }
        }

        let mut top_files = file_counts.into_iter().collect::<Vec<_>>();
        top_files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        top_files.truncate(3);

        let shared_fields = field_counts
            .iter()
            .filter_map(|(field, count)| {
                if *count >= MIN_OCCURRENCES {
                    Some(field.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let seam_display = seams
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let top_files_display = top_files
            .iter()
            .map(|(file, count)| format!("{} ({})", file, count))
            .collect::<Vec<_>>()
            .join(", ");
        let anchor = top_files
            .first()
            .map(|(file, _)| file.clone())
            .unwrap_or_else(|| "<unknown>".to_string());

        findings.push(Finding {
            convention: "direct_aggregate_construction".to_string(),
            severity: Severity::Warning,
            file: anchor,
            description: format!(
                "Direct aggregate construction: `{}` is built inline {} time(s) across {} file(s) despite canonical construction seam(s) [{}]. Repeated fields: [{}]. Top files: {}.",
                type_name,
                literals.len(),
                file_count,
                seam_display,
                shared_fields,
                top_files_display
            ),
            suggestion: format!(
                "Route repeated `{}` construction through the existing builder/factory/helper seam instead of repeating struct literals.",
                type_name
            ),
            kind: AuditFinding::DirectAggregateConstruction,
        });
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.description.cmp(&b.description)));
    findings
}

fn extract_construction_seams(content: &str) -> Vec<ConstructionSeam> {
    let mut seams = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;

    while let Some(impl_pos) = find_word(bytes, i, "impl") {
        let Some((type_name, body_start, body_end)) = parse_impl_block(bytes, impl_pos) else {
            i = impl_pos + 4;
            continue;
        };

        for method in extract_fn_names(&content[body_start..body_end]) {
            if is_canonical_constructor_name(&method, &type_name) {
                seams.push(ConstructionSeam {
                    type_name: type_name.clone(),
                    method,
                });
            }
        }

        i = body_end;
    }

    seams
}

fn extract_aggregate_literals(content: &str) -> Vec<AggregateLiteral> {
    let bytes = content.as_bytes();
    let mut literals = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some(next) = skip_string_or_comment(bytes, i) {
            i = next;
            continue;
        }

        if !is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;
        while i < bytes.len() && is_ident_byte(bytes[i]) {
            i += 1;
        }
        let type_name = &content[start..i];

        if !looks_like_type_name(type_name)
            || is_definition_keyword_before(bytes, start)
            || previous_non_ws(bytes, start) == Some(b'>')
        {
            continue;
        }

        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'{' {
            continue;
        }

        let Some((fields, end)) = parse_struct_literal_fields(bytes, j + 1) else {
            continue;
        };
        if fields.len() >= MIN_FIELDS {
            literals.push(AggregateLiteral {
                type_name: type_name.to_string(),
                fields,
                file: String::new(),
            });
        }
        i = end;
    }

    literals
}

fn parse_impl_block(bytes: &[u8], impl_pos: usize) -> Option<(String, usize, usize)> {
    let mut i = impl_pos + 4;
    i = skip_ascii_whitespace(bytes, i);

    if starts_with_word(bytes, i, "<") {
        i = skip_angle_group(bytes, i)?;
        i = skip_ascii_whitespace(bytes, i);
    }

    if starts_with_word(bytes, i, "dyn") {
        return None;
    }

    let type_start = i;
    while i < bytes.len() && is_ident_byte(bytes[i]) {
        i += 1;
    }
    if type_start == i {
        return None;
    }
    let type_name = std::str::from_utf8(&bytes[type_start..i]).ok()?.to_string();
    if !looks_like_type_name(&type_name) {
        return None;
    }

    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let body_start = i + 1;
    let body_end = find_matching_brace(bytes, i)?;
    Some((type_name, body_start, body_end))
}

fn extract_fn_names(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut names = Vec::new();
    let mut i = 0;
    while let Some(fn_pos) = find_word(bytes, i, "fn") {
        let mut name_start = fn_pos + 2;
        name_start = skip_ascii_whitespace(bytes, name_start);
        let mut name_end = name_start;
        while name_end < bytes.len() && is_ident_byte(bytes[name_end]) {
            name_end += 1;
        }
        if name_end > name_start {
            names.push(content[name_start..name_end].to_string());
        }
        i = name_end.max(fn_pos + 2);
    }
    names
}

fn is_canonical_constructor_name(method: &str, type_name: &str) -> bool {
    if matches!(method, "builder" | "new" | "default") {
        return true;
    }
    if method.starts_with("from_") || method.starts_with("for_") || method.starts_with("with_") {
        return true;
    }
    let snake_type = to_snake_case(type_name);
    method == format!("build_{}", snake_type) || method == format!("create_{}", snake_type)
}

fn parse_struct_literal_fields(bytes: &[u8], mut i: usize) -> Option<(BTreeSet<String>, usize)> {
    let mut fields = BTreeSet::new();
    let mut depth = 0usize;

    while i < bytes.len() {
        if let Some(next) = skip_string_or_comment(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'{' | b'(' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' if depth == 0 => return Some((fields, i + 1)),
            b'}' | b')' | b']' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'.' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'.' => {
                i += 2;
            }
            b if depth == 0 && is_ident_start(b) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
                let end = i;
                let mut j = i;
                j = skip_ascii_whitespace(bytes, j);
                if j < bytes.len() && bytes[j] == b':' {
                    fields.insert(std::str::from_utf8(&bytes[start..end]).ok()?.to_string());
                }
            }
            _ => i += 1,
        }
    }

    None
}

fn skip_string_or_comment(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes.get(i).copied()? {
        b'"' => Some(skip_quoted(bytes, i, b'"')),
        b'\'' => Some(skip_quoted(bytes, i, b'\'')),
        b'/' if bytes.get(i + 1) == Some(&b'/') => bytes[i..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|offset| i + offset + 1)
            .or(Some(bytes.len())),
        b'/' if bytes.get(i + 1) == Some(&b'*') => bytes[i + 2..]
            .windows(2)
            .position(|w| w == b"*/")
            .map(|offset| i + 2 + offset + 2)
            .or(Some(bytes.len())),
        _ => None,
    }
}

fn skip_quoted(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn find_word(bytes: &[u8], start: usize, word: &str) -> Option<usize> {
    let needle = word.as_bytes();
    bytes[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .and_then(|offset| {
            let pos = start + offset;
            let before_ok = pos == 0 || !is_ident_byte(bytes[pos - 1]);
            let after = pos + needle.len();
            let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
            if before_ok && after_ok {
                Some(pos)
            } else {
                None
            }
        })
}

fn starts_with_word(bytes: &[u8], start: usize, word: &str) -> bool {
    bytes[start..].starts_with(word.as_bytes())
}

fn skip_angle_group(bytes: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        if let Some(next) = skip_string_or_comment(bytes, i) {
            i = next;
            continue;
        }
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_ascii_whitespace(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(bytes.len(), |offset| start + offset)
}

fn previous_non_ws(bytes: &[u8], start: usize) -> Option<u8> {
    bytes[..start]
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn is_definition_keyword_before(bytes: &[u8], start: usize) -> bool {
    let prefix = std::str::from_utf8(&bytes[..start]).unwrap_or_default();
    let token = prefix
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .rev()
        .find(|part| !part.is_empty());
    matches!(
        token,
        Some("struct" | "enum" | "impl" | "trait" | "type" | "use")
    )
}

fn looks_like_type_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn to_snake_case(value: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_file(path: &str, content: &str) -> FileFingerprint {
        FileFingerprint {
            relative_path: path.to_string(),
            language: Language::Rust,
            content: content.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_run() {
        let files = [
            rust_file(
                "src/report.rs",
                r#"
                impl SummaryReport {
                    pub fn new(status: Status, count: usize) -> Self {
                        Self { status, count }
                    }
                }
                fn local() -> SummaryReport {
                    SummaryReport { status: Status::Ready, count: 1 }
                }
                "#,
            ),
            rust_file(
                "src/a.rs",
                "fn a() -> SummaryReport { SummaryReport { status: Status::Ready, count: 2 } }",
            ),
            rust_file(
                "src/b.rs",
                "fn b() -> SummaryReport { SummaryReport { status: Status::Skipped, count: 3 } }",
            ),
        ];
        let refs = files.iter().collect::<Vec<_>>();

        assert_eq!(run(&refs).len(), 1);
    }

    #[test]
    fn flags_repeated_direct_aggregate_when_canonical_seam_exists() {
        let files = [
            rust_file(
                "src/order.rs",
                r#"
                pub struct DispatchPlan { status: Status, steps: Vec<Step>, dry_run: bool }
                impl DispatchPlan {
                    pub fn builder() -> DispatchPlanBuilder { DispatchPlanBuilder::default() }
                }
                pub fn a() -> DispatchPlan {
                    DispatchPlan { status: Status::Ready, steps: vec![], dry_run: false }
                }
                "#,
            ),
            rust_file(
                "src/a.rs",
                r#"
                pub fn b() -> DispatchPlan {
                    DispatchPlan { status: Status::Ready, steps: vec![], dry_run: false }
                }
                "#,
            ),
            rust_file(
                "src/b.rs",
                r#"
                pub fn c() -> DispatchPlan {
                    DispatchPlan { status: Status::Skipped, steps: vec![step()], dry_run: true }
                }
                "#,
            ),
        ];
        let refs = files.iter().collect::<Vec<_>>();

        let findings = run(&refs);

        assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
        assert_eq!(findings[0].kind, AuditFinding::DirectAggregateConstruction);
        assert!(findings[0].description.contains("DispatchPlan"));
        assert!(findings[0].description.contains("builder"));
    }

    #[test]
    fn extracts_rust_construction_seams_and_literals() {
        let content = r#"
        impl DispatchPlan {
            pub fn builder() -> DispatchPlanBuilder { DispatchPlanBuilder::default() }
        }
        pub fn a() -> DispatchPlan {
            DispatchPlan { status: Status::Ready, steps: vec![], dry_run: false }
        }
        "#;

        assert_eq!(extract_construction_seams(content).len(), 1);
        assert_eq!(extract_aggregate_literals(content).len(), 1);
    }

    #[test]
    fn does_not_flag_repeated_aggregate_without_construction_seam() {
        let files = [
            rust_file("src/a.rs", "pub fn a() -> Point { Point { x: 1, y: 2 } }"),
            rust_file("src/b.rs", "pub fn b() -> Point { Point { x: 3, y: 4 } }"),
            rust_file("src/c.rs", "pub fn c() -> Point { Point { x: 5, y: 6 } }"),
        ];
        let refs = files.iter().collect::<Vec<_>>();

        assert!(run(&refs).is_empty());
    }

    #[test]
    fn ignores_test_paths() {
        let files = [
            rust_file(
                "src/plan.rs",
                "impl BuildReport { pub fn new() -> Self { Self { status: Status::Ready, count: 0 } } }",
            ),
            rust_file(
                "tests/a.rs",
                "fn a() -> BuildReport { BuildReport { status: Status::Ready, count: 1 } }",
            ),
            rust_file(
                "tests/b.rs",
                "fn b() -> BuildReport { BuildReport { status: Status::Ready, count: 2 } }",
            ),
            rust_file(
                "tests/c.rs",
                "fn c() -> BuildReport { BuildReport { status: Status::Ready, count: 3 } }",
            ),
        ];
        let refs = files.iter().collect::<Vec<_>>();

        assert!(run(&refs).is_empty());
    }
}
