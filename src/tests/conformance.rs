//! Dynamic conformance test runner.
//!
//! Reads every `*.json` fixture file under `src/tests/fixtures/` (language-agnostic
//! TOON test fixtures following `src/tests/fixtures.schema.json`), decodes each
//! test case's TOON `input` with this crate, and compares the result against the
//! fixture's `expected` JSON value. Only `category: "decode"` fixtures are run;
//! this crate does not implement encoding yet.

#![allow(clippy::unwrap_used)]

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
}

/// Decodes `tc.input` (expected to be a TOON source string) and checks it
/// against `tc.expected`/`tc.should_error`. Returns `Err(reason)` on mismatch,
/// decode failure, or panic inside the decoder.
fn run_decode_test(tc: &TestCase) -> Result<(), String> {
    let Some(toon_input) = tc.input.as_str() else {
        return Err("fixture `input` is not a string (decode tests require TOON source text)".to_string());
    };

    let mut buf = toon_input.as_bytes().to_vec();
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        crate::from_slice::<serde_json::Value>(&mut buf)
    }));

    match outcome {
        Ok(Ok(actual)) if tc.should_error => {
            Err(format!("expected an error but decoded successfully: {actual}"))
        }
        Ok(Ok(actual)) if actual == tc.expected => Ok(()),
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
            if let Err(reason) = run_decode_test(tc) {
                failures.push(format!("[{file_name}] {}: {reason}", tc.name));
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
