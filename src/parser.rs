//! A reusable parser handle that owns both the decode options and the scratch
//! buffers.
//!
//! Every parse needs two things that are orthogonal to the input: how to decode
//! it ([`DecodeOptions`]) and somewhere to scratch ([`Buffers`]). Keeping both
//! in one handle avoids a `*_with_buffers_and_options` variant of every free
//! function, and lets a caller that parses many documents configure once:
//!
//! ```rust
//! use simd_json::{DecodeOptions, Parser};
//! # use simd_json::prelude::*;
//!
//! let mut parser = Parser::with_options(DecodeOptions::lenient());
//!
//! let mut input = b"name: Ada".to_vec();
//! let value = parser.parse_to_owned_value(&mut input)?;
//! assert_eq!(value.get_str("name"), Some("Ada"));
//! # Ok::<(), simd_json::Error>(())
//! ```

use crate::value::borrowed::BorrowDeserializer;
use crate::value::owned::OwnedDeserializer;
use crate::{BorrowedValue, Buffers, DecodeOptions, Deserializer, OwnedValue, Result, Tape};
use std::fmt;

#[cfg(feature = "serde_impl")]
use crate::{Error, ErrorType};
#[cfg(feature = "serde_impl")]
use serde::de::DeserializeOwned;
#[cfg(feature = "serde_impl")]
use serde_ext::Deserialize;
#[cfg(feature = "serde_impl")]
use std::io;

/// A parser that carries its [`DecodeOptions`] and reuses its [`Buffers`]
/// across documents.
///
/// The free functions (`simd_json::to_tape`, `simd_json::from_slice`, ...) are
/// this with default options and a fresh set of buffers per call.
pub struct Parser {
    options: DecodeOptions,
    buffers: Buffers,
}

impl Default for Parser {
    #[cfg_attr(not(feature = "no-inline"), inline)]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(tarpaulin_include))]
impl fmt::Debug for Parser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Buffers` is scratch space, there is nothing useful to print about it
        f.debug_struct("Parser")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Parser {
    /// Creates a parser with the default decode options (strict, two space
    /// indentation).
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn new() -> Self {
        Self::with_options(DecodeOptions::new())
    }

    /// Creates a parser with the given decode options.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn with_options(options: DecodeOptions) -> Self {
        Self {
            options,
            buffers: Buffers::default(),
        }
    }

    /// Creates a parser whose buffers are pre-sized for inputs of roughly
    /// `capacity` bytes, with the default decode options.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_and_options(capacity, DecodeOptions::new())
    }

    /// Creates a parser whose buffers are pre-sized for inputs of roughly
    /// `capacity` bytes, with the given decode options.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn with_capacity_and_options(capacity: usize, options: DecodeOptions) -> Self {
        Self {
            options,
            buffers: Buffers::new(capacity),
        }
    }

    /// The options this parser decodes with.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn options(&self) -> DecodeOptions {
        self.options
    }

    /// Mutable access to the options, for tweaking them between documents:
    ///
    /// ```rust
    /// use simd_json::Parser;
    ///
    /// let mut parser = Parser::new();
    /// parser.options_mut().set_strict(false);
    /// assert!(!parser.options().strict());
    /// ```
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn options_mut(&mut self) -> &mut DecodeOptions {
        &mut self.options
    }

    /// Replaces the options this parser decodes with.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn set_options(&mut self, options: DecodeOptions) {
        self.options = options;
    }

    /// The buffers this parser reuses, e.g. to inspect
    /// [`structural_indexes`](Buffers::structural_indexes) after a parse.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn buffers(&self) -> &Buffers {
        &self.buffers
    }

    /// Mutable access to the buffers this parser reuses.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn buffers_mut(&mut self) -> &mut Buffers {
        &mut self.buffers
    }

    /// Creates a [`Deserializer`] from the input, note that the input will be
    /// rewritten in the process.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `input` is invalid TOON.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn deserializer<'de>(&mut self, input: &'de mut [u8]) -> Result<Deserializer<'de>> {
        Deserializer::from_slice_with_buffers_and_options(input, &mut self.buffers, self.options)
    }

    /// Parses the input into a tape for later consumption, note that the input
    /// will be rewritten in the process.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `input` is invalid TOON.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn parse_to_tape<'de>(&mut self, input: &'de mut [u8]) -> Result<Tape<'de>> {
        self.deserializer(input).map(Deserializer::into_tape)
    }

    /// Fills an already existing tape from the input, note that the input will
    /// be rewritten in the process.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `input` is invalid TOON.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn fill_tape<'de>(&mut self, input: &'de mut [u8], tape: &mut Tape<'de>) -> Result<()> {
        tape.0.clear();
        Deserializer::fill_tape(input, &mut self.buffers, &mut tape.0, self.options)
    }

    /// Parses the input into a borrowed DOM value, note that the input will be
    /// rewritten in the process.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `input` is invalid TOON.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn parse_to_borrowed_value<'de>(
        &mut self,
        input: &'de mut [u8],
    ) -> Result<BorrowedValue<'de>> {
        let de = self.deserializer(input)?;
        Ok(BorrowDeserializer::from_deserializer(de).parse())
    }

    /// Parses the input into an owned DOM value, note that the input will be
    /// rewritten in the process.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `input` is invalid TOON.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn parse_to_owned_value(&mut self, input: &mut [u8]) -> Result<OwnedValue> {
        let de = self.deserializer(input)?;
        Ok(OwnedDeserializer::from_deserializer(de).parse())
    }

    /// Parses the input with serde, note that the input will be rewritten in
    /// the process.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `input` is invalid TOON or does not match `T`.
    #[cfg(feature = "serde_impl")]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn parse<'de, T>(&mut self, input: &'de mut [u8]) -> Result<T>
    where
        T: Deserialize<'de>,
    {
        let mut deserializer = self.deserializer(input)?;
        T::deserialize(&mut deserializer)
    }

    /// Parses everything a reader yields with serde.
    ///
    /// # Warning
    ///
    /// Since simd-json does not support streaming and requires mutability of
    /// the data, this reads the entire reader into memory before parsing it.
    ///
    /// # Errors
    ///
    /// Will return `Err` if an IO error is encountered while reading `rdr`, or
    /// if its content is invalid TOON or does not match `T`.
    #[cfg(feature = "serde_impl")]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn parse_reader<R, T>(&mut self, mut rdr: R) -> Result<T>
    where
        R: io::Read,
        T: DeserializeOwned,
    {
        let mut data = Vec::new();
        if let Err(e) = rdr.read_to_end(&mut data) {
            return Err(Error::generic(ErrorType::Io(e)));
        }
        let mut deserializer = self.deserializer(&mut data)?;
        T::deserialize(&mut deserializer)
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use super::Parser;
    use crate::DecodeOptions;
    use crate::prelude::*;

    #[test]
    fn options_travel_with_the_parser() {
        let mut parser = Parser::with_options(DecodeOptions::lenient());
        assert!(!parser.options().strict());

        parser.set_options(DecodeOptions::new());
        assert!(parser.options().strict());

        parser.options_mut().set_indent_size(4).unwrap();
        assert_eq!(parser.options().indent_size(), 4);
    }

    #[test]
    fn buffers_are_reused_across_documents() {
        let mut parser = Parser::new();

        let mut first = b"name: Ada".to_vec();
        let value = parser.parse_to_owned_value(&mut first).unwrap();
        assert_eq!(value.get_str("name"), Some("Ada"));

        let mut second = b"name: Bob".to_vec();
        let value = parser.parse_to_owned_value(&mut second).unwrap();
        assert_eq!(value.get_str("name"), Some("Bob"));
    }

    #[test]
    fn every_entry_point_agrees() {
        let mut parser = Parser::new();

        let mut input = b"name: Ada".to_vec();
        let borrowed = parser.parse_to_borrowed_value(&mut input).unwrap();
        assert_eq!(borrowed.get_str("name"), Some("Ada"));

        let mut input = b"name: Ada".to_vec();
        let tape = parser.parse_to_tape(&mut input).unwrap();
        assert_eq!(tape.as_value().get_str("name"), Some("Ada"));

        let mut input = b"name: Ada".to_vec();
        let mut tape = crate::Tape::null();
        parser.fill_tape(&mut input, &mut tape).unwrap();
        assert_eq!(tape.as_value().get_str("name"), Some("Ada"));
    }

    /// Every option carrying entry point reaches the parser with the options
    /// it was handed - `indent_size` is the one the parser will read first, so
    /// it doubles as the canary that nothing drops them on the way down.
    #[test]
    fn free_functions_take_options() {
        let options = DecodeOptions::lenient().with_indent_size(4).unwrap();

        let mut input = b"name: Ada".to_vec();
        let tape = crate::to_tape_with_options(&mut input, options).unwrap();
        assert_eq!(tape.as_value().get_str("name"), Some("Ada"));

        let mut input = b"name: Ada".to_vec();
        let value = crate::to_borrowed_value_with_options(&mut input, options).unwrap();
        assert_eq!(value.get_str("name"), Some("Ada"));

        let mut input = b"name: Ada".to_vec();
        let value = crate::to_owned_value_with_options(&mut input, options).unwrap();
        assert_eq!(value.get_str("name"), Some("Ada"));

        let mut input = b"name: Ada".to_vec();
        let mut buffers = crate::Buffers::default();
        let de = crate::Deserializer::from_slice_with_buffers_and_options(
            &mut input,
            &mut buffers,
            options,
        )
        .unwrap();
        assert_eq!(de.into_tape().as_value().get_str("name"), Some("Ada"));
    }

    #[cfg(feature = "serde_impl")]
    #[test]
    fn serde_free_functions_take_options() {
        let options = DecodeOptions::lenient();

        let mut input = b"name: Ada".to_vec();
        let value: serde_json::Value = crate::from_slice_with_options(&mut input, options).unwrap();
        assert_eq!(value["name"], "Ada");

        let mut input = String::from("name: Ada");
        let value: serde_json::Value =
            unsafe { crate::from_str_with_options(&mut input, options) }.unwrap();
        assert_eq!(value["name"], "Ada");

        let value: serde_json::Value =
            crate::from_reader_with_options(&b"name: Ada"[..], options).unwrap();
        assert_eq!(value["name"], "Ada");
    }

    #[cfg(feature = "serde_impl")]
    #[test]
    fn serde_entry_points() {
        let mut parser = Parser::new();

        let mut input = b"name: Ada".to_vec();
        let value: serde_json::Value = parser.parse(&mut input).unwrap();
        assert_eq!(value["name"], "Ada");

        let value: serde_json::Value = parser.parse_reader(&b"name: Ada"[..]).unwrap();
        assert_eq!(value["name"], "Ada");
    }
}
