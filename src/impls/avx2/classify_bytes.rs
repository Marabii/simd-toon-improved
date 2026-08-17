use std::arch::x86_64::{
    __m256i, _mm256_cmpeq_epi8, _mm256_cmpgt_epi8, _mm256_loadu_si256, _mm256_movemask_epi8,
    _mm256_or_si256, _mm256_set1_epi8, _mm256_xor_si256,
};

use crate::BasicTypes;

#[target_feature(enable = "avx2")]
#[cfg_attr(not(feature = "no-inline"), inline)]
pub(crate) fn classify_bytes(input: &[u8]) -> BasicTypes {
    match input {
        b"true" => return BasicTypes::Boolean(true),
        b"false" => return BasicTypes::Boolean(false),
        _ => {}
    }

    unsafe {
        let mut ptr = input.as_ptr();
        let mut len = input.len();

        // Handle negative numbers: skip the first byte if it's a '-'
        if *ptr == b'-' {
            ptr = ptr.add(1);
            len -= 1;

            // A single "-" or "-." is a string
            if len == 0 || (len == 1 && *ptr == b'.') {
                return BasicTypes::String;
            }
        } else if len == 1 && *ptr == b'.' {
            // A single "." is a string
            return BasicTypes::String;
        }

        let mut dot_count = 0;
        let mut can_be_number = true;

        while len >= 32 {
            let chunk = _mm256_loadu_si256(ptr.cast::<__m256i>());

            let (can_be_number_new, dot_count_new) = possibly_a_number(chunk);

            can_be_number &= can_be_number_new;

            if !can_be_number {
                return BasicTypes::String;
            }

            dot_count += dot_count_new;
            if dot_count > 1 {
                return BasicTypes::String;
            }

            ptr = ptr.add(32);
            len -= 32;
        }

        // Process the remainder
        if len > 0 {
            let tail = core::slice::from_raw_parts(ptr, len);
            for &b in tail {
                if b == b'.' {
                    dot_count += 1;
                    if dot_count > 1 {
                        return BasicTypes::String;
                    }
                } else if !b.is_ascii_digit() {
                    return BasicTypes::String;
                }
            }
        }
    }

    BasicTypes::Number
}

// Can be a valid number but can also be something like 0......8
// We need to keep track of dot count.
#[target_feature(enable = "avx2")]
unsafe fn possibly_a_number(v: __m256i) -> (bool, u32) {
    let v_min = _mm256_set1_epi8(b'0'.cast_signed());
    let v_max = _mm256_set1_epi8(b'9'.cast_signed());
    let v_dot = _mm256_set1_epi8(b'.'.cast_signed());

    // min > v  <=>  v < min
    let lt_min = _mm256_cmpgt_epi8(v_min, v);
    // v > max
    let gt_max = _mm256_cmpgt_epi8(v, v_max);

    // Invert: !(v < min || v > max)
    let out_of_bounds = _mm256_or_si256(lt_min, gt_max);
    let is_digit = _mm256_xor_si256(out_of_bounds, _mm256_set1_epi8(-1i8));
    let is_dot = _mm256_cmpeq_epi8(v, v_dot);

    let valid_mask = _mm256_or_si256(is_digit, is_dot);
    let dot_mask = _mm256_movemask_epi8(is_dot).cast_unsigned();

    // Extract the most significant bit of each of the 32 bytes
    let dot_count = dot_mask.count_ones();
    let can_be_number =
        (_mm256_movemask_epi8(valid_mask).cast_unsigned() == 0xFFFF_FFFF) && dot_count <= 1;

    // Returns true if all 32 bytes matched
    (can_be_number, dot_count)
}
