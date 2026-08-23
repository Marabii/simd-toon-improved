#![allow(dead_code)]
use crate::BasicTypes;
use crate::StaticNode;
#[allow(unused_imports)]
use crate::macros::unlikely;
use crate::safer_unchecked::GetSaferUnchecked;
use crate::value::tape::Node;
use crate::{DecodeOptions, Deserializer, Error, ErrorType, InternalError, Result};

#[derive(Debug)]
enum State {
    /// Close the current scope.
    ScopeEnd,

    /// Parse a key.
    ParseKey,

    /// Parse simple object value. (The normal case, no tabular shenanigans)
    ParseSimpleObjectValue,

    /// Parse tabular Objects:
    /// ```
    /// users[2:]{age,city}:
    /// alice: 30,Berlin
    /// bob: 25,Oslo
    /// ```
    /// is equivalent to
    /// ```
    /// {
    /// "users": {
    ///  "alice": {
    ///    "age": 30,
    ///    "city": "Berlin"
    ///   },
    ///   "bob": {
    ///    "age": 25,
    ///    "city": "Oslo"
    ///   }
    /// }
    ///}
    /// ```
    /// The strings vector is for header names (eg: age, city)
    ParseTabularObjects {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        rows_count: usize,
    },

    /// Parse inline array
    /// ```
    /// tags[3]: admin,ops,dev
    /// ```
    /// is equivalent to
    /// ```
    /// {"tags":[ "admin", "ops", "dev" ]}
    /// ```
    /// Stores the length of the array
    ParseInlineArray {
        count: usize,
        key: Option<(usize, usize)>,
    },

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
    ParseTabularArray {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        rows_count: usize,
    },

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
    /// Stores the length of the array
    ParseBlockArray {
        count: usize,
        key: Option<(usize, usize)>,
    },

    /// Expect the next hyphen-prefixed element (`- ...`) of a block array.
    /// Reused for every element after the first, including ones reached by
    /// cascading back out of a deeply nested list-item via `ScopeEnd`.
    ExpectBlockArrayItem,
}

#[derive(Debug)]
pub(crate) enum StackState {
    Start,
    Object { last_start: usize, cnt: usize },
    Array { last_start: usize, cnt: usize },
}

#[derive(Debug)]
enum HeaderType {
    /// BlockArraySimpleValue
    /// ```
    /// items[1]:
    ///   - some value
    /// ```
    /// Could be a regular string or a number but not a complex header like
    /// keyed tabular objects or tabular arrays
    BlockArraySimpleValue { key: (usize, usize) },

    /// It marks the start of an object,
    /// Could be an empty object or regular object key,
    /// not a tabular array or some other complex header.
    ObjectStart { key: Option<(usize, usize)> },

    /// Just an empty object, no key, no value.
    EmptyObject,

    /// SimpleArray: Could either be an Inline Array or a Block Array,
    /// We decide after parsing it.
    SimpleArray {
        count: usize,
        key: Option<(usize, usize)>,
    },

    /// EmptyArray:
    /// ```
    /// items[0]:
    /// ```
    /// This header is a complete value on its own:
    /// nothing follows the `:` on this line, nothing is nested below it,
    /// and it is allowed to be the last thing in the document.
    EmptyArray { key: Option<(usize, usize)> },

    /// Keyed Tabular Objects:
    /// ```
    /// users[2:]{age,city}:
    /// ```
    KeyedTabularObjects {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        rows_count: usize,
    },

    /// Tabular Arrays:
    /// ```
    /// items[2]{sku,qty,price}:
    /// ```
    TabularArray {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        rows_count: usize,
    },
}

/// Describes whether the next line is a sibling
/// or a nested value or the end of the current scope
#[derive(Debug)]
enum EOLState {
    Sibling,
    CloseScope,
    Nested,
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
        options: DecodeOptions,
    ) -> Result<()> {
        let strict = options.strict();
        let indent_size = options.indent_size();

        res.clear();
        res.reserve(structural_indexes.len());
        stack.clear();
        stack.reserve(structural_indexes.len());

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
        let mut last_start: usize;

        // Number of entries seen in the current container.
        // Example: for array[3]: 10,20,30 cnt becomes 3.
        let mut cnt: usize;

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

        // Whitespace count measured by the last newline crossing (`get_eol_state!`).
        // Reused by `ScopeEnd` when cascading through several closing scopes for the
        // same newline, since the newline itself is only consumed once.
        let mut last_ws: usize = 0;

        // Only block arrays need their declared count carried across states.
        // Keyed by depth, so an entry can only ever be consumed by the scope it belongs to.
        let mut pending_counts: Vec<(usize, usize)> = Vec::new(); // (depth, expected)

        #[cfg(all(
            feature = "runtime-detection",
            any(target_arch = "x86_64", target_arch = "x86"),
        ))]
        let mut parse_str = |start: usize, end: usize| unsafe {
            parse_str_fn(
                crate::SillyWrapper::from(input_ptr),
                input2,
                buffer,
                start,
                end,
            )
        };

        #[collapse_debuginfo(yes)]
        macro_rules! get {
            ($a:expr_2021, $i:expr_2021) => {{ unsafe { $a.get_kinda_unchecked($i) } }};
        }

        #[collapse_debuginfo(yes)]
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

        #[collapse_debuginfo(yes)]
        macro_rules! insert_res {
            ($t:expr_2021) => {
                unsafe {
                    res_ptr.add(r_i).write($t);
                    r_i += 1;
                }
            };
        }

        #[collapse_debuginfo(yes)]
        macro_rules! success {
            () => {
                unsafe {
                    res.set_len(r_i);
                }
                return Ok(());
            };
        }

        #[collapse_debuginfo(yes)]
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

        #[collapse_debuginfo(yes)]
        macro_rules! goto {
            ($state:expr_2021) => {{
                state = $state;
                #[allow(clippy::needless_continue)]
                continue;
            }};
        }

        #[collapse_debuginfo(yes)]
        macro_rules! insert_str {
            ($start:expr, $end:expr) => {
                insert_res!(Node::String(s2try!(parse_str($start, $end))));
            };

            ($end:expr) => {
                insert_res!(Node::String(s2try!(parse_str($end))));
            };
        }

        #[collapse_debuginfo(yes)]
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

        // When the type of value is unknown, use this macro to automatically
        // figure out the type and insert the value into the tape.
        #[collapse_debuginfo(yes)]
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
        #[collapse_debuginfo(yes)]
        macro_rules! get_value_end {
            ($err:expr, $($expected_delim:expr),+) => {{
                let hard_end;

                loop {
                    if $(c == $expected_delim)||* {
                        hard_end = idx;
                        break;
                    }

                    if unlikely!(c == b'\n') {
                        fail!($err);
                    }

                    // Keep searching forward
                    update_char!();
                }

                trim_trailing_spaces!(idx, hard_end)
            }};
        }

        // Compares an already-measured indentation width against the current `depth`,
        // without touching the input. Used both by `get_eol_state!` (for the newline
        // that was just crossed) and by `ScopeEnd` when it needs to re-evaluate the
        // very same measurement against an outer (just-popped) scope.
        #[collapse_debuginfo(yes)]
        macro_rules! eol_state_from_ws {
            ($actual_ws:expr_2021) => {{
                let actual_ws = $actual_ws;
                let sibling_ws = (((depth - 1) * indent_size) as isize + indent_modifier) as usize;
                let nested_ws = (((depth) * indent_size) as isize + indent_modifier) as usize;

                if actual_ws == sibling_ws {
                    EOLState::Sibling
                } else if actual_ws < sibling_ws && actual_ws.is_multiple_of(indent_size) {
                    EOLState::CloseScope
                } else if actual_ws == nested_ws {
                    EOLState::Nested
                } else {
                    fail!(ErrorType::Syntax);
                }
            }};
        }

        #[collapse_debuginfo(yes)]
        macro_rules! get_eol_state {
            () => {{
                if i >= structural_indexes.len() {
                    goto!(State::ScopeEnd);
                }

                if unlikely!(c != b'\n') {
                    fail!(ErrorType::Syntax);
                }

                let old_idx = idx;
                update_char!();

                if i >= structural_indexes.len() {
                    EOLState::CloseScope
                } else {
                    let new_idx = idx;

                    // Prevents double newline characters
                    if unlikely!(c == b'\n') {
                        fail!(ErrorType::Syntax);
                    }

                    last_ws = new_idx - old_idx - 1;
                    eol_state_from_ws!(last_ws)
                }
            }};
        }

        #[collapse_debuginfo(yes)]
        macro_rules! parse_string_number {
            ($start:expr, $end:expr) => {{
                let value_bytes = &input2[$start..$end];
                match value_bytes.iter().try_fold(0u32, |acc, &b| {
                    if b.is_ascii_digit() {
                        acc.checked_mul(10)?.checked_add((b - b'0') as u32)
                    } else {
                        None
                    }
                }) {
                    Some(v) => v,
                    None => {
                        fail!(ErrorType::Syntax);
                    }
                }
            }};
        }

        /// Reads one line's header and classifies it.
        ///
        /// Whatever the shape, this leaves the cursor on the token that ended
        /// the header:
        /// `:` for every keyed form
        /// `\n` (or the last token of the document) for `BlockArraySimpleValue`.
        ///
        /// Stepping past it is the
        /// caller's job, because only the caller knows whether the header is
        /// allowed to be the final thing in the document.
        macro_rules! read_header {
            () => {{
                let key_start = idx;
                let key_end = get_value_end!(ErrorType::Syntax, b':', b'[', b'\n');
                let key = if key_end > key_start {
                    Some((key_start, key_end))
                } else {
                    None
                };

                enum TabularKind {
                    SimpleArray,
                    TabularArray(Vec<(usize, usize)>),
                    KeyedObject(Vec<(usize, usize)>),
                }

                if c == b'\n' {
                    match key {
                        Some(key) => HeaderType::BlockArraySimpleValue { key },
                        None => HeaderType::EmptyObject,
                    }
                } else if c == b':' {
                    HeaderType::ObjectStart { key }
                } else if c == b'[' {
                    update_char!();
                    let rows_count_start = idx;

                    update_char!();
                    let rows_count_end = idx;
                    let rows_count =
                        parse_string_number!(rows_count_start, rows_count_end) as usize;

                    let mut kind = TabularKind::SimpleArray;

                    if c == b':' {
                        update_char!();
                        if unlikely!(c != b']') {
                            fail!(ErrorType::Syntax);
                        }

                        kind = TabularKind::KeyedObject(Vec::new());
                    }

                    update_char!();

                    if c == b'{' {
                        if matches!(kind, TabularKind::SimpleArray) {
                            kind = TabularKind::TabularArray(Vec::new());
                        }
                        // This vector records the start and end of every header
                        // Example: When parsing: users[2:]{age,city}:
                        // it should record the positions of 'age' and 'city'
                        let mut headers: Vec<(usize, usize)> = Vec::new();

                        if unlikely!(c != b'{') {
                            fail!(ErrorType::Syntax);
                        }

                        loop {
                            update_char!();

                            if c == b':' {
                                break;
                            }

                            if unlikely!(i > structural_indexes.len()) {
                                fail!(ErrorType::Syntax);
                            }

                            let value_start = idx;
                            let value_end = get_value_end!(ErrorType::Syntax, b',', b'}');
                            headers.push((value_start, value_end));
                        }

                        match kind {
                            TabularKind::TabularArray(_) => {
                                kind = TabularKind::TabularArray(headers);
                            }
                            TabularKind::KeyedObject(_) => {
                                kind = TabularKind::KeyedObject(headers);
                            }
                            _ => {
                                fail!(ErrorType::NoStructure);
                            }
                        }
                    }

                    match kind {
                        TabularKind::KeyedObject(headers) => HeaderType::KeyedTabularObjects {
                            key,
                            headers,
                            rows_count,
                        },
                        TabularKind::TabularArray(headers) => HeaderType::TabularArray {
                            key,
                            headers,
                            rows_count,
                        },
                        TabularKind::SimpleArray if rows_count == 0 => {
                            HeaderType::EmptyArray { key }
                        }
                        TabularKind::SimpleArray => HeaderType::SimpleArray {
                            count: rows_count,
                            key,
                        },
                    }
                } else {
                    fail!(ErrorType::Syntax);
                }
            }};
        }

        #[collapse_debuginfo(yes)]
        macro_rules! close_and_pop_state {
            ($node_variant:ident) => {
                unsafe {
                    match *res_ptr.add(last_start) {
                        Node::$node_variant {
                            ref mut len,
                            count: ref mut end,
                        } => {
                            *len = cnt;
                            *end = r_i - last_start - 1;
                        }
                        _ => {
                            fail!(ErrorType::NoStructure);
                        }
                    }

                    // The parent's saved tag (Object/Array) only reflects which kind of
                    // container it is; either way we just restore last_start/cnt from it.
                    match *stack_ptr.add(depth) {
                        StackState::Object {
                            last_start: parent_last_start,
                            cnt: parent_cnt,
                        } => {
                            last_start = parent_last_start;
                            cnt = parent_cnt;
                        }
                        StackState::Array {
                            last_start: parent_last_start,
                            cnt: parent_cnt,
                        } => {
                            last_start = parent_last_start;
                            cnt = parent_cnt;
                        }
                        StackState::Start => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }
            };
        }

        #[collapse_debuginfo(yes)]
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
                    let header_type = read_header!();

                    match header_type {
                        HeaderType::ObjectStart { key } => {
                            if let Some((key_start, key_end)) = key {
                                cnt += 1;
                                insert_str!(key_start, key_end);
                            }

                            update_char!();
                            goto!(State::ParseSimpleObjectValue)
                        }

                        HeaderType::SimpleArray { count, key } => {
                            update_char!(); // step past the header's ':'

                            if c == b'\n' {
                                goto!(State::ParseBlockArray { count, key })
                            } else {
                                goto!(State::ParseInlineArray { count, key })
                            }
                        }

                        HeaderType::EmptyArray { key } => {
                            if i < structural_indexes.len() {
                                update_char!();
                            }

                            goto!(State::ParseInlineArray { count: 0, key })
                        }

                        HeaderType::KeyedTabularObjects {
                            key,
                            headers,
                            rows_count,
                        } => {
                            goto!(State::ParseTabularObjects {
                                key,
                                headers,
                                rows_count
                            })
                        }

                        HeaderType::TabularArray {
                            key,
                            headers,
                            rows_count,
                        } => {
                            goto!(State::ParseTabularArray {
                                key,
                                headers,
                                rows_count
                            })
                        }

                        HeaderType::BlockArraySimpleValue { .. } | HeaderType::EmptyObject => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }

                State::ParseSimpleObjectValue => {
                    if c == b'\n' {
                        if i >= structural_indexes.len() {
                            insert_res!(Node::Object { len: 0, count: 0 });
                            goto!(State::ScopeEnd);
                        }

                        match get_eol_state!() {
                            EOLState::Nested => {
                                unsafe {
                                    stack_ptr
                                        .add(depth)
                                        .write(StackState::Object { last_start, cnt });
                                }
                                last_start = r_i;
                                depth += 1;

                                insert_res!(Node::Object { len: 0, count: 0 });
                                cnt = 0;

                                let key_start = idx;
                                let key_end = get_value_end!(ErrorType::Syntax, b':');
                                cnt += 1;
                                insert_str!(key_start, key_end);

                                update_char!();
                                goto!(State::ParseSimpleObjectValue);
                            }

                            EOLState::Sibling => {
                                // Same level or shallower -> null value.
                                insert_res!(Node::Object { len: 0, count: 0 });
                                goto!(State::ParseKey)
                            }

                            EOLState::CloseScope => {
                                goto!(State::ScopeEnd)
                            }
                        }
                    }

                    let value_start = idx;
                    let value_end = get_value_end!(ErrorType::Syntax, b'\n');

                    // This is meant to handle the WEIRD way empty arrays are represented in TOON:
                    // `key: []`
                    // WHY BREAK CONVENTION OF ARRAYS ? key[N<delimiter?>]<{fields}>:
                    if &input2[value_start..value_end] == b"[]" {
                        insert_res!(Node::Array { len: 0, count: 0 });
                    } else {
                        parse_and_insert_value!(value_start, value_end);
                    }

                    if i >= structural_indexes.len() {
                        goto!(State::ScopeEnd);
                    }

                    match get_eol_state!() {
                        EOLState::Sibling => goto!(State::ParseKey),
                        EOLState::CloseScope => {
                            goto!(State::ScopeEnd)
                        }
                        EOLState::Nested => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }

                State::ParseInlineArray { count, key } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    // The count is verified right in this block, we don't need to add it to the stack and verify it later.
                    unsafe {
                        stack_ptr.add(depth).write(if key.is_some() {
                            StackState::Object { last_start, cnt }
                        } else {
                            StackState::Array { last_start, cnt }
                        });
                    }

                    last_start = r_i;
                    depth += 1;
                    insert_res!(Node::Array { len: 0, count: 0 });
                    cnt = 0;

                    // Only parse values if there are actually values to parse
                    if count > 0 {
                        // Parse all elements except the last one
                        for _ in 1..count {
                            cnt += 1;

                            let value_start = idx;
                            let value_end = get_value_end!(ErrorType::Syntax, b',');
                            parse_and_insert_value!(value_start, value_end);

                            if unlikely!(c != b',') {
                                fail!(ErrorType::Syntax);
                            }

                            update_char!();
                        }

                        // Parse the final element
                        cnt += 1;
                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                        parse_and_insert_value!(value_start, value_end);
                    }

                    match get_eol_state!() {
                        EOLState::CloseScope | EOLState::Sibling => goto!(State::ScopeEnd),
                        EOLState::Nested => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }
                State::ParseBlockArray { count, key } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    if !matches!(get_eol_state!(), EOLState::Nested) {
                        fail!(ErrorType::ExpectedArrayContent);
                    }

                    unsafe {
                        stack_ptr.add(depth).write(if key.is_some() {
                            StackState::Object { last_start, cnt }
                        } else {
                            StackState::Array { last_start, cnt }
                        });
                    }

                    last_start = r_i;
                    depth += 1;
                    insert_res!(Node::Array { len: 0, count: 0 });
                    cnt = 0;
                    pending_counts.push((depth, count));
                    goto!(State::ExpectBlockArrayItem);
                }

                State::ExpectBlockArrayItem => {
                    if unlikely!(c != b'-') {
                        fail!(ErrorType::ExpectedArray);
                    }

                    if let Some(&(d, expected)) = pending_counts.last() {
                        debug_assert_eq!(d, depth);
                        if d == depth && unlikely!(cnt >= expected) {
                            fail!(ErrorType::Syntax); // more items than declared
                        }
                    }

                    if indent_modifier > 0 {
                        indent_modifier -= indent_size as isize;
                    }

                    update_char!(); // move past '-' onto the item's content

                    cnt += 1;

                    macro_rules! wrap_keyed_item {
                        ($key:expr_2021) => {
                            if $key.is_some() {
                                unsafe {
                                    stack_ptr
                                        .add(depth)
                                        .write(StackState::Array { last_start, cnt });
                                }
                                last_start = r_i;
                                depth += 1;
                                insert_res!(Node::Object { len: 0, count: 0 });
                                cnt = 0;
                            }
                        };
                    }

                    match read_header!() {
                        HeaderType::BlockArraySimpleValue {
                            key: (key_start, key_end),
                        } => {
                            parse_and_insert_value!(key_start, key_end);

                            if c == b'\n' {
                                match get_eol_state!() {
                                    EOLState::Sibling => goto!(State::ExpectBlockArrayItem),
                                    EOLState::CloseScope => goto!(State::ScopeEnd),
                                    EOLState::Nested => {
                                        fail!(ErrorType::NoStructure);
                                    }
                                }
                            }

                            goto!(State::ScopeEnd);
                        }

                        HeaderType::EmptyObject => {
                            unsafe {
                                stack_ptr
                                    .add(depth)
                                    .write(StackState::Array { last_start, cnt });
                            }
                            last_start = r_i;
                            depth += 1;
                            insert_res!(Node::Object { len: 0, count: 0 });
                            cnt = 0;
                            goto!(State::ScopeEnd);
                        }

                        // `- key: value`: the item is an object; open its wrapper, insert
                        // the first field, then reuse the normal object-value machinery.
                        HeaderType::ObjectStart { key } => {
                            if let Some((key_start, key_end)) = key {
                                unsafe {
                                    stack_ptr
                                        .add(depth)
                                        .write(StackState::Array { last_start, cnt });
                                }
                                last_start = r_i;
                                depth += 1;
                                insert_res!(Node::Object { len: 0, count: 0 });
                                insert_str!(key_start, key_end);
                                cnt = 1;
                            }

                            update_char!();
                            goto!(State::ParseSimpleObjectValue);
                        }

                        // `- [N]: ...` (anonymous): the item is itself an array.
                        // `- key[N]: ...`: an object whose first field is an array.
                        HeaderType::SimpleArray { count, key } => {
                            wrap_keyed_item!(key);

                            update_char!(); // step past the header's ':'

                            if c == b'\n' {
                                goto!(State::ParseBlockArray { count, key })
                            } else {
                                goto!(State::ParseInlineArray { count, key })
                            }
                        }

                        HeaderType::EmptyArray { key } => {
                            wrap_keyed_item!(key);

                            if i < structural_indexes.len() {
                                update_char!();
                            }

                            goto!(State::ParseInlineArray { count: 0, key })
                        }

                        // `- key[N:]{...}:`: an object whose first field is a keyed
                        // tabular block.
                        HeaderType::KeyedTabularObjects {
                            key,
                            headers,
                            rows_count,
                        } => {
                            wrap_keyed_item!(key);

                            indent_modifier += indent_size as isize;
                            goto!(State::ParseTabularObjects {
                                key,
                                headers,
                                rows_count
                            })
                        }

                        HeaderType::TabularArray {
                            key,
                            headers,
                            rows_count,
                        } => {
                            wrap_keyed_item!(key);
                            indent_modifier += indent_size as isize;
                            goto!(State::ParseTabularArray {
                                key,
                                headers,
                                rows_count
                            })
                        }
                    }
                }

                State::ParseTabularObjects {
                    key,
                    headers,
                    rows_count,
                } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    unsafe {
                        stack_ptr.add(depth).write(if key.is_some() {
                            StackState::Object { last_start, cnt }
                        } else {
                            StackState::Array { last_start, cnt }
                        });
                    }
                    last_start = r_i;
                    depth += 1;
                    insert_res!(Node::Object { len: 0, count: 0 });
                    cnt = 0;

                    update_char!(); // step past the header's ':'
                    update_char!(); // ... and past the line's trailing '\n'

                    let n_headers = headers.len();

                    for row_idx in 0..rows_count {
                        let row_key_start = idx;
                        let row_key_end = get_value_end!(ErrorType::Syntax, b':');

                        if unlikely!(c != b':') {
                            fail!(ErrorType::Syntax);
                        }

                        cnt += 1;
                        insert_str!(row_key_start, row_key_end);

                        // Open the row's object, e.g. "alice": { "age": 30, "city": "Berlin" }
                        unsafe {
                            stack_ptr
                                .add(depth)
                                .write(StackState::Object { last_start, cnt });
                        }
                        last_start = r_i;
                        insert_res!(Node::Object { len: 0, count: 0 });
                        cnt = 0;

                        update_char!(); // skip ':' to reach the first field value

                        for (h_idx, &(h_start, h_end)) in headers.iter().enumerate() {
                            insert_str!(h_start, h_end);
                            cnt += 1;

                            let is_last_header = h_idx + 1 == n_headers;
                            let value_start = idx;
                            let value_end = if is_last_header {
                                get_value_end!(ErrorType::Syntax, b'\n')
                            } else {
                                get_value_end!(ErrorType::Syntax, b',')
                            };
                            parse_and_insert_value!(value_start, value_end);

                            if !is_last_header {
                                if unlikely!(c != b',') {
                                    fail!(ErrorType::Syntax);
                                }
                                update_char!();
                            }
                        }

                        close_and_pop_state!(Object);

                        let is_last_row = row_idx + 1 == rows_count;
                        if !is_last_row {
                            update_char!(); // advance past this row's '\n' to the next row's key
                        }
                    }

                    goto!(State::ScopeEnd);
                }

                State::ParseTabularArray {
                    key,
                    headers,
                    rows_count,
                } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    unsafe {
                        stack_ptr.add(depth).write(if key.is_some() {
                            StackState::Object { last_start, cnt }
                        } else {
                            StackState::Array { last_start, cnt }
                        });
                    }
                    last_start = r_i;
                    depth += 1;
                    insert_res!(Node::Array { len: 0, count: 0 });
                    cnt = 0;

                    update_char!(); // step past the header's ':'
                    update_char!(); // ... and past the line's trailing '\n'

                    let n_headers = headers.len();

                    for row_idx in 0..rows_count {
                        cnt += 1;

                        // Open the row's object, e.g. { "sku": "A1", "qty": 2, "price": 9.99 }
                        unsafe {
                            stack_ptr
                                .add(depth)
                                .write(StackState::Array { last_start, cnt });
                        }
                        last_start = r_i;
                        insert_res!(Node::Object { len: 0, count: 0 });
                        cnt = 0;

                        for (h_idx, &(h_start, h_end)) in headers.iter().enumerate() {
                            insert_str!(h_start, h_end);
                            cnt += 1;

                            let is_last_header = h_idx + 1 == n_headers;
                            let value_start = idx;
                            let value_end = if is_last_header {
                                get_value_end!(ErrorType::Syntax, b'\n')
                            } else {
                                get_value_end!(ErrorType::Syntax, b',')
                            };
                            parse_and_insert_value!(value_start, value_end);

                            if !is_last_header {
                                if unlikely!(c != b',') {
                                    fail!(ErrorType::Syntax);
                                }
                                update_char!();
                            }
                        }

                        close_and_pop_state!(Object);

                        let is_last_row = row_idx + 1 == rows_count;
                        if !is_last_row {
                            update_char!(); // advance past this row's '\n' to the next row's first value
                        }
                    }

                    goto!(State::ScopeEnd);
                }

                State::ScopeEnd => {
                    if unlikely!(depth == 0) {
                        fail!(ErrorType::Syntax);
                    }

                    if let Some(&(d, expected)) = pending_counts.last()
                        && d == depth
                    {
                        pending_counts.pop();
                        if unlikely!(cnt != expected) {
                            fail!(ErrorType::Syntax); // fewer items than declared
                        }
                    }

                    if indent_modifier > 0 {
                        indent_modifier -= indent_size as isize;
                    }

                    depth -= 1;

                    unsafe {
                        // Backfill the tape:
                        match *res_ptr.add(last_start) {
                            Node::Object {
                                ref mut len,
                                count: ref mut end,
                            }
                            | Node::Array {
                                ref mut len,
                                count: ref mut end,
                            } => {
                                *len = cnt;
                                *end = r_i - last_start - 1;
                            }
                            _ => unreachable!(),
                        }

                        // Update the stack state. The tag records what kind of container
                        // we're returning to, so `Sibling` knows whether to expect another
                        // object key (`ParseKey`) or another block-array item (`-`).
                        match *stack_ptr.add(depth) {
                            StackState::Object {
                                last_start: l,
                                cnt: parent_cnt,
                            } => {
                                last_start = l;
                                cnt = parent_cnt;

                                if i >= structural_indexes.len() {
                                    goto!(State::ScopeEnd);
                                }

                                // `c == b'\n'` means this newline hasn't been consumed yet
                                // (fresh close, e.g. right after a tabular block). Otherwise
                                // we're cascading through several closes for a newline that
                                // `get_eol_state!` already consumed, so reuse its measurement
                                // instead of expecting another (nonexistent) `\n`.
                                let eol_state = if c == b'\n' {
                                    get_eol_state!()
                                } else {
                                    eol_state_from_ws!(last_ws)
                                };

                                match eol_state {
                                    EOLState::CloseScope => {
                                        goto!(State::ScopeEnd);
                                    }

                                    EOLState::Nested => {
                                        fail!(ErrorType::NoStructure);
                                    }

                                    EOLState::Sibling => {
                                        goto!(State::ParseKey);
                                    }
                                }
                            }

                            StackState::Array {
                                last_start: l,
                                cnt: parent_cnt,
                            } => {
                                last_start = l;
                                cnt = parent_cnt;

                                if i >= structural_indexes.len() {
                                    goto!(State::ScopeEnd);
                                }

                                let eol_state = if c == b'\n' {
                                    get_eol_state!()
                                } else {
                                    eol_state_from_ws!(last_ws)
                                };

                                match eol_state {
                                    EOLState::CloseScope => {
                                        goto!(State::ScopeEnd);
                                    }

                                    EOLState::Nested => {
                                        fail!(ErrorType::NoStructure);
                                    }

                                    EOLState::Sibling => {
                                        goto!(State::ExpectBlockArrayItem);
                                    }
                                }
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
                        }
                    }
                }
                _ => {
                    fail!();
                }
            }
        }
    }
}
