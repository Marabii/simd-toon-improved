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
        b"null" => return BasicTypes::Null,
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
        let mut exp_seen = false;
        let mut expects_sign = false; // Tracks if a '+' or '-' can start the next chunk

        while len >= 32 {
            let chunk = _mm256_loadu_si256(ptr.cast::<__m256i>());
            let (valid_mask, dot_mask, exp_mask, sign_mask) = parse_number_chunk(chunk);

            // Check for illegal characters
            if valid_mask != 0xFFFF_FFFF {
                return BasicTypes::String;
            }

            // Validate Exponents (Max 1)
            let exp_count = exp_mask.count_ones();
            if exp_count > 1 || (exp_count == 1 && exp_seen) {
                return BasicTypes::String;
            }

            if exp_count == 1 {
                exp_seen = true;
            }

            // Validate Dots (Max 1, cannot appear after an exponent)
            let dots = dot_mask.count_ones();
            if dots > 1 {
                return BasicTypes::String;
            }
            if dots == 1 {
                dot_count += 1;
                if dot_count > 1 {
                    return BasicTypes::String;
                }

                // If an exponent was seen previously, or in this chunk BEFORE the dot
                if (exp_seen && exp_count == 0) || (exp_count == 1 && dot_mask > exp_mask) {
                    return BasicTypes::String;
                }
            }

            // Validate Signs (Must be immediately after an 'e' or 'E')
            let mut allowed_signs = exp_mask << 1;
            if expects_sign {
                allowed_signs |= 1;
            } // Allow sign at bit 0 if previous chunk ended in 'e'

            if (sign_mask & !allowed_signs) != 0 {
                return BasicTypes::String; // Sign found in an illegal position
            }

            // Did this chunk end with an exponent? (meaning the next chunk's first byte can be a sign)
            expects_sign = (exp_mask >> 31) != 0;

            ptr = ptr.add(32);
            len -= 32;
        }

        // Process the remainder (tail < 32 bytes)
        if len > 0 {
            let tail = core::slice::from_raw_parts(ptr, len);
            for &b in tail {
                match b {
                    b'0'..=b'9' => {
                        // Digits are always valid, and they clear the expects_sign flag
                        expects_sign = false;
                    }
                    b'.' => {
                        // A dot cannot appear multiple times, nor can it appear after an exponent
                        if exp_seen || dot_count > 0 {
                            return BasicTypes::String;
                        }
                        dot_count += 1;
                        expects_sign = false;
                    }
                    b'e' | b'E' => {
                        // Only one exponent is allowed
                        if exp_seen {
                            return BasicTypes::String;
                        }
                        exp_seen = true;
                        expects_sign = true; // The NEXT byte is allowed to be a sign
                    }
                    b'+' | b'-' => {
                        // Signs are ONLY valid immediately following an 'e' or 'E'
                        if !expects_sign {
                            return BasicTypes::String;
                        }
                        expects_sign = false;
                    }
                    _ => {
                        // Any other character invalidates the number
                        return BasicTypes::String;
                    }
                }
            }
        }
    }

    BasicTypes::Number
}

// Can be a valid number but can also be something like 0......8
// We need to keep track of dot count.
#[target_feature(enable = "avx2")]
unsafe fn parse_number_chunk(v: __m256i) -> (u32, u32, u32, u32) {
    let v_min = _mm256_set1_epi8(b'0' as i8);
    let v_max = _mm256_set1_epi8(b'9' as i8);

    // Digits: !(v < '0' || v > '9')
    let lt_min = _mm256_cmpgt_epi8(v_min, v);
    let gt_max = _mm256_cmpgt_epi8(v, v_max);
    let is_digit = _mm256_xor_si256(_mm256_or_si256(lt_min, gt_max), _mm256_set1_epi8(-1));

    // Specific characters
    let is_dot = _mm256_cmpeq_epi8(v, _mm256_set1_epi8(b'.' as i8));
    let is_e = _mm256_cmpeq_epi8(v, _mm256_set1_epi8(b'e' as i8));
    let is_e_cap = _mm256_cmpeq_epi8(v, _mm256_set1_epi8(b'E' as i8));
    let is_exp = _mm256_or_si256(is_e, is_e_cap);

    let is_plus = _mm256_cmpeq_epi8(v, _mm256_set1_epi8(b'+' as i8));
    let is_minus = _mm256_cmpeq_epi8(v, _mm256_set1_epi8(b'-' as i8));
    let is_sign = _mm256_or_si256(is_plus, is_minus);

    // Combine all valid characters
    let valid_chars = _mm256_or_si256(
        _mm256_or_si256(is_digit, is_dot),
        _mm256_or_si256(is_exp, is_sign),
    );

    (
        _mm256_movemask_epi8(valid_chars) as u32,
        _mm256_movemask_epi8(is_dot) as u32,
        _mm256_movemask_epi8(is_exp) as u32,
        _mm256_movemask_epi8(is_sign) as u32,
    )
}
