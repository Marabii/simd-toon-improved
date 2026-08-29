//! Dynamic conformance test runner.
//!
//! Reads every `*.json` fixture file under `src/tests/fixtures/` (language-agnostic
//! TOON test fixtures following `src/tests/fixtures.schema.json` https://github.com/toon-format/spec/tree/main/tests
//! ), decodes each test case's TOON `input` with this crate, and compares the result against the
//! fixture's `expected` JSON value. Only `category: "decode"` fixtures are run;
//! this crate does not implement encoding yet.

#![allow(clippy::unwrap_used)]

use crate::DecodeOptions;
use serde::Deserialize;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    category: String,
    tests: Vec<TestCase>,
}

#[derive(Deserialize)]
struct TestCase {
    name: String,
    input: serde_json::Value,
    #[serde(default)]
    expected: serde_json::Value,
    #[serde(default, rename = "shouldError")]
    should_error: bool,
    #[serde(default)]
    options: FixtureOptions,
}

/// The per test `options` object of `fixtures.schema.json`. `delimiter` is
/// encode only (a decoder reads the delimiter off the header's bracket
/// segment) so it is accepted and ignored here.
#[derive(Deserialize, Default)]
struct FixtureOptions {
    strict: Option<bool>,
    #[serde(rename = "indentSize")]
    indent_size: Option<usize>,
}

impl FixtureOptions {
    /// Turns the fixture's options into `DecodeOptions`, filling in the
    /// schema's defaults (`strict` is `true`, `indentSize` is 2).
    fn decode_options(&self) -> Result<DecodeOptions, String> {
        let mut options = DecodeOptions::new().with_strict(self.strict.unwrap_or(true));
        if let Some(indent_size) = self.indent_size {
            options = options
                .with_indent_size(indent_size)
                .map_err(|e| format!("fixture requests an unusable indentSize: {e}"))?;
        }
        Ok(options)
    }

    /// A short `strict=…, indent=…` label, or `None` when the test runs with
    /// the defaults, so failure output only mentions options that were set.
    fn label(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(strict) = self.strict {
            parts.push(format!("strict={strict}"));
        }
        if let Some(indent_size) = self.indent_size {
            parts.push(format!("indent={indent_size}"));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

/// Decodes `tc.input` (expected to be a TOON source string) and checks it
/// against `tc.expected`/`tc.should_error`. Returns `Err(reason)` on mismatch,
/// decode failure, or panic inside the decoder.
fn run_decode_test(tc: &TestCase) -> Result<(), String> {
    let Some(toon_input) = tc.input.as_str() else {
        return Err(
            "fixture `input` is not a string (decode tests require TOON source text)".to_string(),
        );
    };

    let options = tc.options.decode_options()?;

    let mut buf = toon_input.as_bytes().to_vec();
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        crate::from_slice_with_options::<serde_json::Value>(&mut buf, options)
    }));

    match outcome {
        Ok(Ok(actual)) if tc.should_error => Err(format!(
            "expected an error but decoded successfully: {actual}"
        )),
        Ok(Ok(actual)) if json_matches(&actual, &tc.expected) => Ok(()),
        Ok(Ok(actual)) => Err(format!(
            "decoded value does not match expected\n    expected: {}\n    actual:   {}",
            tc.expected, actual
        )),
        Ok(Err(_)) if tc.should_error => Ok(()),
        Ok(Err(e)) => Err(format!("decode failed: {e}")),
        Err(_) if tc.should_error => Ok(()),
        Err(payload) => Err(format!("decode panicked: {}", panic_message(&payload))),
    }
}

fn json_matches(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    match (actual, expected) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => numbers_equal(a, b),
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_matches(a, b))
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|bv| json_matches(v, bv)))
        }
        _ => actual == expected,
    }
}

/// Exact (variant-aware) equality first, since that's precise even for
/// integers too large to round-trip through `f64`; falls back to comparing
/// by value so e.g. `Float(-1000.0)` matches `PosInt`/`NegInt(-1000)`.
fn numbers_equal(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    a == b || matches!((a.as_f64(), b.as_f64()), (Some(a), Some(b)) if a == b)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[test]
fn run_fixture_conformance_tests() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures");

    let mut fixture_files: Vec<_> = fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", fixtures_dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();

    fixture_files.sort();

    assert!(
        !fixture_files.is_empty(),
        "no fixture files found in {}",
        fixtures_dir.display()
    );

    // The decoder is expected to error (or, in a few spots, panic) on invalid
    // input as part of normal conformance testing; suppress the default panic
    // hook's stderr spam for the duration of the run and restore it before we
    // report anything ourselves.
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let mut total = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for path in &fixture_files {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        let raw = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read fixture {file_name}: {e}"));
        let fixture: Fixture = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("failed to parse fixture {file_name}: {e}"));

        if fixture.category != "decode" {
            skipped += fixture.tests.len();
            continue;
        }

        for tc in &fixture.tests {
            total += 1;
            println!("conformance: running {file_name} test case: {}", tc.name);
            if let Err(reason) = run_decode_test(tc) {
                let name = match tc.options.label() {
                    Some(label) => format!("{} ({label})", tc.name),
                    None => tc.name.clone(),
                };
                failures.push(format!("[{file_name}] {name}: {reason}"));
            }
        }
    }

    panic::set_hook(previous_hook);

    println!(
        "conformance: {} passed, {} failed, {total} decode test(s) total ({skipped} non-decode test(s) skipped)",
        total - failures.len(),
        failures.len(),
    );

    assert!(
        failures.is_empty(),
        "{} of {total} decode fixture test(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
