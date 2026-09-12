# simd-toon &emsp; a SIMD-accelerated TOON parser for Rust

> [TOON](https://github.com/toon-format/spec) (Token-Oriented Object Notation) is a compact,
> indentation-based, human-readable serialization format designed to use far fewer tokens than
> JSON when fed to LLMs. `simd-toon` parses TOON source text and decodes it into an in-memory
> value (or any `serde::Deserialize` type).
> Internally it reuses the battle-tested SIMD stage1/stage2 machinery of
> [`simd-json`](https://github.com/simd-lite/simd-json) (the value model, tape, and Serde
> integration are still JSON-shaped).

## ⚠️ Status: work in progress

`simd-toon` is under active development (started by [Hamza DADDA](mailto:minehamza97@gmail.com)
about a month ago) and is **not** feature-complete yet:

* **Decoding only.** Encoding TOON output is not implemented.
* Against the [official TOON conformance fixture suite](https://github.com/toon-format/spec/tree/main/tests),
  this parser currently passes **319 of 363** fixture cases (~88%) and fails **44**, mostly around
  strict-mode validation edge cases, keyed tabular headers, and a few root-form/whitespace corner
  cases. See `src/tests/conformance.rs` and `src/tests/fixtures/` for the up-to-date pass/fail set.
* The public decode API (`to_borrowed_value`, `to_owned_value`, `DecodeOptions`, `Parser`, the
  Serde integration) is stable enough to experiment with, but behavior around the remaining
  failures is still changing.

Contributions, bug reports, and PRs against the failing fixtures are very welcome.

## Goals

The goal of `simd-toon` is a high-performance, SIMD-accelerated **TOON** decoder for Rust. It is
built by adapting the internals of the Rust [simd-json](https://github.com/simd-lite/simd-json)
project (itself a port of the [simdjson c++ library](https://simdjson.org/)) to TOON's
whitespace/indentation-driven grammar instead of JSON's brace-and-bracket grammar. As such we aim
to provide both compatibility with Serde as well as parsing to a DOM to manipulate data — for
TOON documents.

## Performance

Benchmarked against [`toon-format`](https://crates.io/crates/toon-format), the official Rust TOON
parser (`decode_default`) (values below are median
times from `cargo bench`, both parsers decoding equivalent TOON input):

| Corpus                  | `simd_toon::to_borrowed_value` | `toon_format::decode_default` | Speedup   |
|--------------------------|--------------------------------|--------------------------------|-----------|
| `event_stacktrace_10kb`  | 2.28 µs                        | 30.05 µs                       | ~13.2x    |
| `github_events`          | 42.36 µs                       | 437.28 µs                      | ~10.3x    |
| `log`                    | 1.69 µs                        | 14.05 µs                       | ~8.3x     |
| `twitter`                | 515.76 µs                      | 4.11 ms                        | ~8.0x     |
| `citm_catalog`           | 1.56 ms                        | 5.86 ms                        | ~3.7x     |
| `canada`                 | 6.66 ms                        | 18.17 ms                       | ~2.7x     |

`to_owned_value` and `to_borrowed_value_with_buffers` track closely behind `to_borrowed_value` (see
`benches/` for the full Criterion reports). These numbers will move as the remaining 44 conformance
fixtures get fixed, since some of that work touches hot paths.

This parser currently only works on CPUs supporting AVX2

### Allocator
For best performance, we highly suggest using [snmalloc](https://github.com/microsoft/snmalloc), [mimalloc](https://crates.io/crates/mimalloc) or [jemalloc](https://crates.io/crates/jemalloc)
instead of the system default allocator.

## Safety

`simd-toon` uses **a lot** of unsafe code.

There are a few reasons for this:

* SIMD intrinsics are inherently unsafe. These uses of unsafe are inescapable in a library such as `simd-toon`.
* We work around some performance bottlenecks imposed by safe rust. These are avoidable, but at a performance cost.
  This is a more considered path in `simd-json`.


## Features
Various features can be enabled or disabled to tweak various parts of this library. Any features not mentioned here are
for internal configuration and testing.

### `runtime-detection` (default)

This feature allows selecting the optimal algorithm based on available features during runtime. It has no effect on
non-`x86` platforms. When neither `AVX2` nor `SSE4.2` is supported, it will fall back to a native Rust implementation.

Disabling this feature (with `default-features = false`) **and** setting `RUSTFLAGS="-C target-cpu=native` will result
in better performance but the resulting binary will not be portable across `x86` processors.

### `serde_impl` (default)

Enable [Serde](https://serde.rs) support. This consist of implementing `serde::Serializer` and `serde::Deserializer`,
allowing types that implement `serde::Serialize`/`serde::Deserialize` to be constructed/serialized to 
`BorrowedValue`/`OwnedValue`.
In addition, this provides the same convenience functions that [`serde_json`](https://docs.rs/serde_json/latest/serde_json/) provides.

Disabling this feature (with `default-features = false`) will remove `serde` and `serde_json` from the dependencies.

### `swar-number-parsing` (default)
Enables a parsing method that will parse 8 digits at a time for floats. This is a common pattern but comes at a slight
performance hit if most of the float have less than 8 digits.

### `known-key`

The `known-key` feature changes the hash mechanism for the DOM representation of the underlying JSON object from
`ahash` to `fxhash`. The `ahash` hasher is faster at hashing and provides protection against DOS attacks by forcing
multiple keys into a single hashing bucket. The `fxhash` hasher allows for repeatable hashing results,
which in turn allows memoizing hashes for well known keys and saving time on lookups. In workloads that are heavy on
accessing some well-known keys, this can be a performance advantage.

The `known-key` feature is optional and disabled by default and should be explicitly configured.

### `big-int-as-float`

The `big-int-as-float` feature flag treats very large integers that won't fit into u64 as f64 floats. This prevents
parsing errors if the JSON you are parsing contains very large integers. Keep in mind that f64 loses some precision when
representing very large numbers.

### `128bit`

Add support for parsing and serializing 128-bit integers. This feature is disabled by default because such large numbers
are rare in the wild and adding the support incurs a performance penalty.

### `beef`

**Enabling this feature can break dependencies in your dependency tree that are using `simd-json`.**

Replace [`std::borrow::Cow`](https://doc.rust-lang.org/std/borrow/enum.Cow.html) with
[`beef::lean::Cow`][beef] This feature is disabled by default, because
it is a breaking change in the API. 

### `ordered-float`

By default the representation of `Floats` used in `borrowed::Value ` and `owned::Value` is simply a value of `f64`. 
This however has the normally-not-a-big-deal side effect of _not_ having these `Value` types be `std::cmp::Eq`. This does,
however, introduce some incompatibilities when offering `simd-json` as a quasi-drop-in replacement for `serde-json`.

So, this feature changes the internal representation of `Floats` to be an `f64` _wrapped by [an Eq-compatible adapter](https://docs.rs/ordered-float/latest/ordered_float/)_.

This probably carries with it some small performance trade-offs, hence its enablement by feature rather than by default.

### `portable`

**Currently disabled**

An highly experimental implementation of the algorithm using `std::simd` and up to 512 byte wide registers.


## Usage

simd-toon offers three main entry points for usage. In every example the input bytes are **TOON**
source text.

### Values API

The values API is a set of optimized DOM objects that hold the decoded
document when its shape isn't known ahead of time. `simd-toon`
has two versions of this:

**Borrowed Values**

```rust
use simd_toon;
let mut d = b"some[3]: key,value,2".to_vec();
let v: simd_toon::BorrowedValue = simd_toon::to_borrowed_value(&mut d).unwrap();
```

**Owned Values**

```rust
use simd_toon;
let mut d = b"some[3]: key,value,2".to_vec();
let v: simd_toon::OwnedValue = simd_toon::to_owned_value(&mut d).unwrap();
```

Tabular arrays — TOON's compact row-oriented form for arrays of uniform objects — decode the same
way:

```rust
use simd_toon;
let mut d = b"items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5".to_vec();
let v: simd_toon::OwnedValue = simd_toon::to_owned_value(&mut d).unwrap();
```

### Serde Compatible API

```rust ignore
use simd_toon;
use serde_json::Value;

let mut d = b"some[3]: key,value,2".to_vec();
let v: Value = simd_toon::serde::from_slice(&mut d).unwrap();
```

### Tape API

```rust
use simd_toon;

let mut d = b"the_answer: 42".to_vec();
let tape = simd_toon::to_tape(&mut d).unwrap();
let value = tape.as_value();
// try_get treats value like an object, returns Ok(Some(_)) because the key is found
assert!(value.try_get("the_answer").unwrap().unwrap() == 42);
// returns Ok(None) because the key is not found but value is an object
assert!(value.try_get("does_not_exist").unwrap() == None);
// try_get_idx treats value like an array, returns Err(_) because value is not an array
assert!(value.try_get_idx(0).is_err());
```

### Decode Options

How a document is decoded is a per document setting, not a build flavour, so it
is passed to the parser rather than selected by a feature flag. `DecodeOptions`
carries the decoder side of the spec's options: `strict` validation and
the `indent_size` used to compute nesting depth.

Every entry point has a `*_with_options` twin:

```rust
use simd_toon::{DecodeOptions, OwnedValue};

let options = DecodeOptions::new().with_strict(false);

let mut d = b"name: Ada".to_vec();
let v: OwnedValue = simd_toon::to_owned_value_with_options(&mut d, options).unwrap();
```

To decode many documents with the same settings, and to reuse the parser's
buffers while doing so, use a `Parser`:

```rust
use simd_toon::{DecodeOptions, Parser};

let mut parser = Parser::with_options(DecodeOptions::new().with_indent_size(4).unwrap());

let mut d = b"name: Ada".to_vec();
let v = parser.parse_to_owned_value(&mut d).unwrap();
```

## Other interesting things

* The [TOON specification](https://github.com/toon-format/spec) describes the format this crate
  parses, including the official conformance fixtures this crate is tested against.
* [`toon-format`](https://crates.io/crates/toon-format) is the official Rust TOON
  parser, used above as the performance baseline.

## License

simd-toon is licensed under either of

* [Apache License, Version 2.0, (LICENSE-APACHE)](http://www.apache.org/licenses/LICENSE-2.0)
* [MIT license (LICENSE-MIT)](http://opensource.org/licenses/MIT)

at your option.

It is built on top of the parsing engine from [`simd-json`](https://github.com/simd-lite/simd-json),
which itself ports a lot of code from [simdjson](https://github.com/lemire/simdjson), so the
copyright of both of those projects should be respected.

The [Serde][serde] integration is based on `serde-json` so their copyright should as well be respected.

[serde]: https://serde.rs
[beef]: https://docs.rs/beef/latest/beef/lean/type.Cow.html
