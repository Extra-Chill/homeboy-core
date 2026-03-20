//! Test plan generation from function contracts.
//!
//! Takes a `FunctionContract` and produces a `TestPlan` — a structured
//! description of what tests to generate. The plan is language-agnostic.
//!
//! Rendering the plan into actual source code uses templates from grammar.toml
//! `[contract.test_templates]`. Core fills in the variables, the grammar
//! provides the syntax.
//!
//! ## Behavioral inference
//!
//! The test plan generator analyzes branch conditions and return values to
//! produce **behavioral** tests rather than smoke tests. For each branch:
//!
//! 1. **Setup inference** — pattern-matches the condition string against the
//!    parameter types to derive input values that trigger the branch. E.g.,
//!    `"commits.is_empty()"` with a `Vec<CommitInfo>` param → pass `vec![]`.
//!
//! 2. **Assertion inference** — uses the return variant and value description
//!    to derive specific assertions. E.g., `Ok("skipped")` → assert the
//!    unwrapped value matches the expected description, not just `is_ok()`.

use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::contract::*;
use crate::extension::grammar::TypeDefault;

/// A plan for generating tests for a single function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPlan {
    /// The function being tested.
    pub function_name: String,
    /// Source file containing the function.
    pub source_file: String,
    /// Individual test cases to generate.
    pub cases: Vec<TestCase>,
}

/// A single test case to generate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    /// Suggested test function name (e.g., "test_validate_write_skips_when_empty").
    pub test_name: String,
    /// Which branch this test covers.
    pub branch_condition: String,
    /// The expected return variant (ok, err, some, none, true, false, value).
    pub expected_variant: String,
    /// Description of what the expected return value should be.
    pub expected_value: Option<String>,
    /// The template key to use for rendering (e.g., "result_ok", "option_none", "bool_true").
    pub template_key: String,
    /// Template variables for rendering.
    pub variables: HashMap<String, String>,
}

/// Generate a test plan from a function contract.
///
/// Produces one test case per branch with behavioral setup and assertions.
/// The plan is language-agnostic — rendering to source code requires
/// templates from the grammar.
///
/// `type_defaults` from the grammar are used to generate valid input
/// construction for each parameter. Branch conditions and return values
/// are analyzed to derive condition-specific setup overrides and targeted
/// assertions.
pub(crate) fn generate_test_plan(
    contract: &FunctionContract,
    type_defaults: &[TypeDefault],
) -> TestPlan {
    let mut cases = Vec::new();

    if contract.branches.is_empty() {
        // No branches detected — generate a basic "does not panic" test
        cases.push(TestCase {
            test_name: format!("test_{}_does_not_panic", contract.name),
            branch_condition: "default invocation".to_string(),
            expected_variant: "no_panic".to_string(),
            expected_value: None,
            template_key: "no_panic".to_string(),
            variables: build_variables(contract, None, type_defaults),
        });
    } else {
        for (i, branch) in contract.branches.iter().enumerate() {
            let condition_slug = slugify(&branch.condition);
            let test_name = format!(
                "test_{}_{}",
                contract.name,
                if condition_slug.is_empty() {
                    format!("branch_{}", i)
                } else {
                    condition_slug
                }
            );

            let template_key =
                derive_template_key(&contract.signature.return_type, &branch.returns);

            let mut vars = build_variables(contract, Some(branch), type_defaults);
            vars.insert("condition".to_string(), branch.condition.clone());
            vars.insert("condition_slug".to_string(), slugify(&branch.condition));

            // Behavioral inference: derive setup overrides from branch condition
            let setup_override = infer_setup_from_condition(
                &branch.condition,
                &contract.signature.params,
                type_defaults,
            );
            if let Some(ref so) = setup_override {
                vars.insert("param_setup".to_string(), so.setup_lines.clone());
                vars.insert("param_args".to_string(), so.call_args.clone());
                // Merge any additional imports
                if !so.extra_imports.is_empty() {
                    let existing = vars.get("extra_imports").cloned().unwrap_or_default();
                    let merged = merge_imports(&existing, &so.extra_imports);
                    vars.insert("extra_imports".to_string(), merged);
                }
            }

            // Behavioral inference: derive assertion from branch return
            let assertion = infer_assertion(
                &branch.returns,
                &contract.signature.return_type,
                &branch.condition,
            );
            vars.insert("assertion_code".to_string(), assertion);

            cases.push(TestCase {
                test_name,
                branch_condition: branch.condition.clone(),
                expected_variant: branch.returns.variant.clone(),
                expected_value: branch.returns.value.clone(),
                template_key,
                variables: vars,
            });
        }
    }

    // If the function has effects, generate an effect-specific test
    if contract.has_effects() && !contract.effects.is_empty() {
        let effect_names: Vec<&str> = contract
            .effects
            .iter()
            .map(|e| match e {
                Effect::FileRead => "file_read",
                Effect::FileWrite => "file_write",
                Effect::FileDelete => "file_delete",
                Effect::ProcessSpawn { .. } => "process_spawn",
                Effect::Mutation { .. } => "mutation",
                Effect::Panic { .. } => "panic",
                Effect::Network => "network",
                Effect::ResourceAlloc { .. } => "resource_alloc",
                Effect::Logging => "logging",
            })
            .collect();

        let mut vars = build_variables(contract, None, type_defaults);
        vars.insert("effects".to_string(), effect_names.join(", "));

        cases.push(TestCase {
            test_name: format!("test_{}_has_expected_effects", contract.name),
            branch_condition: "effect verification".to_string(),
            expected_variant: "effects".to_string(),
            expected_value: Some(effect_names.join(", ")),
            template_key: "effects".to_string(),
            variables: vars,
        });
    }

    TestPlan {
        function_name: contract.name.clone(),
        source_file: contract.file.clone(),
        cases,
    }
}

/// Render a test plan into source code using templates.
///
/// Templates are key → string pairs where keys match `TestCase.template_key`.
/// Template variables are replaced: `{fn_name}`, `{fn_call}`, `{param_list}`, etc.
pub(crate) fn render_test_plan(plan: &TestPlan, templates: &HashMap<String, String>) -> String {
    let mut output = String::new();

    for case in &plan.cases {
        let template = match templates.get(&case.template_key) {
            Some(t) => t,
            None => {
                // Fall back to a generic template if the specific one doesn't exist
                match templates.get("default") {
                    Some(t) => t,
                    None => continue,
                }
            }
        };

        let mut rendered = template.clone();
        for (key, value) in &case.variables {
            rendered = rendered.replace(&format!("{{{}}}", key), value);
        }
        // Also replace the test name
        rendered = rendered.replace("{test_name}", &case.test_name);

        output.push_str(&rendered);
        output.push('\n');
    }

    output
}

/// Build template variables from a contract and optional branch.
fn build_variables(
    contract: &FunctionContract,
    branch: Option<&Branch>,
    type_defaults: &[TypeDefault],
) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    vars.insert("fn_name".to_string(), contract.name.clone());
    vars.insert("file".to_string(), contract.file.clone());
    vars.insert("line".to_string(), contract.line.to_string());

    // Build param list for function call
    let param_names: Vec<&str> = contract
        .signature
        .params
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    vars.insert("param_names".to_string(), param_names.join(", "));

    // Build typed param declarations
    let param_decls: Vec<String> = contract
        .signature
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.param_type))
        .collect();
    vars.insert("param_decls".to_string(), param_decls.join(", "));

    // Param count
    vars.insert(
        "param_count".to_string(),
        contract.signature.params.len().to_string(),
    );

    // Build param setup lines and call args using type_defaults
    let (setup_lines, call_args, extra_imports) =
        build_param_inputs(&contract.signature.params, type_defaults);
    vars.insert("param_setup".to_string(), setup_lines);
    vars.insert("param_args".to_string(), call_args);
    vars.insert("extra_imports".to_string(), extra_imports);

    // Return type info
    match &contract.signature.return_type {
        ReturnShape::Unit => vars.insert("return_shape".to_string(), "unit".to_string()),
        ReturnShape::Bool => vars.insert("return_shape".to_string(), "bool".to_string()),
        ReturnShape::Value { value_type } => {
            vars.insert("return_shape".to_string(), "value".to_string());
            vars.insert("return_type".to_string(), value_type.clone())
        }
        ReturnShape::OptionType { some_type } => {
            vars.insert("return_shape".to_string(), "option".to_string());
            vars.insert("some_type".to_string(), some_type.clone())
        }
        ReturnShape::ResultType { ok_type, err_type } => {
            vars.insert("return_shape".to_string(), "result".to_string());
            vars.insert("ok_type".to_string(), ok_type.clone());
            vars.insert("err_type".to_string(), err_type.clone())
        }
        ReturnShape::Collection { element_type } => {
            vars.insert("return_shape".to_string(), "collection".to_string());
            vars.insert("element_type".to_string(), element_type.clone())
        }
        ReturnShape::Unknown { raw } => {
            vars.insert("return_shape".to_string(), "unknown".to_string());
            vars.insert("return_type".to_string(), raw.clone())
        }
    };

    // Branch-specific variables
    if let Some(branch) = branch {
        vars.insert("variant".to_string(), branch.returns.variant.clone());
        if let Some(ref val) = branch.returns.value {
            vars.insert("expected_value".to_string(), val.clone());
        }
    }

    // Is it a method (has receiver)?
    vars.insert(
        "is_method".to_string(),
        contract.signature.receiver.is_some().to_string(),
    );
    vars.insert("is_pure".to_string(), contract.is_pure().to_string());
    vars.insert(
        "branch_count".to_string(),
        contract.branch_count().to_string(),
    );

    vars
}

/// Resolve a default value expression for a parameter type using type_defaults patterns.
///
/// Returns `(value_expr, call_arg_expr, imports)` where:
/// - `value_expr` is the `let` binding right-hand side (e.g., `String::new()`)
/// - `call_arg_expr` is what to pass in the function call (e.g., `&name` for `&str` params)
/// - `imports` are any extra `use` statements needed
fn resolve_type_default<'a>(
    param_type: &str,
    type_defaults: &'a [TypeDefault],
) -> (String, Option<String>, Vec<&'a str>) {
    for td in type_defaults {
        if let Ok(re) = Regex::new(&td.pattern) {
            if re.is_match(param_type) {
                let imports: Vec<&str> = td.imports.iter().map(|s| s.as_str()).collect();
                return (td.value.clone(), None, imports);
            }
        }
    }
    // Fallback: Default::default()
    ("Default::default()".to_string(), None, vec![])
}

/// Build parameter setup lines, call arguments, and extra imports from type_defaults.
///
/// Returns `(setup_lines, call_args, extra_imports)` where:
/// - `setup_lines` is newline-separated `let` bindings
/// - `call_args` is comma-separated arguments for the function call
/// - `extra_imports` is newline-separated `use` statements
fn build_param_inputs(params: &[Param], type_defaults: &[TypeDefault]) -> (String, String, String) {
    if params.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let mut setup_lines = Vec::new();
    let mut call_args = Vec::new();
    let mut all_imports: Vec<String> = Vec::new();

    for param in params {
        let (value_expr, call_override, imports) =
            resolve_type_default(&param.param_type, type_defaults);

        // Build the let binding
        setup_lines.push(format!("        let {} = {};", param.name, value_expr));

        // Build the call argument — if the type is a reference, borrow the variable
        let call_arg = call_override.unwrap_or_else(|| {
            let trimmed = param.param_type.trim();
            if trimmed.starts_with('&') {
                format!("&{}", param.name)
            } else {
                param.name.clone()
            }
        });
        call_args.push(call_arg);

        for imp in imports {
            let imp_string = imp.to_string();
            if !all_imports.contains(&imp_string) {
                all_imports.push(imp_string);
            }
        }
    }

    let setup = setup_lines.join("\n");
    let args = call_args.join(", ");
    let imports = all_imports.join("\n");
    (setup, args, imports)
}

/// Derive the template key from the return type shape and the branch's return variant.
fn derive_template_key(return_type: &ReturnShape, returns: &ReturnValue) -> String {
    match return_type {
        ReturnShape::ResultType { .. } => format!("result_{}", returns.variant),
        ReturnShape::OptionType { .. } => format!("option_{}", returns.variant),
        ReturnShape::Bool => format!("bool_{}", returns.variant),
        ReturnShape::Unit => "unit".to_string(),
        ReturnShape::Collection { .. } => "collection".to_string(),
        _ => format!("value_{}", returns.variant),
    }
}

/// Convert a condition string to a snake_case slug suitable for a test name.
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == ' ' || c == '.' || c == ':' || c == '-' || c == '_' {
                '_'
            } else {
                '_'
            }
        })
        .collect::<String>()
        // Collapse multiple underscores
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        // Truncate to reasonable length
        .chars()
        .take(60)
        .collect()
}

// ── Behavioral inference ──
//
// These functions analyze branch conditions and return values to produce
// setup code and assertions that exercise specific behavior, not just
// smoke-test that the function compiles.

/// Overridden setup derived from a branch condition.
struct SetupOverride {
    /// Newline-separated `let` bindings (8-space indented).
    setup_lines: String,
    /// Comma-separated call arguments.
    call_args: String,
    /// Extra `use` imports needed.
    extra_imports: String,
}

/// Infer parameter setup code from a branch condition string.
///
/// Pattern-matches the condition against known idioms to produce inputs
/// that actually trigger the branch. Returns `None` if no condition-specific
/// setup can be inferred (falling back to generic type_defaults).
fn infer_setup_from_condition(
    condition: &str,
    params: &[Param],
    type_defaults: &[TypeDefault],
) -> Option<SetupOverride> {
    // Build baseline setup, then try to override specific params based on condition
    let condition_lower = condition.to_lowercase();

    // Try to find a condition-specific override for at least one parameter
    let mut overrides: HashMap<String, ConditionParamOverride> = HashMap::new();

    for param in params {
        if let Some(ovr) = match_condition_to_param(condition, &condition_lower, param) {
            overrides.insert(param.name.clone(), ovr);
        }
    }

    if overrides.is_empty() {
        return None;
    }

    // Rebuild param setup with overrides applied
    let mut setup_lines = Vec::new();
    let mut call_args = Vec::new();
    let mut all_imports: Vec<String> = Vec::new();

    for param in params {
        let (value_expr, call_arg, imports) = if let Some(ovr) = overrides.get(&param.name) {
            (
                ovr.value_expr.clone(),
                ovr.call_arg.clone().unwrap_or_else(|| {
                    if param.param_type.trim().starts_with('&') {
                        format!("&{}", param.name)
                    } else {
                        param.name.clone()
                    }
                }),
                ovr.imports.clone(),
            )
        } else {
            let (val, call_override, imps) = resolve_type_default(&param.param_type, type_defaults);
            let call = call_override.unwrap_or_else(|| {
                if param.param_type.trim().starts_with('&') {
                    format!("&{}", param.name)
                } else {
                    param.name.clone()
                }
            });
            let imp_strs: Vec<String> = imps.into_iter().map(|s| s.to_string()).collect();
            (val, call, imp_strs)
        };

        setup_lines.push(format!("        let {} = {};", param.name, value_expr));
        call_args.push(call_arg);

        for imp in imports {
            if !all_imports.contains(&imp) {
                all_imports.push(imp);
            }
        }
    }

    Some(SetupOverride {
        setup_lines: setup_lines.join("\n"),
        call_args: call_args.join(", "),
        extra_imports: all_imports.join("\n"),
    })
}

/// A condition-derived override for a single parameter's setup.
struct ConditionParamOverride {
    /// The value expression to use in the `let` binding.
    value_expr: String,
    /// Optional override for the call argument (if different from `&name` / `name`).
    call_arg: Option<String>,
    /// Extra imports needed.
    imports: Vec<String>,
}

/// Try to match a branch condition to a specific parameter and derive a
/// value that triggers the condition.
///
/// This is the heart of behavioral inference. It pattern-matches known
/// condition idioms (`.is_empty()`, `.exists()`, `.is_some()`, etc.)
/// against the parameter's name and type to produce a value that makes
/// the condition true.
fn match_condition_to_param(
    condition: &str,
    condition_lower: &str,
    param: &Param,
) -> Option<ConditionParamOverride> {
    let pname = &param.name;
    let ptype = &param.param_type;

    // ── Pattern: negated emptiness — "!param.is_empty()" or "not empty" ──
    // Check negated BEFORE non-negated to avoid false matches
    if condition_contains_negated_method(condition, pname, "is_empty") {
        return Some(make_non_empty_value(pname, ptype));
    }

    // ── Pattern: "param.is_empty()" or "param is empty" ──
    // Applies to Vec, slice, String, &str, HashMap, HashSet
    if condition_contains_param_method(condition_lower, pname, "is_empty") {
        return Some(make_empty_value(ptype));
    }

    // ── Pattern: "param.is_none()" or "param is None" ──
    if (condition_contains_param_method(condition_lower, pname, "is_none")
        || (condition_lower.contains(&pname.to_lowercase()) && condition_lower.contains("none")))
        && ptype.starts_with("Option")
    {
        return Some(ConditionParamOverride {
            value_expr: "None".to_string(),
            call_arg: None,
            imports: vec![],
        });
    }

    // ── Pattern: "param.is_some()" or "param is Some" ──
    if (condition_contains_param_method(condition_lower, pname, "is_some")
        || (condition_lower.contains(&pname.to_lowercase()) && condition_lower.contains("some")))
        && ptype.starts_with("Option")
    {
        return Some(ConditionParamOverride {
            value_expr: "Some(Default::default())".to_string(),
            call_arg: None,
            imports: vec![],
        });
    }

    // ── Pattern: path existence — "path doesn't exist", "not exists", "!path.exists()" ──
    if is_path_type(ptype) {
        if condition_lower.contains("doesn't exist")
            || condition_lower.contains("does not exist")
            || condition_lower.contains("not exist")
            || condition_contains_negated_method(condition, pname, "exists")
        {
            return Some(ConditionParamOverride {
                value_expr: r#"Path::new("/tmp/nonexistent_test_path_818")"#.to_string(),
                call_arg: None,
                imports: vec!["use std::path::Path;".to_string()],
            });
        }
        if condition_contains_param_method(condition_lower, pname, "exists")
            && !condition_lower.contains("not")
            && !condition.contains('!')
        {
            // Path must exist — use a temp dir
            return Some(ConditionParamOverride {
                value_expr: "tempfile::tempdir().unwrap()".to_string(),
                call_arg: Some(format!("{}.path()", pname)),
                imports: vec![],
            });
        }
    }

    // ── Pattern: boolean params — "param" or "!param" or "param == true/false" ──
    if ptype.trim() == "bool" {
        if condition_lower.contains(&format!("!{}", pname.to_lowercase()))
            || condition_lower.contains(&format!("{} == false", pname.to_lowercase()))
            || condition_lower.contains(&format!("{} is false", pname.to_lowercase()))
        {
            return Some(ConditionParamOverride {
                value_expr: "false".to_string(),
                call_arg: None,
                imports: vec![],
            });
        }
        if condition_lower == pname.to_lowercase()
            || condition_lower.contains(&format!("{} == true", pname.to_lowercase()))
            || condition_lower.contains(&format!("{} is true", pname.to_lowercase()))
        {
            return Some(ConditionParamOverride {
                value_expr: "true".to_string(),
                call_arg: None,
                imports: vec![],
            });
        }
    }

    // ── Pattern: numeric comparisons — "param > 0", "param == 0", "param.len() > X" ──
    if is_numeric_type(ptype) {
        if condition_lower.contains(&format!("{} == 0", pname.to_lowercase()))
            || condition_lower.contains(&format!("{} < 1", pname.to_lowercase()))
        {
            return Some(ConditionParamOverride {
                value_expr: "0".to_string(),
                call_arg: None,
                imports: vec![],
            });
        }
        if condition_lower.contains(&format!("{} > 0", pname.to_lowercase()))
            || condition_lower.contains(&format!("{} >= 1", pname.to_lowercase()))
        {
            return Some(ConditionParamOverride {
                value_expr: "1".to_string(),
                call_arg: None,
                imports: vec![],
            });
        }
    }

    // ── Pattern: string content — "param.contains(X)" or "param.starts_with(X)" ──
    if is_string_type(ptype) {
        // Extract the literal from contains/starts_with if possible
        if let Some(literal) = extract_method_string_arg(condition, pname, "contains") {
            return Some(ConditionParamOverride {
                value_expr: format!("\"{}\"", literal),
                call_arg: None,
                imports: vec![],
            });
        }
        if let Some(literal) = extract_method_string_arg(condition, pname, "starts_with") {
            return Some(ConditionParamOverride {
                value_expr: format!("\"{}\"", literal),
                call_arg: None,
                imports: vec![],
            });
        }
    }

    None
}

/// Check if condition contains `param.method()` (case-insensitive).
fn condition_contains_param_method(condition_lower: &str, param: &str, method: &str) -> bool {
    let pattern = format!("{}.{}(", param.to_lowercase(), method);
    condition_lower.contains(&pattern)
}

/// Check if condition contains a negated method call: `!param.method()`.
fn condition_contains_negated_method(condition: &str, param: &str, method: &str) -> bool {
    let pattern = format!("!{}.{}(", param, method);
    condition.contains(&pattern)
}

/// Produce a value that makes `.is_empty()` return true for the given type.
fn make_empty_value(ptype: &str) -> ConditionParamOverride {
    let trimmed = ptype.trim();
    if trimmed.starts_with("&[") || trimmed.starts_with("Vec<") {
        ConditionParamOverride {
            value_expr: "Vec::new()".to_string(),
            call_arg: None,
            imports: vec![],
        }
    } else if trimmed == "&str" || trimmed == "String" || trimmed == "&String" {
        ConditionParamOverride {
            value_expr: r#""""#.to_string(),
            call_arg: None,
            imports: vec![],
        }
    } else if trimmed.starts_with("HashMap") {
        ConditionParamOverride {
            value_expr: "HashMap::new()".to_string(),
            call_arg: None,
            imports: vec!["use std::collections::HashMap;".to_string()],
        }
    } else if trimmed.starts_with("HashSet") {
        ConditionParamOverride {
            value_expr: "HashSet::new()".to_string(),
            call_arg: None,
            imports: vec!["use std::collections::HashSet;".to_string()],
        }
    } else {
        // Generic fallback for empty collections
        ConditionParamOverride {
            value_expr: "Default::default()".to_string(),
            call_arg: None,
            imports: vec![],
        }
    }
}

/// Produce a value that makes `.is_empty()` return false for the given type.
fn make_non_empty_value(pname: &str, ptype: &str) -> ConditionParamOverride {
    let trimmed = ptype.trim();
    if trimmed.starts_with("&[") || trimmed.starts_with("Vec<") {
        ConditionParamOverride {
            value_expr: "vec![Default::default()]".to_string(),
            call_arg: None,
            imports: vec![],
        }
    } else if trimmed == "&str" || trimmed == "String" || trimmed == "&String" {
        ConditionParamOverride {
            value_expr: format!("\"test_{}\"", pname),
            call_arg: None,
            imports: vec![],
        }
    } else {
        ConditionParamOverride {
            value_expr: "Default::default()".to_string(),
            call_arg: None,
            imports: vec![],
        }
    }
}

/// Check if a type represents a filesystem path.
fn is_path_type(ptype: &str) -> bool {
    let t = ptype.trim();
    t == "&Path" || t == "&PathBuf" || t == "PathBuf" || t == "&std::path::Path"
}

/// Check if a type is numeric.
fn is_numeric_type(ptype: &str) -> bool {
    let t = ptype.trim();
    matches!(
        t,
        "usize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "isize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "f32"
            | "f64"
    )
}

/// Check if a type is string-like.
fn is_string_type(ptype: &str) -> bool {
    let t = ptype.trim();
    t == "&str" || t == "String" || t == "&String"
}

/// Extract a string literal argument from a method call in a condition.
///
/// E.g., from `name.contains("foo")` extracts `"foo"`.
fn extract_method_string_arg(condition: &str, param: &str, method: &str) -> Option<String> {
    let pattern = format!("{}.{}(\"", param, method);
    if let Some(start) = condition.find(&pattern) {
        let after = &condition[start + pattern.len()..];
        if let Some(end) = after.find('"') {
            return Some(after[..end].to_string());
        }
    }
    // Also try single-quote variant
    let pattern_sq = format!("{}.{}('", param, method);
    if let Some(start) = condition.find(&pattern_sq) {
        let after = &condition[start + pattern_sq.len()..];
        if let Some(end) = after.find('\'') {
            return Some(after[..end].to_string());
        }
    }
    None
}

/// Infer an assertion code string from the branch return and function return type.
///
/// Produces assertion code that checks the actual return value, not just the
/// type variant. Falls back to the standard `is_ok()`/`is_err()` checks when
/// no more specific assertion can be derived.
fn infer_assertion(returns: &ReturnValue, return_type: &ReturnShape, condition: &str) -> String {
    let indent = "        ";

    match return_type {
        ReturnShape::ResultType { .. } => {
            match returns.variant.as_str() {
                "ok" => {
                    if let Some(ref val) = returns.value {
                        // We know the Ok value description — assert it
                        format!(
                            "{indent}let inner = result.unwrap();\n\
                             {indent}// Branch returns Ok({val}) when: {condition}\n\
                             {indent}let _ = inner; // TODO: assert specific value for \"{val}\"",
                        )
                    } else {
                        format!(
                            "{indent}assert!(result.is_ok(), \"expected Ok for: {condition}\");",
                        )
                    }
                }
                "err" => {
                    if let Some(ref val) = returns.value {
                        format!(
                            "{indent}let err = result.unwrap_err();\n\
                             {indent}// Branch returns Err({val}) when: {condition}\n\
                             {indent}let err_msg = format!(\"{{:?}}\", err);\n\
                             {indent}let _ = err_msg; // TODO: assert error contains \"{val}\"",
                        )
                    } else {
                        format!(
                            "{indent}assert!(result.is_err(), \"expected Err for: {condition}\");",
                        )
                    }
                }
                _ => format!("{indent}let _ = result; // variant: {}", returns.variant),
            }
        }
        ReturnShape::OptionType { .. } => match returns.variant.as_str() {
            "some" => {
                if let Some(ref val) = returns.value {
                    format!(
                        "{indent}let inner = result.expect(\"expected Some for: {condition}\");\n\
                             {indent}// Branch returns Some({val})\n\
                             {indent}let _ = inner; // TODO: assert value matches \"{val}\"",
                    )
                } else {
                    format!(
                        "{indent}assert!(result.is_some(), \"expected Some for: {condition}\");",
                    )
                }
            }
            "none" => {
                format!("{indent}assert!(result.is_none(), \"expected None for: {condition}\");",)
            }
            _ => format!("{indent}let _ = result; // variant: {}", returns.variant),
        },
        ReturnShape::Bool => match returns.variant.as_str() {
            "true" => format!("{indent}assert!(result, \"expected true when: {condition}\");",),
            "false" => format!("{indent}assert!(!result, \"expected false when: {condition}\");",),
            _ => format!("{indent}let _ = result;"),
        },
        ReturnShape::Collection { .. } => {
            // For collections, check emptiness based on condition
            if condition.contains("empty") || condition.contains("is_empty") {
                format!(
                    "{indent}assert!(result.is_empty(), \"expected empty collection for: {condition}\");",
                )
            } else {
                format!(
                    "{indent}assert!(!result.is_empty(), \"expected non-empty collection for: {condition}\");",
                )
            }
        }
        _ => {
            format!("{indent}let _ = result;")
        }
    }
}

/// Merge new import lines into existing imports, deduplicating.
fn merge_imports(existing: &str, new_imports: &str) -> String {
    let mut all: Vec<String> = Vec::new();
    for line in existing.lines().chain(new_imports.lines()) {
        let trimmed = line.trim().to_string();
        if !trimmed.is_empty() && !all.contains(&trimmed) {
            all.push(trimmed);
        }
    }
    all.join("\n")
}

// ── End-to-end API ──

/// Generated test output with source code and metadata.
pub struct GeneratedTestOutput {
    /// The rendered test source code (test functions only, no module wrapper).
    pub test_source: String,
    /// Extra `use` imports needed by the generated default values.
    pub extra_imports: Vec<String>,
    /// The function names that tests were generated for.
    pub tested_functions: Vec<String>,
}

/// Generate test source code for all functions in a source file.
///
/// This is the full pipeline: grammar → contracts → test plans → rendered source.
/// Returns `None` if the grammar has no contract or test_templates section.
pub fn generate_tests_for_file(
    content: &str,
    file_path: &str,
    grammar: &crate::extension::grammar::Grammar,
) -> Option<GeneratedTestOutput> {
    let contract_grammar = grammar.contract.as_ref()?;

    // Must have test templates to render
    if contract_grammar.test_templates.is_empty() {
        return None;
    }

    // Extract contracts
    let contracts =
        super::contract_extract::extract_contracts_from_grammar(content, file_path, grammar)?;

    if contracts.is_empty() {
        return None;
    }

    // Generate and render test plans
    let mut test_source = String::new();
    let mut all_extra_imports: Vec<String> = Vec::new();
    let mut tested_functions = Vec::new();

    for contract in &contracts {
        // Skip test functions, private functions, and trivial functions
        if contract.name.starts_with("test_") {
            continue;
        }
        if !contract.signature.is_public {
            continue;
        }

        let plan = generate_test_plan(contract, &contract_grammar.type_defaults);
        if plan.cases.is_empty() {
            continue;
        }

        // Collect extra imports from case variables
        for case in &plan.cases {
            if let Some(imports_str) = case.variables.get("extra_imports") {
                for imp in imports_str.lines() {
                    let imp = imp.trim().to_string();
                    if !imp.is_empty() && !all_extra_imports.contains(&imp) {
                        all_extra_imports.push(imp);
                    }
                }
            }
        }

        let rendered = render_test_plan(&plan, &contract_grammar.test_templates);
        if !rendered.trim().is_empty() {
            tested_functions.push(contract.name.clone());
            test_source.push_str(&rendered);
        }
    }

    if test_source.trim().is_empty() {
        None
    } else {
        Some(GeneratedTestOutput {
            test_source,
            extra_imports: all_extra_imports,
            tested_functions,
        })
    }
}

/// Generate test source code for specific methods in a source file.
///
/// Like `generate_tests_for_file`, but only generates tests for functions
/// whose names are in `method_names`. Used for MissingTestMethod findings
/// where the test file exists but specific methods lack coverage.
pub fn generate_tests_for_methods(
    content: &str,
    file_path: &str,
    grammar: &crate::extension::grammar::Grammar,
    method_names: &[&str],
) -> Option<GeneratedTestOutput> {
    let contract_grammar = grammar.contract.as_ref()?;

    if contract_grammar.test_templates.is_empty() {
        return None;
    }

    let contracts =
        super::contract_extract::extract_contracts_from_grammar(content, file_path, grammar)?;

    if contracts.is_empty() {
        return None;
    }

    let mut test_source = String::new();
    let mut all_extra_imports: Vec<String> = Vec::new();
    let mut tested_functions = Vec::new();

    for contract in &contracts {
        // Only generate tests for the requested methods
        if !method_names.contains(&contract.name.as_str()) {
            continue;
        }

        let plan = generate_test_plan(contract, &contract_grammar.type_defaults);
        if plan.cases.is_empty() {
            continue;
        }

        for case in &plan.cases {
            if let Some(imports_str) = case.variables.get("extra_imports") {
                for imp in imports_str.lines() {
                    let imp = imp.trim().to_string();
                    if !imp.is_empty() && !all_extra_imports.contains(&imp) {
                        all_extra_imports.push(imp);
                    }
                }
            }
        }

        let rendered = render_test_plan(&plan, &contract_grammar.test_templates);
        if !rendered.trim().is_empty() {
            tested_functions.push(contract.name.clone());
            test_source.push_str(&rendered);
        }
    }

    if test_source.trim().is_empty() {
        None
    } else {
        Some(GeneratedTestOutput {
            test_source,
            extra_imports: all_extra_imports,
            tested_functions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result_contract() -> FunctionContract {
        FunctionContract {
            name: "validate_write".to_string(),
            file: "src/core/engine/validate_write.rs".to_string(),
            line: 86,
            signature: Signature {
                params: vec![
                    Param {
                        name: "root".to_string(),
                        param_type: "&Path".to_string(),
                        mutable: false,
                        has_default: false,
                    },
                    Param {
                        name: "changed_files".to_string(),
                        param_type: "&[PathBuf]".to_string(),
                        mutable: false,
                        has_default: false,
                    },
                ],
                return_type: ReturnShape::ResultType {
                    ok_type: "ValidationResult".to_string(),
                    err_type: "Error".to_string(),
                },
                receiver: None,
                is_public: true,
                is_async: false,
                generics: vec![],
            },
            branches: vec![
                Branch {
                    condition: "changed_files.is_empty()".to_string(),
                    returns: ReturnValue {
                        variant: "ok".to_string(),
                        value: Some("skipped".to_string()),
                    },
                    effects: vec![],
                    line: Some(91),
                },
                Branch {
                    condition: "validation command fails".to_string(),
                    returns: ReturnValue {
                        variant: "ok".to_string(),
                        value: Some("failed".to_string()),
                    },
                    effects: vec![Effect::ProcessSpawn {
                        command: Some("sh".to_string()),
                    }],
                    line: Some(130),
                },
            ],
            early_returns: 2,
            effects: vec![Effect::ProcessSpawn {
                command: Some("sh".to_string()),
            }],
            calls: vec![],
        }
    }

    fn sample_option_contract() -> FunctionContract {
        FunctionContract {
            name: "find_item".to_string(),
            file: "src/lib.rs".to_string(),
            line: 10,
            signature: Signature {
                params: vec![Param {
                    name: "key".to_string(),
                    param_type: "&str".to_string(),
                    mutable: false,
                    has_default: false,
                }],
                return_type: ReturnShape::OptionType {
                    some_type: "Item".to_string(),
                },
                receiver: Some(Receiver::Ref),
                is_public: true,
                is_async: false,
                generics: vec![],
            },
            branches: vec![
                Branch {
                    condition: "key found".to_string(),
                    returns: ReturnValue {
                        variant: "some".to_string(),
                        value: Some("item".to_string()),
                    },
                    effects: vec![],
                    line: Some(15),
                },
                Branch {
                    condition: "key not found".to_string(),
                    returns: ReturnValue {
                        variant: "none".to_string(),
                        value: None,
                    },
                    effects: vec![],
                    line: Some(20),
                },
            ],
            early_returns: 0,
            effects: vec![],
            calls: vec![],
        }
    }

    fn sample_type_defaults() -> Vec<TypeDefault> {
        vec![
            TypeDefault {
                pattern: r"^&str$".to_string(),
                value: r#""""#.to_string(),
                imports: vec![],
            },
            TypeDefault {
                pattern: r"^&Path$".to_string(),
                value: r#"Path::new("")"#.to_string(),
                imports: vec!["use std::path::Path;".to_string()],
            },
            TypeDefault {
                pattern: r"^&\[.*\]$".to_string(),
                value: "Vec::new()".to_string(),
                imports: vec![],
            },
        ]
    }

    #[test]
    fn test_plan_generates_one_case_per_branch() {
        let contract = sample_result_contract();
        let plan = generate_test_plan(&contract, &[]);
        // 2 branches + 1 effect test
        assert_eq!(plan.cases.len(), 3);
        assert_eq!(plan.function_name, "validate_write");
    }

    #[test]
    fn test_plan_names_are_descriptive() {
        let contract = sample_result_contract();
        let plan = generate_test_plan(&contract, &[]);
        assert!(plan.cases[0].test_name.starts_with("test_validate_write_"));
        assert!(plan.cases[0].test_name.contains("empty"));
    }

    #[test]
    fn test_plan_template_keys_match_return_shape() {
        let contract = sample_result_contract();
        let plan = generate_test_plan(&contract, &[]);
        assert_eq!(plan.cases[0].template_key, "result_ok");
        assert_eq!(plan.cases[1].template_key, "result_ok");
    }

    #[test]
    fn test_plan_for_option_type() {
        let contract = sample_option_contract();
        let plan = generate_test_plan(&contract, &[]);
        assert_eq!(plan.cases.len(), 2);
        assert_eq!(plan.cases[0].template_key, "option_some");
        assert_eq!(plan.cases[1].template_key, "option_none");
    }

    #[test]
    fn test_plan_pure_function_no_effect_test() {
        let contract = sample_option_contract();
        let plan = generate_test_plan(&contract, &[]);
        // Pure function — no effect test case
        assert!(plan.cases.iter().all(|c| c.template_key != "effects"));
    }

    #[test]
    fn test_plan_variables_contain_fn_info() {
        let contract = sample_result_contract();
        let plan = generate_test_plan(&contract, &[]);
        let vars = &plan.cases[0].variables;
        assert_eq!(vars.get("fn_name").unwrap(), "validate_write");
        assert_eq!(vars.get("param_names").unwrap(), "root, changed_files");
        assert_eq!(vars.get("return_shape").unwrap(), "result");
    }

    #[test]
    fn test_plan_with_type_defaults_generates_param_setup() {
        let contract = sample_result_contract();
        let type_defaults = sample_type_defaults();
        let plan = generate_test_plan(&contract, &type_defaults);
        let vars = &plan.cases[0].variables;

        let param_setup = vars.get("param_setup").unwrap();
        assert!(
            param_setup.contains("let root ="),
            "should have root binding"
        );
        assert!(
            param_setup.contains("let changed_files ="),
            "should have changed_files binding"
        );
        assert!(
            param_setup.contains("Path::new"),
            "should use Path::new for &Path"
        );
        assert!(
            param_setup.contains("Vec::new()"),
            "should use Vec::new() for &[PathBuf]"
        );

        let param_args = vars.get("param_args").unwrap();
        assert!(
            param_args.contains("&root"),
            "should borrow root for &Path param"
        );
        assert!(
            param_args.contains("&changed_files"),
            "should borrow changed_files for &[PathBuf] param"
        );

        let extra_imports = vars.get("extra_imports").unwrap();
        assert!(
            extra_imports.contains("use std::path::Path;"),
            "should include Path import"
        );
    }

    #[test]
    fn test_render_test_plan_with_templates() {
        let contract = sample_result_contract();
        let plan = generate_test_plan(&contract, &[]);

        let mut templates = HashMap::new();
        templates.insert(
            "result_ok".to_string(),
            "#[test]\nfn {test_name}() {{\n    // {fn_name} should return Ok\n}}\n".to_string(),
        );

        let output = render_test_plan(&plan, &templates);
        assert!(output.contains("#[test]"));
        assert!(output.contains("test_validate_write_"));
        assert!(output.contains("validate_write should return Ok"));
    }

    #[test]
    fn test_render_test_plan_missing_template_uses_default() {
        let contract = sample_option_contract();
        let plan = generate_test_plan(&contract, &[]);

        let mut templates = HashMap::new();
        templates.insert(
            "default".to_string(),
            "#[test]\nfn {test_name}() {{ todo!() }}\n".to_string(),
        );

        let output = render_test_plan(&plan, &templates);
        assert!(output.contains("test_find_item_"));
        assert!(output.contains("todo!()"));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(
            slugify("changed_files.is_empty()"),
            "changed_files_is_empty"
        );
        assert_eq!(
            slugify("validation command fails"),
            "validation_command_fails"
        );
        assert_eq!(slugify("default path"), "default_path");
    }

    #[test]
    fn test_empty_branches_generates_no_panic_test() {
        let mut contract = sample_result_contract();
        contract.branches.clear();
        contract.effects.clear();
        let plan = generate_test_plan(&contract, &[]);
        assert_eq!(plan.cases.len(), 1);
        assert_eq!(plan.cases[0].template_key, "no_panic");
    }

    // ── Behavioral inference tests ──

    #[test]
    fn test_setup_infers_empty_vec_for_is_empty_condition() {
        let params = vec![Param {
            name: "items".to_string(),
            param_type: "Vec<String>".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("items.is_empty()", &params, &[]);
        assert!(result.is_some(), "should infer setup for is_empty");
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("Vec::new()"),
            "should use Vec::new() for empty vec, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_infers_empty_slice_for_is_empty_condition() {
        let params = vec![Param {
            name: "changed_files".to_string(),
            param_type: "&[PathBuf]".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("changed_files.is_empty()", &params, &[]);
        assert!(result.is_some(), "should infer setup for slice is_empty");
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("Vec::new()"),
            "should use Vec::new() for empty slice, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_infers_non_empty_vec_for_negated_is_empty() {
        let params = vec![Param {
            name: "commits".to_string(),
            param_type: "Vec<String>".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("!commits.is_empty()", &params, &[]);
        assert!(result.is_some(), "should infer setup for negated is_empty");
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("vec![Default::default()]"),
            "should use vec![Default::default()] for non-empty vec, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_infers_nonexistent_path() {
        let params = vec![Param {
            name: "path".to_string(),
            param_type: "&Path".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("path doesn't exist", &params, &[]);
        assert!(result.is_some(), "should infer setup for nonexistent path");
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("nonexistent"),
            "should use a nonexistent path, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_infers_none_for_option_is_none() {
        let params = vec![Param {
            name: "config".to_string(),
            param_type: "Option<Config>".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("config.is_none()", &params, &[]);
        assert!(result.is_some(), "should infer setup for is_none");
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("None"),
            "should use None for is_none, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_infers_some_for_option_is_some() {
        let params = vec![Param {
            name: "config".to_string(),
            param_type: "Option<String>".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("config.is_some()", &params, &[]);
        assert!(result.is_some(), "should infer setup for is_some");
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("Some("),
            "should use Some(...) for is_some, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_returns_none_for_unrecognized_condition() {
        let params = vec![Param {
            name: "x".to_string(),
            param_type: "CustomType".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("some random condition", &params, &[]);
        assert!(
            result.is_none(),
            "should return None for unrecognized condition"
        );
    }

    #[test]
    fn test_setup_preserves_non_overridden_params() {
        let type_defaults = sample_type_defaults();
        let params = vec![
            Param {
                name: "root".to_string(),
                param_type: "&Path".to_string(),
                mutable: false,
                has_default: false,
            },
            Param {
                name: "items".to_string(),
                param_type: "&[String]".to_string(),
                mutable: false,
                has_default: false,
            },
        ];
        // Condition only targets items, root should keep its type_default
        let result = infer_setup_from_condition("items.is_empty()", &params, &type_defaults);
        assert!(result.is_some());
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("let root = Path::new"),
            "root should keep its type_default, got: {}",
            so.setup_lines
        );
        assert!(
            so.setup_lines.contains("let items = Vec::new()"),
            "items should be overridden to Vec::new(), got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_assertion_result_ok_with_value() {
        let returns = ReturnValue {
            variant: "ok".to_string(),
            value: Some("skipped".to_string()),
        };
        let return_type = ReturnShape::ResultType {
            ok_type: "ValidationResult".to_string(),
            err_type: "Error".to_string(),
        };
        let assertion = infer_assertion(&returns, &return_type, "items.is_empty()");
        assert!(
            assertion.contains("unwrap()"),
            "should unwrap Ok value, got: {}",
            assertion
        );
        assert!(
            assertion.contains("skipped"),
            "should mention the expected value, got: {}",
            assertion
        );
    }

    #[test]
    fn test_assertion_result_err_with_value() {
        let returns = ReturnValue {
            variant: "err".to_string(),
            value: Some("not found".to_string()),
        };
        let return_type = ReturnShape::ResultType {
            ok_type: "String".to_string(),
            err_type: "Error".to_string(),
        };
        let assertion = infer_assertion(&returns, &return_type, "path doesn't exist");
        assert!(
            assertion.contains("unwrap_err()"),
            "should unwrap Err, got: {}",
            assertion
        );
        assert!(
            assertion.contains("not found"),
            "should mention the error description, got: {}",
            assertion
        );
    }

    #[test]
    fn test_assertion_result_ok_without_value_falls_back() {
        let returns = ReturnValue {
            variant: "ok".to_string(),
            value: None,
        };
        let return_type = ReturnShape::ResultType {
            ok_type: "()".to_string(),
            err_type: "Error".to_string(),
        };
        let assertion = infer_assertion(&returns, &return_type, "default path");
        assert!(
            assertion.contains("is_ok()"),
            "should fall back to is_ok(), got: {}",
            assertion
        );
    }

    #[test]
    fn test_assertion_option_none() {
        let returns = ReturnValue {
            variant: "none".to_string(),
            value: None,
        };
        let return_type = ReturnShape::OptionType {
            some_type: "Item".to_string(),
        };
        let assertion = infer_assertion(&returns, &return_type, "key not found");
        assert!(
            assertion.contains("is_none()"),
            "should assert is_none(), got: {}",
            assertion
        );
    }

    #[test]
    fn test_assertion_bool_true() {
        let returns = ReturnValue {
            variant: "true".to_string(),
            value: None,
        };
        let return_type = ReturnShape::Bool;
        let assertion = infer_assertion(&returns, &return_type, "input is valid");
        assert!(
            assertion.contains("assert!(result"),
            "should assert true, got: {}",
            assertion
        );
        assert!(
            !assertion.contains("!result"),
            "should NOT negate, got: {}",
            assertion
        );
    }

    #[test]
    fn test_assertion_collection_empty_condition() {
        let returns = ReturnValue {
            variant: "value".to_string(),
            value: None,
        };
        let return_type = ReturnShape::Collection {
            element_type: "String".to_string(),
        };
        let assertion = infer_assertion(&returns, &return_type, "input.is_empty()");
        assert!(
            assertion.contains("is_empty()"),
            "should assert emptiness, got: {}",
            assertion
        );
    }

    #[test]
    fn test_behavioral_plan_overrides_setup_for_is_empty_branch() {
        let contract = sample_result_contract();
        let type_defaults = sample_type_defaults();
        let plan = generate_test_plan(&contract, &type_defaults);

        // First branch: changed_files.is_empty()
        let vars = &plan.cases[0].variables;
        let setup = vars.get("param_setup").unwrap();
        assert!(
            setup.contains("let changed_files = Vec::new()"),
            "should override changed_files to empty vec for is_empty branch, got:\n{}",
            setup
        );
        // root should still use its type_default (Path::new)
        assert!(
            setup.contains("let root = Path::new"),
            "root should keep its type_default, got:\n{}",
            setup
        );
    }

    #[test]
    fn test_behavioral_plan_generates_assertion_code() {
        let contract = sample_result_contract();
        let type_defaults = sample_type_defaults();
        let plan = generate_test_plan(&contract, &type_defaults);

        // First branch returns Ok("skipped") — should have rich assertion
        let vars = &plan.cases[0].variables;
        let assertion = vars.get("assertion_code").unwrap();
        assert!(
            assertion.contains("unwrap()") || assertion.contains("is_ok()"),
            "should have an assertion for Ok branch, got:\n{}",
            assertion
        );
    }

    #[test]
    fn test_setup_bool_param_true() {
        let params = vec![Param {
            name: "verbose".to_string(),
            param_type: "bool".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("verbose == true", &params, &[]);
        assert!(result.is_some());
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("true"),
            "should set bool to true, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_numeric_param_zero() {
        let params = vec![Param {
            name: "count".to_string(),
            param_type: "usize".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("count == 0", &params, &[]);
        assert!(result.is_some());
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("let count = 0"),
            "should set count to 0, got: {}",
            so.setup_lines
        );
    }

    #[test]
    fn test_setup_string_contains() {
        let params = vec![Param {
            name: "name".to_string(),
            param_type: "&str".to_string(),
            mutable: false,
            has_default: false,
        }];
        let result = infer_setup_from_condition("name.contains(\"test\")", &params, &[]);
        assert!(result.is_some());
        let so = result.unwrap();
        assert!(
            so.setup_lines.contains("\"test\""),
            "should use the literal from contains(), got: {}",
            so.setup_lines
        );
    }
}
