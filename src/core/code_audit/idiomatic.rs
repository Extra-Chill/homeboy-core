//! Shared predicates for recognizing idiomatic Rust / PHP shape — names that
//! are expected to have boilerplate bodies and test names that describe
//! behavior rather than literally repeat the production method name.
//!
//! Two distinct concerns live here, both about "don't punish idiomatic code":
//!
//! ## `is_trivial_method` — universally idiomatic method names
//!
//! Some method names — `len`, `is_empty`, `iter`, `new`, `default`, `from`,
//! `into`, `clone`, `fmt`, `as_str`, `to_string`, etc. — are **expected** to
//! have boilerplate-shaped bodies across unrelated types. That's the language
//! and stdlib doing what they're designed to do, not a code-smell.
//!
//! - **test_coverage**: don't expect a dedicated test for a method whose name
//!   is universally idiomatic. `len`/`is_empty`/`fmt` get tested transitively.
//! - **near_duplicate (duplication.rs)**: don't flag a method whose name is
//!   universally idiomatic — every collection wrapper in the Rust ecosystem
//!   defines `fn len(&self) -> usize { self.inner.len() }`, and Clippy's
//!   `len_without_is_empty` lint actually *requires* you to add `is_empty`
//!   alongside it. Treating these as duplication findings is a false positive.
//!
//! ## `test_covers_method` — behavior-describing test names
//!
//! Behavior-describing test names — the dominant Rust idiom in serde, tokio,
//! rayon, clippy, and homeboy itself — name the function under test inside
//! a longer descriptive name (`fingerprint_content_matches_fingerprint_file`
//! tests `fingerprint_content`). Strict literal-prefix matching
//! (`test_<methodname>`) misses these. The token-bounded substring predicate
//! recognizes them as coverage without false-matching `getrandom_works` to
//! `get`.
//!
//! - **test_coverage**: used by `MissingTestMethod` to detect that a source
//!   method is exercised by a behavior-describing test, even when the test
//!   name doesn't literally start with `test_<methodname>`.

/// Method names that are universally idiomatic-shape across types.
///
/// Returns true if the name is either:
/// - in a curated list of stdlib-trait / common-accessor / lifecycle method
///   names that are expected to look the same across unrelated types, or
/// - prefixed with `get_`, `is_`, or `has_` (simple getters / predicates).
pub(super) fn is_trivial_method(name: &str) -> bool {
    let trivial = [
        // Rust core trait methods
        "new",
        "default",
        "from",
        "into",
        "clone",
        "fmt",
        "display",
        "eq",
        "hash",
        "drop",
        // Rust common conversions
        "as_str",
        "as_ref",
        "as_mut",
        "to_string",
        "to_str",
        "to_owned",
        // Rust common accessors
        "is_empty",
        "len",
        "iter",
        // Serde
        "serialize",
        "deserialize",
        // Builder pattern
        "build",
        "builder",
        // PHP magic methods
        "__construct",
        "__destruct",
        "__toString",
        "__clone",
        "get_instance",
        "getInstance",
        // Test lifecycle methods (PHPUnit / WP_UnitTestCase)
        // These are optional overrides inherited from the base test class —
        // not every test class needs to define them.
        "set_up",
        "tear_down",
        "set_up_before_class",
        "tear_down_after_class",
        "setUp",
        "tearDown",
        "setUpBeforeClass",
        "tearDownAfterClass",
    ];
    if trivial.contains(&name) {
        return true;
    }
    // Prefix-based rules: simple getters/accessors/predicates
    if name.starts_with("get_") || name.starts_with("is_") || name.starts_with("has_") {
        return true;
    }
    false
}

/// Check if `test_name` covers `source_method` under the configured prefix.
///
/// Coverage is established when EITHER:
/// 1. The test name matches the literal `{prefix}{source_method}` shape
///    (the existing strict path — preserves PHPUnit-style conventions
///    where every test is `test_<methodname>`), OR
/// 2. The source method name appears as a snake_case-token-bounded
///    substring within the test name. This handles behavior-describing
///    test names — the dominant Rust idiom — like
///    `fingerprint_content_matches_fingerprint_file` covering
///    `fingerprint_content`.
///
/// Token boundary for snake_case identifiers: `_` is a separator, not part
/// of a word. So a match is token-bounded when the byte before the match
/// is start-of-string, `_`, or any non-alphanumeric byte; and the byte
/// after the match is end-of-string, `_`, or any non-alphanumeric byte.
/// This accepts `foo_handles_empty` covering `foo` (separator on the
/// right) and rejects `getrandom_works` covering `get` (alphanumeric `r`
/// on the right) or `foobar_test` covering `foo` (alphanumeric `b` on the
/// right).
///
/// Used by MissingTestMethod / coverage-presence checks and by Rust
/// orphaned-test suppression for behavior-style test names.
pub(super) fn test_covers_method(test_name: &str, source_method: &str, prefix: &str) -> bool {
    // Literal-prefix path
    if let Some(stripped) = test_name.strip_prefix(prefix) {
        if stripped == source_method {
            return true;
        }
    }

    // Token-bounded substring path
    if source_method.is_empty() {
        return false;
    }
    let bytes = test_name.as_bytes();
    let needle = source_method.as_bytes();
    let needle_len = needle.len();
    if bytes.len() < needle_len {
        return false;
    }
    let mut i = 0;
    while i + needle_len <= bytes.len() {
        if &bytes[i..i + needle_len] == needle {
            let before_ok = i == 0 || !is_word_byte(bytes[i - 1]);
            let after_ok = i + needle_len == bytes.len() || !is_word_byte(bytes[i + needle_len]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// True if `b` is part of a snake_case word (alphanumeric only). `_` is a
/// separator, not part of a word, so it counts as a token boundary.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_trivial_method_recognizes_collection_idioms() {
        // The triggering case: standard collection-wrapper boilerplate.
        // Every Vec/HashMap/String wrapper in the ecosystem looks the same
        // for these names, and Clippy's `len_without_is_empty` lint requires
        // them paired.
        assert!(is_trivial_method("len"));
        assert!(is_trivial_method("is_empty"));
        assert!(is_trivial_method("iter"));
    }

    #[test]
    fn is_trivial_method_recognizes_prefix_rules() {
        // Simple getters / predicates / capability checks.
        assert!(is_trivial_method("get_foo"));
        assert!(is_trivial_method("is_bar"));
        assert!(is_trivial_method("has_baz"));
    }

    #[test]
    fn is_trivial_method_rejects_real_methods() {
        // Domain methods with substantive bodies should not be considered
        // trivial — they carry real logic that's worth testing and worth
        // flagging if duplicated.
        assert!(!is_trivial_method("compute_fixability"));
        assert!(!is_trivial_method("from_snapshot"));
    }

    #[test]
    fn is_trivial_method_recognizes_stdlib_trait_methods() {
        // Core trait methods on the curated list.
        assert!(is_trivial_method("new"));
        assert!(is_trivial_method("default"));
        assert!(is_trivial_method("from"));
        assert!(is_trivial_method("into"));
        assert!(is_trivial_method("clone"));
        assert!(is_trivial_method("fmt"));
    }

    #[test]
    fn is_trivial_method_recognizes_php_magic_methods() {
        assert!(is_trivial_method("__construct"));
        assert!(is_trivial_method("__toString"));
        assert!(is_trivial_method("getInstance"));
    }

    // ========================================================================
    // test_covers_method — substring matching for descriptive test names (#1518)
    // ========================================================================

    #[test]
    fn test_covers_method_strict_prefix_match() {
        // Existing literal-prefix path: `test_foo` covers `foo`.
        assert!(test_covers_method("test_foo", "foo", "test_"));
    }

    #[test]
    fn test_covers_method_descriptive_match() {
        // Behavior-describing test name: `foo_handles_empty` covers `foo`.
        // The source method appears at the start, followed by `_`.
        assert!(test_covers_method("foo_handles_empty", "foo", "test_"));
    }

    #[test]
    fn test_covers_method_descriptive_match_at_end() {
        // Source method appears at the end of the test name.
        assert!(test_covers_method("handles_empty_foo", "foo", "test_"));
    }

    #[test]
    fn test_covers_method_descriptive_match_in_middle() {
        // Source method appears in the middle, surrounded by underscores.
        assert!(test_covers_method("handles_foo_empty", "foo", "test_"));
    }

    #[test]
    fn test_covers_method_rejects_substring_inside_identifier() {
        // Token-bounded substring: `getrandom_works` does NOT cover `get`
        // because `get` is followed by `r` (an identifier byte).
        assert!(!test_covers_method("getrandom_works", "get", "test_"));
        // Same idea trailing: `foobar_test` does NOT cover `foo` because
        // `foo` is followed by `b` (an identifier byte).
        assert!(!test_covers_method("foobar_test", "foo", "test_"));
        // Leading: `myfoo_test` does NOT cover `foo` because `foo` is
        // preceded by `y` (an identifier byte).
        assert!(!test_covers_method("myfoo_test", "foo", "test_"));
    }

    #[test]
    fn test_covers_method_rejects_unrelated_test() {
        // Test name doesn't contain the source method at all.
        assert!(!test_covers_method("unrelated_test", "foo", "test_"));
    }
}
