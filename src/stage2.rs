#![allow(dead_code)]
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
    ParseHeader,

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
        delimiter: u8,
        is_root: bool,
    },

    /// Parse inline array
    /// ```
    /// tags[3]: admin,ops,dev
    /// ```
    /// is equivalent to
    /// ```
    /// {"tags":[ "admin", "ops", "dev" ]}
    /// ```
    /// Stores the length of the array guarenteed to be > 0
    ParseInlineArray {
        count: usize,
        key: Option<(usize, usize)>,
        delimiter: u8,
        is_root: bool,
    },

    /// Parse empty array:
    /// ```
    /// items[0]:
    /// ```
    /// This header is a complete value on its own:
    /// nothing follows the `:` on this line, nothing is nested below it,
    ParseEmptyArray {
        key: Option<(usize, usize)>,
        is_root: bool,
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
    /// Side note, I'm well aware there's a risk of writing to unallocated memory since tabular arrays don't have
    /// enough structurals, I'll work on it later.
    ParseTabularArrayStrict {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        rows_count: usize,
        delimiter: u8,
        is_root: bool,
    },

    /// Same as ParseTabularArrayStrict but used in lenient mode.
    /// It's not as efficient as the strict variant.
    ParseTabularArrayLenient {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        delimiter: u8,
        is_root: bool,
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
    ParseNestedFieldGroupsArrayStrict {
        key: Option<(usize, usize)>,
        nested_fields: NestedFields,
        rows_count: usize,
        delimiter: u8,
        /// True when this header is the very first one in the document and has no key.
        is_root: bool,
    },

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
        is_root: bool,
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
enum FieldEntry {
    Leaf((usize, usize)),
    Nested {
        name: (usize, usize),
        children: Vec<FieldEntry>,
    },
}

#[derive(Debug)]
struct NestedFields {
    field_entries: Vec<FieldEntry>,
    leaf_count: usize,
    nested_count: usize,
}

#[derive(Debug)]
enum HeaderType {
    /// PrimitiveValue
    /// Could be in a block array or in the start of the TOON file.
    /// ```
    /// items[1]:
    ///   - some value
    /// ```
    /// Could be a regular string or a number but not a complex header like
    /// keyed tabular objects or tabular arrays
    PrimitiveValue { val: (usize, usize) },

    /// It marks the key of an object,
    /// not a tabular array or some other complex header.
    ObjectStart { key: (usize, usize) },

    /// Just an empty object, no key, no value.
    EmptyObject,

    /// SimpleArray: Could either be an Inline Array or a Block Array,
    /// We decide after parsing it.
    SimpleArray {
        count: usize,
        key: Option<(usize, usize)>,
        delimiter: u8,
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
        delimiter: u8,
    },

    /// Tabular Arrays:
    /// ```
    /// items[2]{sku,qty,price}:
    /// ```
    TabularArray {
        key: Option<(usize, usize)>,
        headers: Vec<(usize, usize)>,
        rows_count: usize,
        delimiter: u8,
    },

    /// Tabular Arrays where at least one field entry carries its own nested
    /// field group:
    /// ```
    /// orders[2]{id,customer{name,country},total}:
    /// ```
    /// Rows stay flat; the leaf-field sequence is the depth-first, pre-order
    /// walk of `fields` (§9.3).
    NestedFieldGroupsArray {
        key: Option<(usize, usize)>,
        nested_fields: NestedFields,
        rows_count: usize,
        delimiter: u8,
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

        // Some data structures (Tabular Arrays and Nested Field Groups) are so efficient
        // that the number of tape slots they write out is greater than the number of structural indexes
        // they have. We introduce a bit of slack to try to avoid as much as possible expensive reallocations.
        // 5 here is just a heuristic and will need fine-tuning.
        let mut tape_slack: usize = structural_indexes.len() / 5;

        res.clear();
        res.reserve(structural_indexes.len() + tape_slack);
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

        let mut res_ptr = res.as_mut_ptr();
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
        // Example: c == b'{' when entering an object, c == delimiter between values.
        let mut c: u8 = 0;

        // Cursor into `structural_indexes`.
        // Example: i == 5 means the next update_char!() reads structural_indexes[5].
        let mut i: usize = 0;

        // Current state of the stage-2 state machine.
        // Example: State::ParseHeader means the parser currently expects to parse a key.
        let mut state;

        // A stack of whitespaces to keep track of depth
        // The next items are considered siblings if their indentation is
        // content_ws_stack.last()
        let mut content_ws_stack: Vec<usize> = Vec::new();

        // Only block arrays need their declared count carried across states.
        // Keyed by depth, so an entry can only ever be consumed by the scope it belongs to.
        let mut pending_counts: Vec<(usize, usize)> = Vec::new(); // (depth, expected)

        // The indentation of the next real token, measured the one time `get_eol_state!`
        // actually reads a newline. A ScopeEnd cascade (dedenting past several containers
        // at once) re-reads this instead of the newline, which no longer exists at those
        // levels, so it must reuse this measurement rather than compare a level's own
        // expected indentation to itself.
        let mut last_dedent_ws: usize = 0;

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
        macro_rules! grow_res {
            () => {{
                grow_res!(0);
            }};

            ($minimum_required:expr) => {
                unsafe {
                    let old_cap = res.capacity();

                    // the growth factor is another heuristic.
                    // it is kept small since the number structural indexes is
                    // so much bigger than the deficit created by tabular arrays and nested field objects
                    // but again, it must be fine tuned on real data.
                    let growth_factor = 0.2;
                    let factor_added = ((old_cap as f64) * growth_factor) as usize;

                    let added_cap = std::cmp::max(factor_added, $minimum_required);
                    let new_cap = old_cap + added_cap;

                    res.reserve_exact(new_cap - res.len()); // len is always 0 here, so effectively new_cap
                    res_ptr = res.as_mut_ptr();

                    tape_slack += added_cap;
                }
            };
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
                    if prev_char == b' ' || prev_char == b'\r' {
                        end -= 1;
                    } else {
                        break;
                    }
                }
                end
            }};
        }

        #[collapse_debuginfo(yes)]
        macro_rules! insert_inferred_value {
            ($start:expr, $end:expr) => {
                match *get!(input2, $start) {
                    first @ (b'0'..=b'9' | b'-') => {
                        match Self::try_parse_number($start, $end, input2, first == b'-') {
                            Ok(number) => {
                                insert_res!(Node::Static(number));
                            }
                            // Not a number after all -- `05`, `1.`, `12 monkeys`,
                            // `-Infinity` -- which makes it a string.
                            Err(_) => {
                                insert_str!($start, $end);
                            }
                        }
                    }

                    _ => match &input2[$start..$end] {
                        b"true" => {
                            insert_res!(Node::Static(StaticNode::Bool(true)));
                        }
                        b"false" => {
                            insert_res!(Node::Static(StaticNode::Bool(false)));
                        }
                        b"null" => {
                            insert_res!(Node::Static(StaticNode::Null));
                        }
                        _ => {
                            insert_str!($start, $end);
                        }
                    },
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
                // `idx` walks forward below, so the token's first byte has to be
                // taken now -- it is the floor the trim must not walk past.
                let token_start = idx;
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

                trim_trailing_spaces!(token_start, hard_end)
            }};
        }

        #[collapse_debuginfo(yes)]
        macro_rules! eol_state_from_ws {
            ($actual_ws:expr_2021) => {{
                let actual_ws = $actual_ws;
                let sibling_ws = curr_indent!();

                if actual_ws == sibling_ws {
                    EOLState::Sibling
                } else if actual_ws < sibling_ws {
                    EOLState::CloseScope
                } else if actual_ws == sibling_ws + indent_size {
                    EOLState::Nested
                } else {
                    fail!(ErrorType::Syntax);
                }
            }};
        }

        #[collapse_debuginfo(yes)]
        /// should_error_on_newline allows us to specify whether encountering consecutive
        /// newlines should trigger a syntax error.
        macro_rules! get_eol_state {
            () => {
                get_eol_state!(false)
            };

            ($should_error_on_newline: expr) => {{
                if i >= structural_indexes.len() {
                    EOLState::CloseScope
                } else {
                    if unlikely!(c != b'\n') {
                        fail!(ErrorType::Syntax);
                    }

                    let mut old_idx = idx;
                    update_char!();

                    let mut consecutive_newlines_detected = false;

                    while c == b'\n' {
                        consecutive_newlines_detected = true;
                        old_idx = idx;
                        if i >= structural_indexes.len() {
                            break;
                        }
                        update_char!();
                    }

                    if i >= structural_indexes.len() {
                        EOLState::CloseScope
                    } else {
                        last_dedent_ws = idx - old_idx - 1;

                        // A block array's span runs from its header through the last
                        // line of its content; a blank line inside that span is a
                        // strict-mode error. The dedent only *leaves* the span once
                        // it drops below the item indentation of the outermost still-open
                        // block array -- reaching the declared item count early doesn't
                        // end the span, only dedenting past it does.
                        let still_inside_pending_array = pending_counts
                            .first()
                            .is_some_and(|&(d, _)| last_dedent_ws >= content_ws_stack[d - 1]);

                        if consecutive_newlines_detected
                            && strict
                            && ($should_error_on_newline || still_inside_pending_array)
                        {
                            fail!(ErrorType::Syntax);
                        }

                        eol_state_from_ws!(last_dedent_ws)
                    }
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

        #[collapse_debuginfo(yes)]
        macro_rules! is_non_default_delimiter {
            () => {{ c == b'|' || c == b'\t' }};
        }

        /// Reads one line's header and classifies it.
        ///
        /// Whatever the shape, this leaves the cursor on the token that ended
        /// the header:
        /// `:` for every keyed form
        /// `\n` (or the last token of the document) for `PrimitiveValue`.
        ///
        /// Stepping past it is the
        /// caller's job, because only the caller knows whether the header is
        /// allowed to be the final thing in the document.
        #[collapse_debuginfo(yes)]
        macro_rules! read_header {
            () => {{
                let key_start = idx;
                let key_end = get_value_end!(ErrorType::Syntax, b':', b'[', b'\n');
                let key = if key_end > key_start {
                    Some((key_start, key_end))
                } else {
                    None
                };

                let mut delimiter = b',';

                match c {
                    b':' => match key {
                        Some(v) => HeaderType::ObjectStart { key: v },
                        None => {
                            fail!(ErrorType::Syntax);
                        }
                    },

                    b'\n' => match key {
                        Some(key) => HeaderType::PrimitiveValue { val: key },
                        None => HeaderType::EmptyObject,
                    },

                    b'[' => {
                        // The row count sits between the `[` and whatever token
                        // closes the segment (`]`, or the `:` of `[N:]`).
                        update_char!();
                        let rows_count_start = idx;
                        update_char!();
                        let rows_count = parse_string_number!(rows_count_start, idx) as usize;

                        // `[N:]` marks keyed tabular objects, `[N]` an array.
                        let keyed = c == b':';
                        if keyed {
                            update_char!();
                        }

                        if is_non_default_delimiter!() {
                            delimiter = c;
                            update_char!(); // step past the delimiter
                        }

                        if unlikely!(c != b']') {
                            fail!(ErrorType::Syntax);
                        }

                        update_char!(); // step past the ']'

                        if c != b'{' {
                            // No field list, so the header is already complete.
                            if keyed {
                                HeaderType::KeyedTabularObjects {
                                    key,
                                    headers: Vec::new(),
                                    rows_count,
                                    delimiter
                                }
                            } else if rows_count == 0 {
                                HeaderType::EmptyArray { key }
                            } else {
                                HeaderType::SimpleArray {
                                    count: rows_count,
                                    key,
                                    delimiter
                                }
                            }
                        } else {
                            // Fast path: a flat field list collected straight
                            // into the `Vec<(usize, usize)>` the tabular states
                            // want. A nested field group is rare, so it only
                            // pays for the richer `FieldEntry` tree when one
                            // actually shows up, reusing what we have as level 0.
                            let mut fields: Vec<(usize, usize)> = Vec::with_capacity(8);
                            let mut nested: Option<NestedFields> = None;

                            loop {
                                update_char!();

                                let value_start = idx;
                                let value_end = get_value_end!(ErrorType::Syntax, delimiter, b'}', b'{');

                                if unlikely!(c == b'{') {
                                    let mut leaf_count = fields.len();
                                    let mut nested_count = 0;
                                    let mut field_stack: Vec<Vec<FieldEntry>> =
                                        Vec::with_capacity(4);

                                    field_stack
                                        .push(fields.drain(..).map(FieldEntry::Leaf).collect());

                                    field_stack.push(Vec::new());

                                    let mut pending_names: Vec<(usize, usize)> =
                                        vec![(value_start, value_end)];

                                    let field_entries = 'field_list: loop {
                                        update_char!();

                                        let value_start = idx;
                                        let value_end =
                                            get_value_end!(ErrorType::Syntax, delimiter, b'}', b'{');

                                        if c == b'{' {
                                            field_stack.push(Vec::new());
                                            pending_names.push((value_start, value_end));
                                            continue;
                                        }

                                        match field_stack.last_mut() {
                                            Some(level) => {
                                                leaf_count += 1;
                                                level.push(FieldEntry::Leaf((value_start, value_end)));
                                            }
                                            None => {
                                                fail!(ErrorType::NoStructure);
                                            }
                                        }

                                        // A leaf can close one or more field lists at
                                        // once (e.g. the innermost `}` of `a{b{c}}`), so
                                        // keep popping until we hit a sibling `,` or run
                                        // out of levels, at which point the whole header
                                        // field list is done.
                                        while c == b'}' {
                                            let children = match field_stack.pop() {
                                                Some(v) => v,
                                                None => {
                                                    fail!(ErrorType::NoStructure);
                                                }
                                            };

                                            if field_stack.is_empty() {
                                                update_char!(); // step past this '}' onto the header's ':'
                                                break 'field_list children;
                                            }

                                            let name = match pending_names.pop() {
                                                Some(v) => v,
                                                None => {
                                                    fail!(ErrorType::NoStructure);
                                                }
                                            };

                                            match field_stack.last_mut() {
                                                Some(level) => {
                                                    nested_count += 1;
                                                    level.push(FieldEntry::Nested { name, children });
                                                }
                                                None => {
                                                    fail!(ErrorType::NoStructure);
                                                }
                                            }

                                            update_char!(); // step past this '}' onto ',' or the next '}'
                                        }
                                    };

                                    nested = Some(NestedFields {
                                        field_entries,
                                        leaf_count,
                                        nested_count
                                    });
                                    break;
                                }

                                fields.push((value_start, value_end));

                                if c == b'}' {
                                    update_char!(); // step past this '}' onto the header's ':'
                                    break;
                                }
                            }

                            match nested {
                                // Nested field groups in keyed headers aren't
                                // supported yet.
                                Some(_) if keyed => {
                                    fail!(ErrorType::NoStructure);
                                }
                                Some(nested_fields) => HeaderType::NestedFieldGroupsArray {
                                    key,
                                    nested_fields,
                                    rows_count,
                                    delimiter
                                },
                                None if keyed => HeaderType::KeyedTabularObjects {
                                    key,
                                    headers: fields,
                                    rows_count,
                                    delimiter
                                },
                                None => HeaderType::TabularArray {
                                    key,
                                    headers: fields,
                                    rows_count,
                                    delimiter
                                },
                            }
                        }
                    }

                    _ => {
                        fail!(ErrorType::Syntax);
                    }
                }
            }};
        }

        /// The frame that saves the container we are currently *inside of*.
        /// `frame!(keyed key)`: a keyed header only occurs inside an object,
        /// an unkeyed one only inside an array.
        #[collapse_debuginfo(yes)]
        macro_rules! frame {
            (Object) => {
                StackState::Object { last_start, cnt }
            };
            (Array) => {
                StackState::Array { last_start, cnt }
            };
            (keyed $key:expr_2021) => {
                if $key.is_some() {
                    frame!(Object)
                } else {
                    frame!(Array)
                }
            };
        }

        /// Usage:
        ///   open_scope!(Object | Array, parent: frame!(..), indent: <children's ws>);
        #[collapse_debuginfo(yes)]
        macro_rules! open_scope {
            ($node:ident, parent: $parent:expr_2021, indent: $ws:expr_2021) => {{
                // Evaluate everything that describes the *parent* before touching state.
                let parent = $parent;
                let ws = $ws;
                unsafe { stack_ptr.add(depth).write(parent) };
                depth += 1;
                content_ws_stack.push(ws);
                last_start = r_i;
                insert_res!(Node::$node { len: 0, count: 0 });
                cnt = 0;
            }};
        }

        /// Used only in Tabular formats when handling different rows.
        /// we handle all rows in a single loop without moving to ScopeEnd to close
        /// the current node and update the state which is where this macro comes in handy.
        #[collapse_debuginfo(yes)]
        macro_rules! close_and_pop_state {
            ($node_variant:ident) => {
                content_ws_stack.pop();
                depth -= 1;
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
        macro_rules! curr_indent {
            () => {{ content_ws_stack.last().copied().unwrap_or(0) }};
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
        depth += 1;
        content_ws_stack.push(0);
        cnt = 0;

        update_char!();

        // Skip initial comments.
        if c == b'#' {
            update_char!();
        }

        // Skip any blank lines.
        while c == b'\n' {
            update_char!();
        }

        let header_type = read_header!();

        // This check is used to decide which envelope to put the rest of the items in:
        // an array or an object.
        if let HeaderType::PrimitiveValue {
            val: (val_start, val_end),
        } = header_type
        {
            if i < structural_indexes.len() {
                fail!(ErrorType::Syntax);
            }

            insert_inferred_value!(val_start, val_end);
            success!();
        }

        let (root_is_object, root_is_array) = match &header_type {
            HeaderType::ObjectStart { .. } => (true, false),
            HeaderType::KeyedTabularObjects { .. } => (true, false),
            HeaderType::SimpleArray { key, .. }
            | HeaderType::EmptyArray { key, .. }
            | HeaderType::TabularArray { key, .. }
            | HeaderType::NestedFieldGroupsArray { key, .. } => (key.is_some(), key.is_none()),
            HeaderType::PrimitiveValue { .. } => (false, false),
            HeaderType::EmptyObject => (true, false),
        };

        if root_is_object {
            insert_res!(Node::Object { len: 0, count: 0 });
        }

        if root_is_array {
            insert_res!(Node::Array { len: 0, count: 0 });
        }

        match header_type {
            HeaderType::ObjectStart {
                key: (key_start, key_end),
            } => {
                cnt += 1;
                insert_str!(key_start, key_end);

                update_char!();
                state = State::ParseSimpleObjectValue;
            }

            HeaderType::SimpleArray {
                count,
                key,
                delimiter,
            } => {
                update_char!(); // step past the header's ':'

                if c == b'\n' {
                    state = State::ParseBlockArray {
                        count,
                        key,
                        is_root: root_is_array,
                    };
                } else {
                    state = State::ParseInlineArray {
                        count,
                        key,
                        delimiter,
                        is_root: root_is_array,
                    };
                }
            }

            HeaderType::EmptyArray { key } => {
                state = State::ParseEmptyArray {
                    key,
                    is_root: root_is_array,
                };
            }

            HeaderType::KeyedTabularObjects {
                key,
                headers,
                rows_count,
                delimiter,
            } => {
                state = State::ParseTabularObjects {
                    key,
                    headers,
                    rows_count,
                    delimiter,
                    // Unlike `root_is_object` (which only says whether a
                    // placeholder `Object` was inserted -- true here even
                    // when keyed, since the map itself is always an
                    // Object), reuse of that placeholder is only correct
                    // when this header has no key: a keyed header's
                    // placeholder is the *wrapping* document root, and the
                    // map still needs its own, separate container.
                    is_root: key.is_none(),
                };
            }

            HeaderType::TabularArray {
                key,
                headers,
                rows_count,
                delimiter,
            } => {
                if strict {
                    state = State::ParseTabularArrayStrict {
                        key,
                        headers,
                        rows_count,
                        delimiter,
                        is_root: root_is_array,
                    };
                } else {
                    state = State::ParseTabularArrayLenient {
                        key,
                        headers,
                        delimiter,
                        is_root: root_is_array,
                    };
                }
            }

            HeaderType::NestedFieldGroupsArray {
                key,
                nested_fields,
                rows_count,
                delimiter,
            } => {
                state = State::ParseNestedFieldGroupsArrayStrict {
                    key,
                    nested_fields,
                    rows_count,
                    delimiter,
                    is_root: root_is_array,
                };
            }

            HeaderType::PrimitiveValue { .. } | HeaderType::EmptyObject => {
                fail!(ErrorType::NoStructure);
            }
        }

        loop {
            match state {
                State::ParseHeader => {
                    let header_type = read_header!();

                    match header_type {
                        HeaderType::ObjectStart {
                            key: (key_start, key_end),
                        } => {
                            cnt += 1;
                            insert_str!(key_start, key_end);

                            update_char!();
                            goto!(State::ParseSimpleObjectValue)
                        }

                        HeaderType::SimpleArray {
                            count,
                            key,
                            delimiter,
                        } => {
                            update_char!(); // step past the header's ':'

                            if c == b'\n' {
                                goto!(State::ParseBlockArray {
                                    count,
                                    key,
                                    is_root: false
                                })
                            } else {
                                goto!(State::ParseInlineArray {
                                    count,
                                    key,
                                    delimiter,
                                    is_root: false,
                                })
                            }
                        }

                        HeaderType::EmptyArray { key, .. } => {
                            goto!(State::ParseEmptyArray {
                                key,
                                is_root: false
                            })
                        }

                        HeaderType::KeyedTabularObjects {
                            key,
                            headers,
                            rows_count,
                            delimiter,
                        } => {
                            goto!(State::ParseTabularObjects {
                                key,
                                headers,
                                rows_count,
                                delimiter,
                                is_root: false
                            })
                        }

                        HeaderType::TabularArray {
                            key,
                            headers,
                            rows_count,
                            delimiter,
                        } => {
                            goto!(State::ParseTabularArrayStrict {
                                key,
                                headers,
                                rows_count,
                                delimiter,
                                is_root: false
                            })
                        }

                        HeaderType::NestedFieldGroupsArray {
                            key,
                            nested_fields,
                            rows_count,
                            delimiter,
                        } => {
                            goto!(State::ParseNestedFieldGroupsArrayStrict {
                                key,
                                nested_fields,
                                rows_count,
                                delimiter,
                                is_root: false
                            })
                        }

                        HeaderType::PrimitiveValue { .. } | HeaderType::EmptyObject => {
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
                                open_scope!(Object, parent: frame!(Object), indent: curr_indent!() + indent_size);
                                goto!(State::ParseHeader);
                            }

                            EOLState::Sibling => {
                                // Same level or shallower -> null value.
                                insert_res!(Node::Object { len: 0, count: 0 });
                                goto!(State::ParseHeader)
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
                        insert_inferred_value!(value_start, value_end);
                    }

                    if i >= structural_indexes.len() {
                        goto!(State::ScopeEnd);
                    }

                    match get_eol_state!() {
                        EOLState::Sibling => goto!(State::ParseHeader),
                        EOLState::CloseScope => {
                            goto!(State::ScopeEnd)
                        }
                        EOLState::Nested => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }

                State::ParseEmptyArray { key, is_root } => {
                    update_char!(); // step past the header's ':'

                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Array, parent: frame!(keyed key), indent: curr_indent!() + indent_size);
                    }

                    match get_eol_state!() {
                        EOLState::CloseScope | EOLState::Sibling => goto!(State::ScopeEnd),
                        EOLState::Nested => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }

                State::ParseInlineArray {
                    count,
                    key,
                    delimiter,
                    is_root,
                } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Array, parent: frame!(keyed key), indent: curr_indent!());
                    }

                    // Parse all elements except the last one
                    for _ in 1..count {
                        cnt += 1;

                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, delimiter);
                        insert_inferred_value!(value_start, value_end);

                        if unlikely!(c != delimiter) {
                            fail!(ErrorType::Syntax);
                        }

                        update_char!();
                    }

                    // Parse the final element
                    cnt += 1;
                    let value_start = idx;
                    let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                    insert_inferred_value!(value_start, value_end);

                    match get_eol_state!(true) {
                        EOLState::CloseScope | EOLState::Sibling => goto!(State::ScopeEnd),
                        EOLState::Nested => {
                            fail!(ErrorType::NoStructure);
                        }
                    }
                }

                State::ParseBlockArray {
                    count,
                    key,
                    is_root,
                } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    if !matches!(get_eol_state!(), EOLState::Nested) {
                        fail!(ErrorType::ExpectedArrayContent);
                    }

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Array, parent: frame!(keyed key), indent: curr_indent!() + indent_size);
                    }

                    pending_counts.push((depth, count));
                    goto!(State::ExpectBlockArrayItem);
                }

                State::ExpectBlockArrayItem => {
                    if unlikely!(c != b'-') {
                        fail!(ErrorType::ExpectedArray);
                    }

                    if strict && let Some(&(d, expected)) = pending_counts.last() {
                        debug_assert_eq!(d, depth);
                        if d == depth && unlikely!(cnt >= expected) {
                            fail!(ErrorType::Syntax); // more items than declared (Block array length mismatch)
                        }
                    }

                    update_char!(); // move past '-' onto the item's content

                    cnt += 1;

                    match read_header!() {
                        HeaderType::PrimitiveValue {
                            val: (val_start, val_end),
                        } => {
                            insert_inferred_value!(val_start, val_end);

                            match get_eol_state!() {
                                EOLState::Sibling => goto!(State::ExpectBlockArrayItem),
                                EOLState::CloseScope => goto!(State::ScopeEnd),
                                EOLState::Nested => {
                                    fail!(ErrorType::NoStructure);
                                }
                            }
                        }

                        HeaderType::EmptyObject => {
                            insert_res!(Node::Object { len: 0, count: 0 });

                            match get_eol_state!() {
                                EOLState::Sibling => goto!(State::ExpectBlockArrayItem),
                                EOLState::CloseScope => goto!(State::ScopeEnd),
                                EOLState::Nested => {
                                    fail!(ErrorType::NoStructure);
                                }
                            }
                        }

                        // `- key: value`: the item is an object; open its wrapper, insert
                        // the first field, then reuse the normal object-value machinery.
                        HeaderType::ObjectStart {
                            key: (key_start, key_end),
                        } => {
                            open_scope!(Object, parent: frame!(Array), indent: curr_indent!() + 2);
                            insert_str!(key_start, key_end);
                            cnt += 1;
                            update_char!();
                            goto!(State::ParseSimpleObjectValue);
                        }

                        // `- [N]: ...` (anonymous): the item is itself an array.
                        // `- key[N]: ...`: an object whose first field is an array.
                        HeaderType::SimpleArray {
                            count,
                            key,
                            delimiter,
                        } => {
                            if key.is_some() {
                                open_scope!(Object, parent: frame!(Array), indent: curr_indent!() + indent_size);
                            }

                            update_char!(); // step past the header's ':'

                            if c == b'\n' {
                                goto!(State::ParseBlockArray {
                                    count,
                                    key,
                                    is_root: false
                                })
                            } else {
                                goto!(State::ParseInlineArray {
                                    count,
                                    key,
                                    delimiter,
                                    is_root: false
                                })
                            }
                        }

                        HeaderType::EmptyArray { key } => {
                            if key.is_some() {
                                open_scope!(Object, parent: frame!(Array), indent: curr_indent!() + indent_size);
                            }

                            goto!(State::ParseEmptyArray {
                                key,
                                is_root: false
                            })
                        }

                        // `- key[N:]{...}:`: an object whose first field is a keyed
                        // tabular block.
                        HeaderType::KeyedTabularObjects {
                            key,
                            headers,
                            rows_count,
                            delimiter,
                        } => {
                            if key.is_some() {
                                open_scope!(Object, parent: frame!(Array), indent: curr_indent!() + indent_size);
                            }

                            goto!(State::ParseTabularObjects {
                                key,
                                headers,
                                rows_count,
                                delimiter,
                                is_root: false
                            })
                        }

                        HeaderType::TabularArray {
                            key,
                            headers,
                            rows_count,
                            delimiter,
                        } => {
                            if key.is_some() {
                                open_scope!(Object, parent: frame!(Array), indent: curr_indent!() + indent_size);
                            }

                            goto!(State::ParseTabularArrayStrict {
                                key,
                                headers,
                                rows_count,
                                delimiter,
                                is_root: false
                            })
                        }

                        HeaderType::NestedFieldGroupsArray {
                            key,
                            nested_fields,
                            rows_count,
                            delimiter,
                        } => {
                            if key.is_some() {
                                open_scope!(Object, parent: frame!(Array), indent: curr_indent!() + indent_size);
                            }

                            goto!(State::ParseNestedFieldGroupsArrayStrict {
                                key,
                                nested_fields,
                                rows_count,
                                delimiter,
                                is_root: false
                            })
                        }
                    }
                }

                State::ParseTabularObjects {
                    key,
                    headers,
                    rows_count,
                    delimiter,
                    is_root,
                } => {
                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    update_char!(); // step past the header's ':'
                    if !matches!(get_eol_state!(), EOLState::Nested) {
                        fail!(ErrorType::ExpectedArrayContent);
                    }

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Object, parent: frame!(keyed key), indent: curr_indent!() + indent_size);
                    }

                    let n_headers = headers.len();

                    #[collapse_debuginfo(yes)]
                    macro_rules! reject_cell_less_row {
                        ($start:expr, $end:expr) => {
                            if unlikely!(n_headers == 1 && $start == $end) {
                                fail!(ErrorType::ExpectedArrayContent);
                            }
                        };
                    }

                    // Handle all rows except the last one
                    for _ in 0..(rows_count - 1) {
                        let row_key_start = idx;
                        let row_key_end = get_value_end!(ErrorType::Syntax, b':');

                        if unlikely!(c != b':') {
                            fail!(ErrorType::Syntax);
                        }

                        cnt += 1;
                        insert_str!(row_key_start, row_key_end);

                        // Open the row's object
                        open_scope!(Object, parent: frame!(Object), indent: curr_indent!());

                        update_char!(); // skip ':' to reach the first field value

                        for &(h_start, h_end) in headers.iter().take(n_headers - 1) {
                            insert_str!(h_start, h_end);
                            cnt += 1;

                            let value_start = idx;
                            let value_end = get_value_end!(ErrorType::Syntax, delimiter);
                            insert_inferred_value!(value_start, value_end);

                            if unlikely!(c != delimiter) {
                                fail!(ErrorType::Syntax);
                            }
                            update_char!();
                        }

                        let (h_start, h_end) = match headers.last() {
                            Some(v) => v,
                            None => {
                                fail!();
                            }
                        };

                        insert_str!(*h_start, *h_end);
                        cnt += 1;

                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                        reject_cell_less_row!(value_start, value_end);
                        insert_inferred_value!(value_start, value_end);

                        close_and_pop_state!(Object);

                        match get_eol_state!(true) {
                            EOLState::Sibling => {}
                            // rows must stay at the same indentation
                            EOLState::CloseScope | EOLState::Nested => {
                                fail!(ErrorType::Syntax);
                            }
                        }
                    }

                    // Handle the final row separately
                    let row_key_start = idx;
                    let row_key_end = get_value_end!(ErrorType::Syntax, b':');

                    if unlikely!(c != b':') {
                        fail!(ErrorType::Syntax);
                    }

                    cnt += 1;
                    insert_str!(row_key_start, row_key_end);

                    open_scope!(Object, parent: frame!(Object), indent: curr_indent!());

                    update_char!();

                    for &(h_start, h_end) in headers.iter().take(n_headers - 1) {
                        insert_str!(h_start, h_end);
                        cnt += 1;

                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, delimiter);
                        insert_inferred_value!(value_start, value_end);

                        if unlikely!(c != delimiter) {
                            fail!(ErrorType::Syntax);
                        }
                        update_char!();
                    }

                    let (h_start, h_end) = match headers.last() {
                        Some(v) => v,
                        None => {
                            fail!();
                        }
                    };

                    insert_str!(*h_start, *h_end);
                    cnt += 1;

                    let value_start = idx;
                    let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                    reject_cell_less_row!(value_start, value_end);
                    insert_inferred_value!(value_start, value_end);

                    close_and_pop_state!(Object);

                    goto!(State::ScopeEnd);
                }

                State::ParseTabularArrayLenient {
                    key,
                    headers,
                    delimiter,
                    is_root,
                } => {
                    // In lenient mode, we can't trust rows_count to be correct.
                    // So we can't use it to preallocate memory accurately.
                    // Also looping through rows will be inefficient.

                    // There are 2 * number_of_headers * rows_count structurals in the body of a tabular
                    // array but the number of tape slots it produces is
                    // (2 * number_of_headers + 1) * rows_count
                    // so the difference is: rows_count
                    // We only reallocate memory when we run out of slack.

                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    update_char!(); // step past the header's ':'
                    if !matches!(get_eol_state!(), EOLState::Nested) {
                        fail!(ErrorType::ExpectedArrayContent);
                    }

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Array, parent: frame!(keyed key), indent: curr_indent!() + indent_size);
                    }

                    let n_headers = headers.len();

                    loop {
                        if tape_slack <= 1 {
                            // Each row produces a deficit of 1 tape slot
                            // Since we can't lean on rows_count to determine how much
                            // memory we need, we just grow it by the default amount of 20% structural characters.
                            grow_res!();
                        }

                        cnt += 1;

                        open_scope!(Object, parent: frame!(Array), indent: curr_indent!());

                        // Handle n - 1 headers:
                        for &(h_start, h_end) in headers.iter().take(n_headers - 1) {
                            insert_str!(h_start, h_end);
                            cnt += 1;

                            let value_start = idx;
                            let value_end = get_value_end!(ErrorType::Syntax, delimiter);
                            insert_inferred_value!(value_start, value_end);
                            update_char!();
                        }

                        // handle the last header:
                        let (h_start, h_end) = match headers.last() {
                            Some(v) => v,
                            None => {
                                fail!();
                            }
                        };

                        insert_str!(*h_start, *h_end);
                        cnt += 1;

                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                        insert_inferred_value!(value_start, value_end);

                        close_and_pop_state!(Object);

                        match get_eol_state!(true) {
                            EOLState::Sibling => {}
                            // rows must stay at the same indentation
                            EOLState::Nested => {
                                fail!(ErrorType::Syntax);
                            }
                            EOLState::CloseScope => {
                                break;
                            }
                        }
                    }

                    goto!(State::ScopeEnd);
                }

                State::ParseTabularArrayStrict {
                    key,
                    headers,
                    rows_count,
                    delimiter,
                    is_root,
                } => {
                    // There are 2 * number_of_headers * rows_count structurals in the body of a tabular
                    // array but the number of tape slots it produces is
                    // (2 * number_of_headers + 1) * rows_count
                    // so the difference is: rows_count
                    // We only reallocate memory when we run out of slack.
                    if rows_count > tape_slack {
                        grow_res!(rows_count - tape_slack);
                    }

                    tape_slack -= rows_count;

                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    update_char!(); // step past the header's ':'
                    if !matches!(get_eol_state!(), EOLState::Nested) {
                        fail!(ErrorType::ExpectedArrayContent);
                    }

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Array, parent: frame!(keyed key), indent: curr_indent!() + indent_size);
                    }

                    let n_headers = headers.len();

                    // Handle all rows except the last one:
                    for _ in 0..(rows_count - 1) {
                        cnt += 1;

                        open_scope!(Object, parent: frame!(Array), indent: curr_indent!());

                        // Handle n - 1 headers:
                        for &(h_start, h_end) in headers.iter().take(n_headers - 1) {
                            insert_str!(h_start, h_end);
                            cnt += 1;

                            let value_start = idx;
                            let value_end = get_value_end!(ErrorType::Syntax, delimiter);
                            insert_inferred_value!(value_start, value_end);
                            update_char!();
                        }

                        // handle the last header:
                        let (h_start, h_end) = match headers.last() {
                            Some(v) => v,
                            None => {
                                fail!();
                            }
                        };

                        insert_str!(*h_start, *h_end);
                        cnt += 1;

                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                        insert_inferred_value!(value_start, value_end);

                        close_and_pop_state!(Object);

                        match get_eol_state!(true) {
                            EOLState::Sibling => {}
                            // rows must stay at the same indentation
                            EOLState::CloseScope | EOLState::Nested => {
                                fail!(ErrorType::Syntax);
                            }
                        }
                    }

                    // Handle the final row separately:
                    cnt += 1;
                    open_scope!(Object, parent: frame!(Array), indent: curr_indent!());

                    for &(h_start, h_end) in headers.iter().take(n_headers - 1) {
                        insert_str!(h_start, h_end);
                        cnt += 1;

                        let value_start = idx;
                        let value_end = get_value_end!(ErrorType::Syntax, delimiter);
                        insert_inferred_value!(value_start, value_end);
                        update_char!();
                    }

                    let (h_start, h_end) = match headers.last() {
                        Some(v) => v,
                        None => {
                            fail!();
                        }
                    };

                    insert_str!(*h_start, *h_end);
                    cnt += 1;

                    let value_start = idx;
                    let value_end = get_value_end!(ErrorType::Syntax, b'\n');
                    insert_inferred_value!(value_start, value_end);

                    close_and_pop_state!(Object);

                    goto!(State::ScopeEnd);
                }

                State::ParseNestedFieldGroupsArrayStrict {
                    key,
                    nested_fields:
                        NestedFields {
                            ref field_entries,
                            leaf_count,
                            nested_count,
                        },
                    rows_count,
                    delimiter,
                    is_root,
                } => {
                    // There are  2 * leaf_count * rows_count
                    // structurals in the body of a nested group field
                    // but the number of tape slots it produces is
                    // (1 + 2 * leaf_count + 2 * nested_count) * rows_count
                    // so the difference is: (1 + 2 * nested_count) * rows_count
                    // We only reallocate memory when we run out of slack.
                    let total_deficit = rows_count * (1 + 2 * nested_count);

                    if total_deficit > tape_slack {
                        grow_res!(total_deficit - tape_slack);
                    }

                    tape_slack -= total_deficit;

                    if let Some((key_start, key_end)) = key {
                        cnt += 1;
                        insert_str!(key_start, key_end);
                    }

                    update_char!(); // step past the header's ':'
                    if !matches!(get_eol_state!(), EOLState::Nested) {
                        fail!(ErrorType::ExpectedArrayContent);
                    }

                    // We can't use recursion so we use a stack instead.
                    enum WorkItem<'a> {
                        Process(&'a FieldEntry),
                        CloseScope,
                    }

                    let mut entry_stack: Vec<WorkItem> = Vec::new();

                    if unlikely!(is_root) {
                        content_ws_stack.push(curr_indent!() + indent_size);
                    } else {
                        open_scope!(Array, parent: frame!(keyed key), indent: curr_indent!() + indent_size);
                    }

                    for _ in 0..(rows_count - 1) {
                        cnt += 1;
                        open_scope!(Object, parent: frame!(Array), indent: curr_indent!());

                        let mut curr_leaf_count = 0;

                        for field_entry in field_entries {
                            entry_stack.push(WorkItem::Process(field_entry));

                            while let Some(work_item) = entry_stack.pop() {
                                match work_item {
                                    WorkItem::Process(current_entry) => match current_entry {
                                        FieldEntry::Leaf((h_start, h_end)) => {
                                            insert_str!(*h_start, *h_end);
                                            cnt += 1;
                                            curr_leaf_count += 1;

                                            if unlikely!(curr_leaf_count == leaf_count) {
                                                let value_start = idx;
                                                let value_end =
                                                    get_value_end!(ErrorType::Syntax, b'\n');
                                                insert_inferred_value!(value_start, value_end);
                                            } else {
                                                let value_start = idx;
                                                let value_end =
                                                    get_value_end!(ErrorType::Syntax, delimiter);
                                                insert_inferred_value!(value_start, value_end);
                                                update_char!();
                                            }
                                        }

                                        FieldEntry::Nested {
                                            name: (h_start, h_end),
                                            children,
                                        } => {
                                            insert_str!(*h_start, *h_end);
                                            cnt += 1;

                                            open_scope!(Object, parent: frame!(Object), indent: curr_indent!());

                                            entry_stack.push(WorkItem::CloseScope);

                                            for child in children.iter().rev() {
                                                entry_stack.push(WorkItem::Process(child));
                                            }
                                        }
                                    },

                                    WorkItem::CloseScope => {
                                        close_and_pop_state!(Object);
                                    }
                                }
                            }
                        }

                        close_and_pop_state!(Object);

                        match get_eol_state!(true) {
                            EOLState::Sibling => {}
                            // rows must stay at the same indentation
                            EOLState::CloseScope | EOLState::Nested => {
                                fail!(ErrorType::Syntax);
                            }
                        }
                    }

                    cnt += 1;
                    open_scope!(Object, parent: frame!(Array), indent: curr_indent!());

                    let mut curr_leaf_count = 0;

                    for field_entry in field_entries {
                        entry_stack.push(WorkItem::Process(field_entry));

                        while let Some(work_item) = entry_stack.pop() {
                            match work_item {
                                WorkItem::Process(current_entry) => match current_entry {
                                    FieldEntry::Leaf((h_start, h_end)) => {
                                        insert_str!(*h_start, *h_end);
                                        cnt += 1;
                                        curr_leaf_count += 1;

                                        if unlikely!(curr_leaf_count == leaf_count) {
                                            let value_start = idx;
                                            let value_end =
                                                get_value_end!(ErrorType::Syntax, b'\n');
                                            insert_inferred_value!(value_start, value_end);
                                        } else {
                                            let value_start = idx;
                                            let value_end =
                                                get_value_end!(ErrorType::Syntax, delimiter);
                                            insert_inferred_value!(value_start, value_end);
                                            update_char!();
                                        }
                                    }

                                    FieldEntry::Nested {
                                        name: (h_start, h_end),
                                        children,
                                    } => {
                                        insert_str!(*h_start, *h_end);
                                        cnt += 1;

                                        open_scope!(Object, parent: frame!(Object), indent: curr_indent!());

                                        entry_stack.push(WorkItem::CloseScope);

                                        for child in children.iter().rev() {
                                            entry_stack.push(WorkItem::Process(child));
                                        }
                                    }
                                },

                                WorkItem::CloseScope => {
                                    close_and_pop_state!(Object);
                                }
                            }
                        }
                    }

                    close_and_pop_state!(Object);

                    goto!(State::ScopeEnd);
                }

                State::ScopeEnd => {
                    if unlikely!(depth == 0) {
                        fail!(ErrorType::Syntax);
                    }

                    if strict
                        && let Some(&(d, expected)) = pending_counts.last()
                        && d == depth
                    {
                        pending_counts.pop();
                        if unlikely!(cnt != expected) {
                            fail!(ErrorType::Syntax); // fewer items than declared (Block array length mismatch)
                        }
                    }

                    depth -= 1;
                    content_ws_stack.pop();

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
                            _ => {
                                fail!();
                            }
                        }

                        // Update the stack state. The tag records what kind of container
                        // we're returning to, so `Sibling` knows whether to expect another
                        // object key (`ParseHeader`) or another block-array item (`-`).
                        match *stack_ptr.add(depth) {
                            StackState::Object {
                                last_start: l,
                                cnt: parent_cnt,
                            } => {
                                last_start = l;
                                cnt = parent_cnt;

                                // `c == b'\n'` means this newline hasn't been consumed yet
                                // (fresh close, e.g. right after a tabular block). Otherwise
                                // we're cascading through several closes for a newline that
                                // `get_eol_state!` already consumed, so reuse `last_dedent_ws`
                                // (the indentation it measured) instead of expecting another
                                // (nonexistent) `\n`.
                                let eol_state = if c == b'\n' {
                                    get_eol_state!()
                                } else {
                                    eol_state_from_ws!(last_dedent_ws)
                                };

                                match eol_state {
                                    EOLState::CloseScope => {
                                        goto!(State::ScopeEnd);
                                    }

                                    EOLState::Nested => {
                                        fail!(ErrorType::NoStructure);
                                    }

                                    EOLState::Sibling => {
                                        goto!(State::ParseHeader);
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
                                    eol_state_from_ws!(last_dedent_ws)
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
                                while i < structural_indexes.len() && c == b'\n' {
                                    update_char!();
                                }
                                if i == structural_indexes.len() {
                                    success!();
                                }
                                fail!();
                            }
                        }
                    }
                }
            }
        }
    }
}
