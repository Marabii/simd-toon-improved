#![allow(dead_code)]
use crate::BasicTypes;
use crate::StaticNode;
#[allow(unused_imports)]
use crate::macros::unlikely;
use crate::safer_unchecked::GetSaferUnchecked;
use crate::value::tape::Node;
use crate::{Deserializer, Error, ErrorType, InternalError, Result};

#[derive(Debug)]
enum State {
    // The first 2 states are mandatory for any grammar that uses whitespace for structure.
    /// The sole hub for indentation decisions.
    CheckIndentation,

    /// Close the current scope.
    ScopeEnd,

    /// Parse a key.
    ParseKey,

    /// Parse value.
    ParseValue,

    /// Decide how to parse an array.
    ArraySwitch,

    /// Parse inline array
    /// ```
    /// tags[3]: admin,ops,dev
    /// ```
    /// is equivalent to
    /// ```
    /// {"tags":[ "admin", "ops", "dev" ]}
    /// ```
    ParseInlineArray,

    /// Parse tabular array
    /// ```
    /// items[2]{sku,qty,price}:
    /// A1,2,9.99
    /// B2,1,14.5
    /// ```
    /// is equivalent to
    /// ```
    /// {"items":[ {"sku":"A1","qty":2,"price":9.99}, {"sku":"B2","qty":1,"price":14.5} ]}
    /// ```
    ParseTabularArray,

    /// Parse nested field groups array
    /// ```
    /// orders[2]{id,customer{name,country},total}:
    ///   1,Ada,DK,99
    ///   2,Bob,UK,149
    /// ```
    /// is equivalent to
    /// ```
    /// {"orders":[ {"id":1,"customer":{"name":"Ada","country":"DK"},"total":99}, {"id":2,"customer":{"name":"Bob","country":"UK"},"total":149} ]}
    /// ```
    ParseNestedFieldGroupsArray,

    /// Parse Mixed and Non-Uniform Arrays
    /// ```
    /// items[3]:
    ///   - 1
    ///   - a: 1
    ///   - text
    /// ```
    /// is equivalent to
    /// ```
    /// {"items":[ 1, {"a":1}, "text" ]}
    /// ```
    ParseMixedAndNonUniformArrays,

    /// Parse Objects as List Items
    /// ```
    /// items[2]:
    ///   - id: 1
    ///     name: First
    ///   - id: 2
    ///     name: Second
    ///     extra: true
    /// ```
    /// is equivalent to
    /// ```
    /// {"items":[ {"id":1,"name":"First"}, {"id":2,"name":"Second","extra":true} ]}
    /// ```
    ParseObjectsAsListItems,
}

#[derive(Debug)]
pub(crate) enum StackState {
    Start,
    Object { last_start: usize, cnt: usize },
    Array { last_start: usize, cnt: usize },
}

impl<'de> Deserializer<'de> {
    #[cfg_attr(not(feature = "no-inline"), inline)]
    #[allow(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        unused_unsafe,
        clippy::needless_continue
    )]
    pub(crate) fn build_tape(
        input: &'de mut [u8],
        input2: &[u8],
        buffer: &mut [u8],
        structural_indexes: &[u32],
        stack: &mut Vec<StackState>,
        res: &mut Vec<Node<'de>>,
    ) -> Result<()> {
        res.clear();
        res.reserve(structural_indexes.len());
        stack.clear();
        stack.reserve(structural_indexes.len());
        println!("input: {:?}", std::str::from_utf8(input).expect("sdfsad"));

        println!("{structural_indexes:?}");

        // Safety: Must NOT advance input pointer as part of logic, since we only get the pointer once.
        // Use idx in order to advance through the input.
        let input_ptr = input.as_mut_ptr();
        // Resolve the per-ISA `parse_str` implementation once per document
        // instead of once per string (T6).
        #[cfg(all(
            feature = "runtime-detection",
            any(target_arch = "x86_64", target_arch = "x86"),
        ))]
        let parse_str_fn = Self::parse_str_fn();

        #[cfg(all(
            feature = "runtime-detection",
            any(target_arch = "x86_64", target_arch = "x86"),
        ))]
        let classify_bytes_fn = Self::classify_bytes_fn();

        let res_ptr = res.as_mut_ptr();
        let stack_ptr = stack.as_mut_ptr();

        // Current nesting level of arrays/objects.
        // Example: parsing the equivalent of {"a":[1]} in TOON goes depth 0 -> 1 (object) -> 2 (array).
        let mut depth: usize = 0;

        // Tape slot where the current container (Node::Object / Node::Array) started.
        // Example: if '{' starts at tape index 7, last_start = 7 until the matching '}'.
        let mut last_start: usize = 0;

        // Accumulator for tracking visual indentation shifts (Compact Arrays / Root Arrays)
        let mut delta: isize = 0;

        // Number of entries seen in the current container.
        // Example: for array[3]: 10,20,30 cnt becomes 3.
        let mut cnt: usize = 0;

        // Write cursor into `res` (the tape under construction).
        // Example: after writing three nodes, r_i == 3.
        let mut r_i = 0;

        // Byte offset in the input buffer for the current structural token.
        // Example: in name: Hamza idx can point to 'n' ':' 'H' ('n' and 'H' because they are the first character of the token)
        let mut idx: usize = 0;

        // Structural byte currently being handled (read from input2[idx]).
        // Example: c == b'{' when entering an object, c == b',' between values.
        let mut c: u8 = 0;

        // Cursor into `structural_indexes`.
        // Example: i == 5 means the next update_char!() reads structural_indexes[5].
        let mut i: usize = 0;

        // Current state of the stage-2 state machine.
        // Example: State::ParseKey means the parser currently expects to parse a key.
        let mut state;

        // Accumulator for tracking visual indentation shifts (Compact Arrays / Root Arrays)
        let mut indent_modifier: isize = 0;

        macro_rules! get {
            ($a:expr_2021, $i:expr_2021) => {{ unsafe { $a.get_kinda_unchecked($i) } }};
        }

        macro_rules! s2try {
            ($e:expr_2021) => {
                match $e {
                    ::std::result::Result::Ok(val) => val,
                    ::std::result::Result::Err(err) => {
                        // We need to ensure that rust doesn't
                        // try to free strings that we never
                        // allocated
                        unsafe {
                            res.set_len(r_i);
                        };
                        return ::std::result::Result::Err(err);
                    }
                }
            };
        }

        macro_rules! insert_res {
            ($t:expr_2021) => {
                unsafe {
                    res_ptr.add(r_i).write($t);
                    r_i += 1;
                }
            };
        }
        macro_rules! success {
            () => {
                unsafe {
                    res.set_len(r_i);
                }
                return Ok(());
            };
        }
        macro_rules! update_char {
            () => {
                if i < structural_indexes.len() {
                    idx = *get!(structural_indexes, i) as usize;
                    i += 1;
                    c = *get!(input2, idx);
                } else {
                    fail!(ErrorType::Syntax);
                }
            };
        }

        macro_rules! goto {
            ($state:expr_2021) => {{
                state = $state;
                #[allow(clippy::needless_continue)]
                continue;
            }};
        }

        macro_rules! insert_str {
            ($start:expr, $end:expr) => {
                insert_res!(Node::String(s2try!(Self::parse_str_(
                    input.as_mut_ptr(),
                    input2,
                    buffer,
                    $start,
                    $end
                ))));
            };

            ($end:expr) => {
                insert_res!(Node::String(s2try!(Self::parse_str_(
                    input.as_mut_ptr(),
                    input2,
                    buffer,
                    idx,
                    $end
                ))));
            };
        }

        macro_rules! trim_trailing_spaces {
            ($start:expr, $hard_end:expr) => {{
                let mut end = $hard_end;
                while end > $start {
                    let prev_char = *get!(input2, end - 1);
                    if prev_char == b' ' || prev_char == b'\r' || prev_char == b'\t' {
                        end -= 1;
                    } else {
                        break;
                    }
                }
                end
            }};
        }

        // When the type of value is unknown, use this macro to automatically figure out the type and insert the value into the tape.
        macro_rules! parse_and_insert_value {
            ($start:expr, $end:expr) => {
                let value_bytes = &input2[$start..$end];
                let basic_type = unsafe { classify_bytes_fn(value_bytes) };
                match basic_type {
                    BasicTypes::Number => {
                        let is_negative = *get!(input2, $start) == b'-';
                        insert_res!(Node::Static(s2try!(Self::parse_number(
                            $start,
                            input2,
                            is_negative,
                        ))));
                    }
                    BasicTypes::String => {
                        insert_str!($start, $end);
                    }
                    BasicTypes::Boolean(b) => {
                        insert_res!(Node::Static(StaticNode::Bool(b)));
                    }
                    BasicTypes::Null => {
                        insert_res!(Node::Static(StaticNode::Null));
                    }
                }
            };
        }

        /// Used to parse values like:
        /// ```
        /// name: Hamza DADDA
        /// ```
        /// We don't don't the length of the string "Hamza DADDA"
        /// structural indexes will contain both 'H' and 'D'
        /// We thus keep moving forward until we find the delimiter we're looking for.
        macro_rules! get_value_end {
            ($err:expr, $($expected_delim:expr),+) => {{
                let mut hard_end = input.len();

                loop {
                    if $(c == $expected_delim)||* {
                        hard_end = idx;
                        break;
                    }

                    if unlikely!(c == b'\n') {
                        fail!($err);
                    }

                    if i >= structural_indexes.len() {
                        break;
                    }

                    // Keep searching forward
                    update_char!();
                }

                trim_trailing_spaces!(idx, hard_end)
            }};
        }

        // The continue cases are the most frequently called onces it's
        // worth pulling them out into a macro (aka inlining them)
        // Since we don't have a 'gogo' in rust.
        // macro_rules! array_continue {
        //     () => {{
        //         update_char!();
        //         match c {
        //             b',' => {
        //                 cnt += 1;
        //                 update_char!();
        //                 goto!(MainArraySwitch);
        //             }
        //             b']' => {
        //                 goto!(ScopeEnd);
        //             }
        //             _c => {
        //                 fail!(ErrorType::ExpectedArrayContent);
        //             }
        //         }
        //     }};
        // }

        // macro_rules! object_continue {
        //     () => {{
        //         update_char!();
        //         match c {
        //             b',' => {
        //                 cnt += 1;
        //                 update_char!();
        //                 if c == b'"' {
        //                     insert_str!();
        //                     goto!(ObjectKey);
        //                 }
        //                 fail!(ErrorType::ExpectedObjectKey);
        //             }
        //             b'}' => {
        //                 goto!(ScopeEnd);
        //             }
        //             _ => {
        //                 fail!(ErrorType::ExpectedObjectContent);
        //             }
        //         }
        //     }};
        // }

        macro_rules! fail {
            () => {
                // We need to ensure that rust doesn't
                // try to free strings that we never
                // allocated
                unsafe {
                    res.set_len(r_i);
                };
                return Err(Error::new_c(
                    idx,
                    c as char,
                    ErrorType::InternalError(InternalError::TapeError),
                ));
            };
            ($t:expr_2021) => {
                // We need to ensure that rust doesn't
                // try to free strings that we never
                // allocated
                unsafe {
                    res.set_len(r_i);
                };
                return Err(Error::new_c(idx, c as char, $t));
            };
        }

        // State start:
        unsafe { stack_ptr.add(depth).write(StackState::Start) };
        last_start = r_i;
        state = State::ParseKey;
        depth += 1;
        insert_res!(Node::Object { len: 0, count: 0 });
        cnt = 0;

        update_char!();
        loop {
            match state {
                State::ParseKey => {
                    let key_start = idx;
                    update_char!();

                    if c == b':' {
                        cnt += 1;
                        let key_end = trim_trailing_spaces!(key_start, idx);
                        insert_str!(key_start, key_end);
                        update_char!();
                        goto!(State::ParseValue);
                    }
                }

                State::ParseValue => {
                    let value_start = idx;
                    let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                    parse_and_insert_value!(value_start, value_end);

                    if i >= structural_indexes.len() {
                        goto!(State::ScopeEnd);
                    }

                    goto!(State::CheckIndentation)
                }

                State::ScopeEnd => {
                    if unlikely!(depth == 0) {
                        fail!(ErrorType::Syntax);
                    }
                    depth -= 1;

                    unsafe {
                        // Backfill the tape:
                        match *res_ptr.add(last_start) {
                            Node::Object {
                                ref mut len,
                                count: ref mut end,
                            } => {
                                *len = cnt;
                                *end = r_i - last_start - 1;
                            }
                            _ => unreachable!("scope end expects an object"),
                        }

                        // Update the stack state:
                        match *stack_ptr.add(depth) {
                            StackState::Object {
                                last_start: l,
                                cnt: c,
                            } => {
                                last_start = l;
                                cnt = c;

                                if i >= structural_indexes.len() {
                                    goto!(State::ScopeEnd);
                                }

                                goto!(State::CheckIndentation);
                            }

                            StackState::Start => {
                                // Skip any trailing `\n` structurals (EOF terminators).
                                while i < structural_indexes.len()
                                    && *get!(input2, *get!(structural_indexes, i) as usize) == b'\n'
                                {
                                    i += 1;
                                }
                                if i == structural_indexes.len() {
                                    success!();
                                }
                                fail!();
                            }

                            _ => unreachable!("Not yet implemented"),
                        }
                    }
                }

                State::CheckIndentation => {
                    if unlikely!(depth == 0) {
                        fail!(ErrorType::Syntax);
                    }

                    if i >= structural_indexes.len() {
                        goto!(State::ScopeEnd);
                    }

                    let old_idx = idx;

                    if unlikely!(*get!(input2, old_idx) != b'\n') {
                        fail!(ErrorType::NoStructure);
                    }

                    update_char!();

                    if i >= structural_indexes.len() {
                        goto!(State::ScopeEnd);
                    }

                    let new_idx = idx;

                    // Prevents double newline characters
                    if unlikely!(*get!(input2, new_idx) == b'\n') {
                        fail!(ErrorType::Syntax);
                    }

                    let actual_ws = new_idx - old_idx - 1;

                    let sibling_ws = (((depth - 1) * 2) as isize + indent_modifier) as usize;

                    if actual_ws == sibling_ws {
                        goto!(State::ParseKey);
                    }
                    if actual_ws < sibling_ws && actual_ws.is_multiple_of(2) {
                        goto!(State::ScopeEnd);
                    }

                    fail!(ErrorType::Syntax);
                }

                _ => {
                    fail!();
                }
            }
        }
    }
}
