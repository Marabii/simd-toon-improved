use crate::{Error, ErrorType, Result};

/// The indentation width, in spaces, a decoder assumes when none is given.
pub const DEFAULT_INDENT_SIZE: usize = 2;

/// Options controlling how a TOON document is decoded.
///
/// The default is what the spec asks of a conforming decoder: strict
/// validation with two space indentation.
///
/// ```rust
/// use simd_json::DecodeOptions;
///
/// let strict = DecodeOptions::new();
/// assert!(strict.strict());
/// assert_eq!(strict.indent_size(), 2);
///
/// let lenient = DecodeOptions::new().with_strict(false).with_indent_size(4)?;
/// assert!(!lenient.strict());
/// assert_eq!(lenient.indent_size(), 4);
/// # Ok::<(), simd_json::Error>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DecodeOptions {
    strict: bool,
    indent_size: usize,
}

impl Default for DecodeOptions {
    #[cfg_attr(not(feature = "no-inline"), inline)]
    fn default() -> Self {
        Self::new()
    }
}

impl DecodeOptions {
    /// Spec defaults: strict validation, two space indentation.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn new() -> Self {
        Self {
            strict: true,
            indent_size: DEFAULT_INDENT_SIZE,
        }
    }

    /// Shorthand for `DecodeOptions::new().with_strict(false)`.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn lenient() -> Self {
        Self::new().with_strict(false)
    }

    /// Sets whether the document is decoded strictly.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Sets the indentation width in spaces.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `indent_size` is `0`, which would make every
    /// nesting level indistinguishable from its parent.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn with_indent_size(mut self, indent_size: usize) -> Result<Self> {
        self.set_indent_size(indent_size)?;
        Ok(self)
    }

    /// Sets whether the document is decoded strictly, in place.
    ///
    /// See [`with_strict`](Self::with_strict).
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    /// Sets the indentation width in spaces, in place.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `indent_size` is `0`.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub fn set_indent_size(&mut self, indent_size: usize) -> Result<()> {
        if indent_size == 0 {
            return Err(Error::generic(ErrorType::InvalidIndentSize));
        }
        self.indent_size = indent_size;
        Ok(())
    }

    /// Whether the document is decoded strictly.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn strict(self) -> bool {
        self.strict
    }

    /// The indentation width in spaces, always at least `1`.
    #[must_use]
    #[cfg_attr(not(feature = "no-inline"), inline)]
    pub const fn indent_size(self) -> usize {
        self.indent_size
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]
    use super::{DEFAULT_INDENT_SIZE, DecodeOptions};
    use crate::ErrorType;

    #[test]
    fn defaults_are_spec_defaults() {
        let options = DecodeOptions::default();
        assert!(options.strict());
        assert_eq!(options.indent_size(), DEFAULT_INDENT_SIZE);
        assert_eq!(options, DecodeOptions::new());
    }

    #[test]
    fn builders_and_setters_agree() {
        let built = DecodeOptions::new().with_strict(false).with_indent_size(4);
        let built = built.unwrap();

        let mut set = DecodeOptions::new();
        set.set_strict(false);
        set.set_indent_size(4).unwrap();

        assert_eq!(built, set);
        assert_eq!(built, DecodeOptions::lenient().with_indent_size(4).unwrap());
    }

    #[test]
    fn zero_indent_size_is_rejected() {
        let err = DecodeOptions::new().with_indent_size(0).err().unwrap();
        assert_eq!(*err.error(), ErrorType::InvalidIndentSize);

        let mut options = DecodeOptions::new();
        assert!(options.set_indent_size(0).is_err());
        // a rejected value leaves the options untouched
        assert_eq!(options.indent_size(), DEFAULT_INDENT_SIZE);
    }
}
