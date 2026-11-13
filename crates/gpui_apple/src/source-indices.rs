pub(crate) enum SourceIndices {
    Ascii(usize),
    Utf16(Vec<Option<usize>>),
}

impl SourceIndices {
    pub(crate) fn new(text: &str) -> Self {
        if text.is_ascii() {
            return Self::Ascii(text.len());
        }
        let mut boundaries = vec![None; text.encode_utf16().count() + 1];
        let mut offset = 0;
        for (index, character) in text.char_indices() {
            boundaries[offset] = Some(index);
            offset += character.len_utf16();
        }
        boundaries[offset] = Some(text.len());
        Self::Utf16(boundaries)
    }

    pub(crate) fn get(&self, index: usize) -> Option<usize> {
        match self {
            Self::Ascii(length) => (index <= *length).then_some(index),
            Self::Utf16(boundaries) => boundaries.get(index).copied().flatten(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SourceIndices;

    #[test]
    fn ascii_offsets_include_the_terminal_boundary_and_reject_overflow() {
        let indices = SourceIndices::new("a\r\nb\t");
        for index in 0..=5 {
            assert_eq!(indices.get(index), Some(index));
        }
        assert_eq!(indices.get(6), None);
        assert_eq!(indices.get(usize::MAX), None);
    }

    #[test]
    fn empty_source_has_one_boundary() {
        let indices = SourceIndices::new("");
        assert_eq!(indices.get(0), Some(0));
        assert_eq!(indices.get(1), None);
    }

    #[test]
    fn unicode_offsets_preserve_scalars_and_reject_split_surrogates() {
        let indices = SourceIndices::new("a😀源é");
        for (index, expected) in [Some(0), Some(1), None, Some(5), Some(8), Some(10)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(indices.get(index), expected);
        }
        assert_eq!(indices.get(6), None);
        assert_eq!(indices.get(usize::MAX), None);
    }

    #[test]
    fn combining_and_direction_marks_remain_distinct_scalar_boundaries() {
        let indices = SourceIndices::new("a\u{301}\u{200f}b");
        for (index, expected) in [0, 1, 3, 6, 7].into_iter().enumerate() {
            assert_eq!(indices.get(index), Some(expected));
        }
    }
}
