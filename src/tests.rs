#![allow(clippy::ignored_unit_patterns)]

#[cfg(feature = "serde_impl")]
mod conformance;

use crate::DecodeOptions;
#[cfg(not(target_arch = "wasm32"))]
use crate::{Deserializer, tape::Node};
#[cfg(not(target_arch = "wasm32"))]
use value_trait::prelude::*;

#[test]
fn test_send_sync() {
    struct TestStruct<T: Sync + Send>(T);
    #[allow(let_underscore_drop)] // test
    let _: TestStruct<_> = TestStruct(super::AlignedBuf::with_capacity(0));
}

#[test]
fn test_root_keyless_block_array() {
    let mut d = String::from("[2]:\n  - a\n  - b");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Array { len: 2, count: 2 },
            Node::String("a"),
            Node::String("b"),
        ]
    );
}

#[test]
fn test_root_keyless_inline_array() {
    let mut d = String::from("[2]: a, b");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Array { len: 2, count: 2 },
            Node::String("a"),
            Node::String("b"),
        ]
    );
}

#[test]
fn test_empty_root_array_with_key() {
    let mut d = String::from("items[0]:");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("items"),
            Node::Array { len: 0, count: 0 },
        ]
    );
}

#[test]
fn test_empty_root_array_without_key() {
    let mut d = String::from("[0]:\n       #   lasndflk;ansdf;lnasdf");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(simd.tape, [Node::Array { len: 0, count: 0 },]);
}

#[test]
fn playground() {
    let mut d = String::from("value: -0e1");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    println!("{:?}", simd.tape)
}

#[test]
fn test_tape_object_simple() {
    let mut d = String::from("a:\n  b:\n    c: Hamza\n  d: Dadda");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::lenient();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    println!("{:?}", simd.tape);
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 8 },
            Node::String("a"),
            Node::Object { len: 2, count: 6 },
            Node::String("b"),
            Node::Object { len: 1, count: 2 },
            Node::String("c"),
            Node::String("Hamza"),
            Node::String("d"),
            Node::String("Dadda")
        ]
    );
}

#[test]
fn test_nested_block_array_items() {
    let mut d = String::from(
        r"geodata[3]:
  - [2]: 7.69,47.54
  - [2]:
    - 8.71
    - 47.69
  - [2]{lat,lng}:
    9.12,48.11
    9.15,48.15
",
    );
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 19 },
            Node::String("geodata"),
            Node::Array { len: 3, count: 17 },
            Node::Array { len: 2, count: 2 },
            Node::Static(StaticNode::F64(7.69)),
            Node::Static(StaticNode::F64(47.54)),
            Node::Array { len: 2, count: 2 },
            Node::Static(StaticNode::F64(8.71)),
            Node::Static(StaticNode::F64(47.69)),
            Node::Array { len: 2, count: 10 },
            Node::Object { len: 2, count: 4 },
            Node::String("lat"),
            Node::Static(StaticNode::F64(9.12)),
            Node::String("lng"),
            Node::Static(StaticNode::F64(48.11)),
            Node::Object { len: 2, count: 4 },
            Node::String("lat"),
            Node::Static(StaticNode::F64(9.15)),
            Node::String("lng"),
            Node::Static(StaticNode::F64(48.15)),
        ]
    );
}

#[test]
fn test_keyed_tabular_objects() {
    let mut d = String::from("users[2:]{age,city}:\n  alice: 30,Berlin\n  bob: 25,Oslo\n");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    println!("{:?}", simd.tape);
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 14 },
            Node::String("users"),
            Node::Object { len: 2, count: 12 },
            Node::String("alice"),
            Node::Object { len: 2, count: 4 },
            Node::String("age"),
            Node::Static(StaticNode::U64(30)),
            Node::String("city"),
            Node::String("Berlin"),
            Node::String("bob"),
            Node::Object { len: 2, count: 4 },
            Node::String("age"),
            Node::Static(StaticNode::U64(25)),
            Node::String("city"),
            Node::String("Oslo"),
        ]
    );
}

#[test]
fn test_null_in_array() {
    let mut d = String::from("arr[4]: 1,null,2,null");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 6 },
            Node::String("arr"),
            Node::Array { len: 4, count: 4 },
            Node::Static(StaticNode::U64(1)),
            Node::Static(StaticNode::Null),
            Node::Static(StaticNode::U64(2)),
            Node::Static(StaticNode::Null),
        ]
    );
}

#[test]
fn test_deeply_nested_rows_elements() {
    let mut d = String::from(
        r"rows[1]:
  - elements[1]:
      - distance:
          text: 1 m",
    );
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::lenient();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 10 },
            Node::String("rows"),
            Node::Array { len: 1, count: 8 },
            Node::Object { len: 1, count: 7 },
            Node::String("elements"),
            Node::Array { len: 1, count: 5 },
            Node::Object { len: 1, count: 4 },
            Node::String("distance"),
            Node::Object { len: 1, count: 2 },
            Node::String("text"),
            Node::String("1 m"),
        ]
    );
}

#[test]
fn test_multiline_array_with_indented_strings() {
    let mut d = String::from("users[3]:\n  - Eren Yeager\n  - Mikasa Akarman\n  - Armin Arlert");
    let d = unsafe { d.as_bytes_mut() };

    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 5 },
            Node::String("users"),
            Node::Array { len: 3, count: 3 },
            Node::String("Eren Yeager"),
            Node::String("Mikasa Akarman"),
            Node::String("Armin Arlert"),
        ]
    );
}

#[test]
fn test_multi_word_strings_within_arrays() {
    let mut d = String::from("names[2]: hamza dadda, Arima Kousei");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 4 },
            Node::String("names"),
            Node::Array { len: 2, count: 2 },
            Node::String("hamza dadda"),
            Node::String("Arima Kousei"),
        ]
    );
}

#[test]
fn test_multi_word_strings_within_objects() {
    let mut d = String::from("fullName: Hamza DADDA\nage: 21");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 4 },
            Node::String("fullName"),
            Node::String("Hamza DADDA"),
            Node::String("age"),
            Node::Static(StaticNode::U64(21)),
        ]
    );
}
#[test]
fn test_tape_inline_string_array() {
    let mut d = String::from("tags[3]: rust,parser,simd");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 5 },
            Node::String("tags"),
            Node::Array { len: 3, count: 3 },
            Node::String("rust"),
            Node::String("parser"),
            Node::String("simd"),
        ]
    );
}

#[test]
fn test_tape_inline_number_array() {
    let mut d = String::from("numbers[3]: 1,2,3\n");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    println!("{:?}", simd.tape);
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 5 },
            Node::String("numbers"),
            Node::Array { len: 3, count: 3 },
            Node::Static(StaticNode::U64(1)),
            Node::Static(StaticNode::U64(2)),
            Node::Static(StaticNode::U64(3)),
        ]
    );
}

#[test]
fn test_tape_inline_bool_array() {
    let mut d = String::from("flags[3]: true,false,true");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 5 },
            Node::String("flags"),
            Node::Array { len: 3, count: 3 },
            Node::Static(StaticNode::Bool(true)),
            Node::Static(StaticNode::Bool(false)),
            Node::Static(StaticNode::Bool(true)),
        ]
    );
}

#[test]
fn test_tape_empty_array() {
    let mut d = String::from("empty[0]:\nother: val");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 4 },
            Node::String("empty"),
            Node::Array { len: 0, count: 0 },
            Node::String("other"),
            Node::String("val"),
        ]
    );
}

#[test]
fn test_tape_array_with_sibling_key() {
    // Array followed by another key-value pair
    let mut d = String::from("tags[3]: rust,parser,simd\nver: 1");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 7 },
            Node::String("tags"),
            Node::Array { len: 3, count: 3 },
            Node::String("rust"),
            Node::String("parser"),
            Node::String("simd"),
            Node::String("ver"),
            Node::Static(StaticNode::U64(1)),
        ]
    );
}

#[test]
fn test_tape_complex_object_array() {
    // The input string with your specific formatting
    let mut d = String::from("items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5\n");
    let d = unsafe { d.as_bytes_mut() };

    let simd = Deserializer::from_slice(d).expect("failed to parse");

    // Comparing against your provided Node tape
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 16 },
            Node::String("items"),
            Node::Array { len: 2, count: 14 },
            // First item in the array
            Node::Object { len: 3, count: 6 },
            Node::String("sku"),
            Node::String("A1"),
            Node::String("qty"),
            Node::Static(StaticNode::U64(2)),
            Node::String("price"),
            Node::Static(StaticNode::F64(9.99)),
            // Second item in the array
            Node::Object { len: 3, count: 6 },
            Node::String("sku"),
            Node::String("B2"),
            Node::String("qty"),
            Node::Static(StaticNode::U64(1)),
            Node::String("price"),
            Node::Static(StaticNode::F64(14.5)),
        ]
    );
}

#[test]
fn test_tape_tabular_string_array() {
    // users[2]{id,name}:\n  1,Alice\n  2,Bob
    let mut d = String::from("users[2]{id,name}:\n  1,Alice\n  2,Bob");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    println!("{:?}", simd.tape);
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 12 },
            Node::String("users"),
            Node::Array { len: 2, count: 10 },
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(1)),
            Node::String("name"),
            Node::String("Alice"),
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(2)),
            Node::String("name"),
            Node::String("Bob"),
        ]
    );
}

#[test]
fn test_tape_tabular_mixed_types() {
    // items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5
    let mut d = String::from("items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    println!("{:?}", simd.tape);
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 16 },
            Node::String("items"),
            Node::Array { len: 2, count: 14 },
            Node::Object { len: 3, count: 6 },
            Node::String("sku"),
            Node::String("A1"),
            Node::String("qty"),
            Node::Static(StaticNode::U64(2)),
            Node::String("price"),
            Node::Static(StaticNode::F64(9.99)),
            Node::Object { len: 3, count: 6 },
            Node::String("sku"),
            Node::String("B2"),
            Node::String("qty"),
            Node::Static(StaticNode::U64(1)),
            Node::String("price"),
            Node::Static(StaticNode::F64(14.5)),
        ]
    );
}

#[test]
fn test_tape_tabular_with_sibling_key() {
    // Tabular array followed by another key-value pair at the same level
    let mut d = String::from("users[2]{id,name}:\n  1,Alice\n  2,Bob\nver: 2");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    println!("{:?}", simd.tape);
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 14 },
            Node::String("users"),
            Node::Array { len: 2, count: 10 },
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(1)),
            Node::String("name"),
            Node::String("Alice"),
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(2)),
            Node::String("name"),
            Node::String("Bob"),
            Node::String("ver"),
            Node::Static(StaticNode::U64(2)),
        ]
    );
}

#[test]
fn test_tape_block_array_mixed_items() {
    let mut d = String::from("items[3]:\n  - 1\n  - a: 1\n  - text");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 7 },
            Node::String("items"),
            Node::Array { len: 3, count: 5 },
            Node::Static(StaticNode::U64(1)),
            Node::Object { len: 1, count: 2 },
            Node::String("a"),
            Node::Static(StaticNode::U64(1)),
            Node::String("text"),
        ]
    );
}

#[test]
fn test_tape_block_array_object_items() {
    let mut d = String::from(
        "items[2]:\n  - id: 1\n    name: First\n  - id: 2\n    name: Second\n    extra: true",
    );
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 14 },
            Node::String("items"),
            Node::Array { len: 2, count: 12 },
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(1)),
            Node::String("name"),
            Node::String("First"),
            Node::Object { len: 3, count: 6 },
            Node::String("id"),
            Node::Static(StaticNode::U64(2)),
            Node::String("name"),
            Node::String("Second"),
            Node::String("extra"),
            Node::Static(StaticNode::Bool(true)),
        ]
    );
}

#[test]
fn test_tape_block_array_object_first_tabular_field_with_sibling() {
    let mut d = String::from(
        "items[1]:\n  - users[2]{id,name}:\n      1,Ada\n      2,Bob\n    status: active",
    );
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 17 },
            Node::String("items"),
            Node::Array { len: 1, count: 15 },
            Node::Object { len: 2, count: 14 },
            Node::String("users"),
            Node::Array { len: 2, count: 10 },
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(1)),
            Node::String("name"),
            Node::String("Ada"),
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(2)),
            Node::String("name"),
            Node::String("Bob"),
            Node::String("status"),
            Node::String("active"),
        ]
    );
}

#[test]
fn test_tape_block_array_object_single_tabular_field() {
    let mut d = String::from("items[1]:\n  - users[2]{id,name}:\n      1,Ada\n      2,Bob");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 15 },
            Node::String("items"),
            Node::Array { len: 1, count: 13 },
            Node::Object { len: 1, count: 12 },
            Node::String("users"),
            Node::Array { len: 2, count: 10 },
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(1)),
            Node::String("name"),
            Node::String("Ada"),
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(2)),
            Node::String("name"),
            Node::String("Bob"),
        ]
    );
}

#[test]
fn test_tape_root_block_array_strings() {
    let mut d = String::from("[2]:\n  - something\n  - something else");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Array { len: 2, count: 2 },
            Node::String("something"),
            Node::String("something else"),
        ]
    );
}

#[test]
fn test_tape_root_inline_array_strings() {
    let mut d = String::from("[2]: something, \"something else\"");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Array { len: 2, count: 2 },
            Node::String("something"),
            Node::String("something else"),
        ]
    );
}

#[test]
fn test_tape_root_block_array_object_items() {
    let mut d = String::from("[1]:\n  - id: 0\n    uuid: \"abc\"");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    println!("{:?}", simd.tape);

    assert_eq!(
        simd.tape,
        [
            Node::Array { len: 1, count: 5 },
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(0)),
            Node::String("uuid"),
            Node::String("abc"),
        ]
    );
}

#[test]
fn test_tape_block_array_of_inline_arrays() {
    // pairs[2]:
    //   - [2]: 1,2
    //   - [2]: 3,4
    let mut d = String::from("pairs[2]:\n  - [2]: 1,2\n  - [2]: 3,4");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 8 },
            Node::String("pairs"),
            Node::Array { len: 2, count: 6 },
            Node::Array { len: 2, count: 2 },
            Node::Static(StaticNode::U64(1)),
            Node::Static(StaticNode::U64(2)),
            Node::Array { len: 2, count: 2 },
            Node::Static(StaticNode::U64(3)),
            Node::Static(StaticNode::U64(4)),
        ]
    );
}

#[test]
fn test_tape_root_block_array_object_nested_items() {
    let mut d =
        String::from("[1]:\n  - id: 0\n    metadata:\n      timestamp: \"2025-01-01T00:00:00Z\"");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Array { len: 1, count: 7 },
            Node::Object { len: 2, count: 6 },
            Node::String("id"),
            Node::Static(StaticNode::U64(0)),
            Node::String("metadata"),
            Node::Object { len: 1, count: 2 },
            Node::String("timestamp"),
            Node::String("2025-01-01T00:00:00Z"),
        ]
    );
}

#[test]
fn test_compact_array_multiple_object_items_with_nested_object_fields() {
    let mut d = String::from(
        r#"rows[1]:
  - elements[2]:
      - distance:
          text: "4,490 km"
          value: 4489862
        duration:
          text: 1 day 16 hours
          value: 145589
        status: OK
      - distance:
          text: "1,270 km"
          value: 1270445
        duration:
          text: 12 hours 10 mins
          value: 43773
        status: OK"#,
    );
    let d = unsafe { d.as_bytes_mut() };
    let _ = Deserializer::from_slice(d).expect("failed to parse");
}

#[test]
fn test_empty_object_before_sibling_key() {
    let mut d = String::from("morphTargets:\nnormals[1]: 0");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");

    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 5 },
            Node::String("morphTargets"),
            Node::Object { len: 0, count: 0 },
            Node::String("normals"),
            Node::Array { len: 1, count: 1 },
            Node::Static(StaticNode::U64(0)),
        ]
    );
}

#[test]
fn test_parses_objects_with_primitive_values() {
    let mut d = String::from("id: 123\nname: Ada\nactive: true");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 3, count: 6 },
            Node::String("id"),
            Node::Static(StaticNode::I64(123)),
            Node::String("name"),
            Node::String("Ada"),
            Node::String("active"),
            Node::Static(StaticNode::Bool(true))
        ]
    );
}

#[test]
fn test_parses_null_values_in_objects() {
    let mut d = String::from("id: 123\nvalue: null");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::I64(123)),
            Node::String("value"),
            Node::Static(StaticNode::Null)
        ]
    );
}

#[test]
fn test_parses_empty_nested_object_header() {
    let mut d = String::from("user:");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("user"),
            Node::Object { len: 0, count: 0 }
        ]
    );
}

#[test]
fn test_bare_key_with_no_children_decodes_as_empty_object() {
    let mut d = String::from("matches:");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("matches"),
            Node::Object { len: 0, count: 0 }
        ]
    );
}

#[test]
fn test_applies_last_write_wins_for_duplicate_sibling_keys_in_non_strict_mode() {
    let mut d = String::from("name: Ada\nname: Bob");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::lenient();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("name"),
            Node::String("Bob")
        ]
    );
}

#[test]
fn test_parses_quoted_object_value_with_colon() {
    let mut d = String::from("note: \"a:b\"");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("note"),
            Node::String("a:b")
        ]
    );
}

#[test]
fn test_parses_quoted_object_value_with_newline_escape() {
    let mut d = String::from("text: \"line1\\nline2\"");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("text"),
            Node::String("line1\nline2")
        ]
    );
}

#[test]
fn test_parses_quoted_object_value_with_escaped_quotes() {
    let mut d = String::from("text: \"say \\\"hello\\\"\"");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("text"),
            Node::String("say \"hello\"")
        ]
    );
}

#[test]
fn test_parses_quoted_string_value_that_looks_like_true() {
    let mut d = String::from("v: \"true\"");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("v"),
            Node::String("true")
        ]
    );
}

#[test]
fn test_parses_quoted_string_value_that_looks_like_negative_decimal() {
    let mut d = String::from("v: \"-7.5\"");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("v"),
            Node::String("-7.5")
        ]
    );
}

#[test]
fn test_parses_unquoted_value_shaped_like_an_inline_array_header_after_the_key() {
    let mut d = String::from("key: foo [2]: bar");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("key"),
            Node::String("foo [2]: bar")
        ]
    );
}

#[test]
fn test_decodes_uXXXX_in_quoted_key() {
    let mut d = String::from("\"a\\u0004b\": 1");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("a\u{0004}b"),
            Node::Static(StaticNode::I64(1))
        ]
    );
}

#[test]
fn test_treats_extra_brackets_after_valid_array_segment_as_literal_key_non_strict() {
    let mut d = String::from("foo[1][bar]: 10");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::lenient();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("foo[1][bar]"),
            Node::Static(StaticNode::I64(10))
        ]
    );
}

#[test]
fn test_parses_deeply_nested_objects_with_indentation() {
    let mut d = String::from("a:\n  b:\n    c: deep");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 6 },
            Node::String("a"),
            Node::Object { len: 1, count: 4 },
            Node::String("b"),
            Node::Object { len: 1, count: 2 },
            Node::String("c"),
            Node::String("deep")
        ]
    );
}

#[test]
fn test_applies_lww_for_nested_duplicate_sibling_keys_in_non_strict_mode() {
    let mut d = String::from("outer:\n  name: Ada\n  name: Bob");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::lenient();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 4 },
            Node::String("outer"),
            Node::Object { len: 1, count: 2 },
            Node::String("name"),
            Node::String("Bob")
        ]
    );
}

#[test]
fn test_applies_lww_for_duplicate_keys_within_a_list_item_object_in_non_strict_mode() {
    let mut d = String::from("items[1]:\n  - id: 1\n    id: 2");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::lenient();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 4 },
            Node::String("items"),
            Node::Array { len: 1, count: 3 },
            Node::Object { len: 1, count: 2 },
            Node::String("id"),
            Node::Static(StaticNode::I64(2))
        ]
    );
}

#[test]
fn test_materializes_proto_as_an_ordinary_own_key() {
    let mut d = String::from("__proto__: polluted");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("__proto__"),
            Node::String("polluted")
        ]
    );
}

#[test]
fn test_materializes_proto_tabular_field_name_as_ordinary_own_keys() {
    let mut d = String::from("rows[2]{__proto__,x}:\n  a,1\n  b,2");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 12 },
            Node::String("rows"),
            Node::Array { len: 2, count: 10 },
            Node::Object { len: 2, count: 4 },
            Node::String("__proto__"),
            Node::String("a"),
            Node::String("x"),
            Node::Static(StaticNode::U64(1)),
            Node::Object { len: 2, count: 4 },
            Node::String("__proto__"),
            Node::String("b"),
            Node::String("x"),
            Node::Static(StaticNode::U64(2))
        ]
    );
}

#[test]
fn test_accepts_a_header_key_outside_the_encoder_unquoted_key_pattern() {
    let mut d = String::from("foo-bar[2]: 1,2");
    let d = unsafe { d.as_bytes_mut() };
    let decode_options = DecodeOptions::new();
    let simd = Deserializer::from_slice_with_options(d, decode_options).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 4 },
            Node::String("foo-bar"),
            Node::Array { len: 2, count: 2 },
            Node::Static(StaticNode::U64(1)),
            Node::Static(StaticNode::U64(2))
        ]
    );
}

#[test]
fn test_comment_line_is_absorbed_by_the_line_above() {
    let mut d = String::from("name: Hamza\n  # some comment\nage: 21");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 4 },
            Node::String("name"),
            Node::String("Hamza"),
            Node::String("age"),
            Node::Static(StaticNode::U64(21)),
        ]
    );
}

#[test]
fn test_consecutive_comment_lines_collapse_into_one_run_of_spaces() {
    let mut d = String::from("a: 1\n# c1\n  # c2\n# c3\nb: 2");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 2, count: 4 },
            Node::String("a"),
            Node::Static(StaticNode::U64(1)),
            Node::String("b"),
            Node::Static(StaticNode::U64(2)),
        ]
    );
}

#[test]
fn test_comment_does_not_count_as_a_list_item() {
    let mut d = String::from("items[2]:\n  - a\n  # note\n  - b");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 4 },
            Node::String("items"),
            Node::Array { len: 2, count: 2 },
            Node::String("a"),
            Node::String("b"),
        ]
    );
}

#[test]
fn test_outdented_comment_does_not_close_the_scope_it_sits_in() {
    let mut d = String::from("user:\n  id: 1\n# outdented note\n  name: Ada");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 6 },
            Node::String("user"),
            Node::Object { len: 2, count: 4 },
            Node::String("id"),
            Node::Static(StaticNode::U64(1)),
            Node::String("name"),
            Node::String("Ada"),
        ]
    );
}

#[test]
fn test_hash_that_is_not_a_lines_first_token_stays_data() {
    let mut d = String::from("note: #x");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("note"),
            Node::String("#x"),
        ]
    );
}

#[test]
fn test_quoted_hash_leading_cell_stays_data() {
    let mut d = String::from("items[1]{tag}:\n  \"#a\"");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 5 },
            Node::String("items"),
            Node::Array { len: 1, count: 3 },
            Node::Object { len: 1, count: 2 },
            Node::String("tag"),
            Node::String("#a"),
        ]
    );
}

#[test]
fn test_tab_indented_hash_is_not_a_comment() {
    // Only U+0020 may precede the '#', so this line stays data -- and a tab in
    // indentation is an error in strict mode.
    let mut d = String::from("a: 1\n\t# not a comment");
    let d = unsafe { d.as_bytes_mut() };
    assert!(Deserializer::from_slice(d).is_err());
}

/// A comment, and the indentation in front of one, can outrun the 64-byte block
/// stage 1 classifies at a time; the run has to carry across blocks, and the
/// newline it swallows may already have been emitted as a structural.
#[test]
fn test_comments_spanning_simd_block_boundaries() {
    for indent in 0..80 {
        for body in [0, 1, 63, 64, 65, 200] {
            let src = format!("a: 1\n{}# {}\nb: 2", " ".repeat(indent), "x".repeat(body));
            let mut d = src.clone().into_bytes();
            let simd = Deserializer::from_slice(&mut d)
                .unwrap_or_else(|e| panic!("{src:?} failed to decode: {e}"));
            assert_eq!(
                simd.tape,
                [
                    Node::Object { len: 2, count: 4 },
                    Node::String("a"),
                    Node::Static(StaticNode::U64(1)),
                    Node::String("b"),
                    Node::Static(StaticNode::U64(2)),
                ],
                "unexpected tape for {src:?}"
            );
        }
    }
}
