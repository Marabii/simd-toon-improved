#![allow(clippy::ignored_unit_patterns)]

#[cfg(feature = "serde_impl")]
mod conformance;

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
fn playground() {
    let mut d = String::from("key: foo [2]: bar");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
    println!("{:?}", simd.tape);
}

#[test]
fn test_tape_object_simple() {
    let mut d = String::from("a:\n  b:\n    c: Hamza\n  d: Dadda");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("");
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
fn test_null_value() {
    let mut d = String::from("v_str: \"\0[\"");
    let d = unsafe { d.as_bytes_mut() };
    let simd = Deserializer::from_slice(d).expect("failed to parse");
    assert_eq!(
        simd.tape,
        [
            Node::Object { len: 1, count: 2 },
            Node::String("v_str"),
            Node::String("\0[")
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
    let simd = Deserializer::from_slice(d).expect("failed to parse");
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
    let mut d = String::from("numbers[3]: 1,2,3");
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
