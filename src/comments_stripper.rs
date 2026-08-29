use crate::SIMDINPUT_LENGTH;

/// As defined by the TOON spec sheet for comments:
/// `A comment line is a line whose first character after zero or more leading spaces (U+0020) is "#" (U+0023)`
/// removing comment lines starts with detecting all hashes from stage 1
/// then figuring out which are the start of real comment lines by keeping only those
/// that have only 0 or more whitespaces after the preceding new line.
/// for TOON documents that start with comments, that case is handled separately
/// at the start of stage 2.
/// The goal of CommentStripper is to make the following transformation:
///
/// ```text
/// name: Hamza\n  # note\nage: 21
/// name: Hamza          \nage: 21
/// ```
/// Note how the newline character at the end of `Hamza` was replaced by whitespace,
/// that's the trick that makes this work.
/// Stage 2 has a macro for trimming whitespaces at the end, so
/// ```
/// Hamza\n  # note\n
/// ```
/// becomes just `Hamza`.
pub struct CommentStripper {
    base: *mut u8,
    len: usize,
    /// This helps us manage cases where the previous block entered in a \n and possibly a few
    /// whitespaces then the next block came in, it must know that the previous iteration
    /// is already inside the indentation and to just keep searching for `#`.
    inside_indent: u64,
    /// Does the previous block end inside a `#` comment?
    inside_comment: u64,
}

impl CommentStripper {
    pub fn new(base: *mut u8, len: usize) -> Self {
        Self {
            base,
            len,
            inside_indent: 1,
            inside_comment: 0,
        }
    }

    /// Blanks every comment byte in one 64-byte block and returns their mask;
    #[cfg_attr(not(feature = "no-inline"), inline)]
    #[allow(clippy::cast_possible_truncation)]
    pub unsafe fn strip_block(
        &mut self,
        idx: usize,
        newlines: u64,
        spaces: u64,
        hashes: u64,
        structural_indexes: &mut Vec<u32>,
    ) -> u64 {
        let line_starts = (newlines << 1) | self.inside_indent;
        let (indent_sum, indent_spills) = spaces.overflowing_add(line_starts);
        // An indentation run that reaches past byte 63 continues into the next
        // block; so does a '\n' on byte 63, whose line starts there.
        self.inside_indent = u64::from(indent_spills) | (newlines >> 63);

        // A comment opens where that first non-space byte is a '#'.
        let comment_starts = indent_sum & !spaces & hashes;
        let starts = comment_starts | self.inside_comment;
        if starts == 0 {
            return 0;
        }

        let not_newlines = !newlines;
        let (body_sum, body_spills) = not_newlines.overflowing_add(starts);
        self.inside_comment = u64::from(body_spills);

        let mut blank = (not_newlines ^ body_sum) & not_newlines;

        for opener in BitIter(comment_starts) {
            unsafe {
                blank |= self.swallow_preceding_newline(idx, opener, newlines, structural_indexes);
            }
        }

        unsafe { self.blank_bytes(idx, blank) };
        blank
    }

    #[cfg_attr(not(feature = "no-inline"), inline)]
    #[allow(clippy::cast_possible_truncation)]
    unsafe fn swallow_preceding_newline(
        &self,
        idx: usize,
        opener: usize,
        newlines: u64,
        structural_indexes: &mut Vec<u32>,
    ) -> u64 {
        let mask = (1_u64 << opener).wrapping_sub(1);
        let prev_newlines = newlines & mask;

        // Fast path: the preceding newline is within the current 64-byte block
        if prev_newlines != 0 {
            let bit = 63 - prev_newlines.leading_zeros() as usize;
            return 1_u64 << bit;
        }

        // the newline is in a previous block
        let p = idx + opener;

        let newline = p - 1;
        if structural_indexes.last() == Some(&(newline as u32)) {
            structural_indexes.pop();
            unsafe { *self.base.add(newline) = b' ' };
        }
        0
    }

    /// Overwrites the blanked bytes, one run of set bits at a time. The tail
    /// block reaches past the input, so writes are clamped to `len`.
    #[cfg_attr(not(feature = "no-inline"), inline)]
    unsafe fn blank_bytes(&self, idx: usize, blank: u64) {
        let mut todo = blank;
        while todo != 0 {
            let start = todo.trailing_zeros() as usize;
            let run = (!(todo >> start)).trailing_zeros() as usize;
            let end = (start + run).min(SIMDINPUT_LENGTH);
            let from = idx + start;
            let count = (idx + end).min(self.len).saturating_sub(from);
            if count != 0 {
                unsafe { std::ptr::write_bytes(self.base.add(from), b' ', count) };
            }
            todo = if end == SIMDINPUT_LENGTH {
                0
            } else {
                todo >> end << end
            };
        }
    }
}

/// Yields the index of each set bit, lowest first.
struct BitIter(u64);

impl Iterator for BitIter {
    type Item = usize;

    #[cfg_attr(not(feature = "no-inline"), inline)]
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        let bit = self.0.trailing_zeros() as usize;
        self.0 &= self.0.wrapping_sub(1);
        Some(bit)
    }
}
