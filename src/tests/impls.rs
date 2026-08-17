// use crate::{Deserializer, SIMDJSON_PADDING, Stage1Parse};

// fn test_find_structural_bits<S: Stage1Parse>(input_str: &str, expected: &[u32]) {
//     let mut input = input_str.as_bytes().to_vec();
//     input.append(&mut vec![0; SIMDJSON_PADDING]);
//     let mut res = Vec::new();

//     unsafe {
//         Deserializer::_find_structural_bits::<S>(input.as_slice(), &mut res)
//             .expect("failed to find structural bits");
//     };
//     println!("{input_str}");
//     assert_eq!(res, expected);
// }

// fn find_structural_bits_test_cases<S: Stage1Parse>() {
//     test_find_structural_bits::<S>("", &[0]);
//     test_find_structural_bits::<S>("1", &[0]);
//     test_find_structural_bits::<S>("[1]", &[0, 1, 2, 3]);
//     test_find_structural_bits::<S>("[1, 2]", &[0, 1, 2, 4, 5, 6]);
//     test_find_structural_bits::<S>(
//         r#"{
//                 "snot": "badger",
//                 "numbers": [1,2,3,4,5,6,7,8,9,10,11,12, 13, {"not a number": "but a flat object"}],
//                 "a float because we can": 0.123456789e11,
//                 "and a string that we can put in here": "oh my stringy string, you are long so that we exceed the twohundredsixtyfive bits of a simd register"
//     }"#,
//         &[
//             0, 18, 24, 26, 34, 52, 61, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
//             78, 79, 80, 81, 82, 84, 85, 87, 88, 90, 92, 94, 96, 97, 111, 113, 132, 133, 134, 152,
//             176, 178, 192, 210, 248, 250, 357, 358,
//         ],
//     );

//     test_find_structural_bits::<S>(
//         r#" { "hell\"o": 1 , "b": [ 1, 2, 3 ] }"#,
//         &[1, 3, 12, 14, 16, 18, 21, 23, 25, 26, 28, 29, 31, 33, 35, 36],
//     );
// }
