use crate::{Bounds, FontId, FontRun, Pixels, ShapedGlyph, px};
use anyhow::{Context as _, ensure};
use std::{borrow::Cow, ops::Range};

/// Requested paragraph base direction, including native automatic resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParagraphDirection {
    /// Delegate paragraph base direction to the platform text system.
    Natural,
    /// Left-to-right paragraph base.
    LeftToRight,
    /// Right-to-left paragraph base.
    RightToLeft,
}

/// A paragraph base direction known independently of visual glyph order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedParagraphDirection {
    /// Resolved left-to-right paragraph base.
    LeftToRight,
    /// Resolved right-to-left paragraph base.
    RightToLeft,
}

/// The paragraph's fitting constraint.
#[derive(Clone, Copy, Debug)]
pub enum ParagraphWidth {
    /// No horizontal fitting constraint.
    Unbounded,
    /// A finite, strictly positive fitting width.
    Bounded(Pixels),
}

/// Native paragraph line height, applied during fitting and drawing.
#[derive(Clone, Copy, Debug, Default)]
pub enum ParagraphLineHeight {
    /// Native font metrics, without a line-height constraint.
    #[default]
    Natural,
    /// Exact height with the configured fonts centered using an integral baseline offset
    /// when their ascent plus descent fits. Oversized glyphs may extend outside the row.
    /// The caller resolves any product-specific height rounding before requesting layout.
    Exact(Pixels),
    /// A minimum native height, allowing taller font content.
    AtLeast(Pixels),
}

/// Native paragraph alignment; physical left/right do not flip with base direction.
#[derive(Clone, Copy, Debug, Default)]
pub enum ParagraphAlignment {
    /// Follow the caller-resolved paragraph base direction.
    #[default]
    Natural,
    /// Align to the physical left edge.
    Left,
    /// Center within the fitting width.
    Center,
    /// Align to the physical right edge.
    Right,
    /// Native justification, retaining native last-row and whitespace behavior.
    Justified,
}

/// Which consumed rows need insertion positions from the native layout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ParagraphCaretCoverage {
    /// Return insertion positions for every row.
    #[default]
    All,
    /// Fit and draw without extracting insertion positions.
    None,
    /// Return positions for rows touching this scalar-aligned UTF-8 range.
    /// A collapsed range includes both rows at a soft-wrap boundary.
    Source(Range<usize>),
}

impl ParagraphCaretCoverage {
    /// Includes both sides of a shared insertion boundary, including the terminal row.
    pub fn covers(&self, source: &Range<usize>) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Source(wanted) => source.start <= wanted.end && wanted.start <= source.end,
        }
    }
}

/// A validated contextual layout request borrowing source and borrowing or owning resolved font runs.
#[derive(Clone, Debug)]
pub struct ParagraphRequest<'source> {
    text: &'source str,
    font_size: Pixels,
    runs: Cow<'source, [FontRun]>,
    width: ParagraphWidth,
    direction: ParagraphDirection,
    selection: Option<Range<usize>>,
    line_height: ParagraphLineHeight,
    alignment: ParagraphAlignment,
    carets: ParagraphCaretCoverage,
}

impl<'source> ParagraphRequest<'source> {
    /// Validates scalar-aligned source coverage and finite positive sizing before dispatch.
    ///
    /// # Errors
    /// Rejects invalid font size, bounded width, source coverage or UTF-8 font-run boundaries.
    pub fn try_new(
        text: &'source str,
        font_size: Pixels,
        runs: &'source [FontRun],
        width: ParagraphWidth,
        direction: ParagraphDirection,
    ) -> anyhow::Result<Self> {
        Self::try_from_runs(text, font_size, Cow::Borrowed(runs), width, direction)
    }

    pub(crate) fn validate(
        text: &str,
        font_size: Pixels,
        lengths: impl IntoIterator<Item = usize>,
        width: ParagraphWidth,
    ) -> anyhow::Result<()> {
        ensure!(
            f32::from(font_size).is_finite() && font_size > px(0.),
            "invalid paragraph font size"
        );
        if let ParagraphWidth::Bounded(width) = width {
            ensure!(
                f32::from(width).is_finite() && width > px(0.),
                "bounded paragraph width must be finite and positive"
            );
        }
        let mut consumed = 0usize;
        for len in lengths {
            let end = consumed.checked_add(len).context("font run overflow")?;
            ensure!(
                text.get(consumed..end).is_some(),
                "invalid font run boundary"
            );
            consumed = end;
        }
        ensure!(consumed == text.len(), "font runs must cover the source");
        Ok(())
    }

    pub(crate) fn try_from_runs(
        text: &'source str,
        font_size: Pixels,
        runs: Cow<'source, [FontRun]>,
        width: ParagraphWidth,
        direction: ParagraphDirection,
    ) -> anyhow::Result<Self> {
        Self::validate(text, font_size, runs.iter().map(|run| run.len), width)?;
        Ok(Self {
            text,
            font_size,
            runs,
            width,
            direction,
            selection: None,
            line_height: ParagraphLineHeight::Natural,
            alignment: ParagraphAlignment::Natural,
            carets: ParagraphCaretCoverage::All,
        })
    }

    /// Applies a native height policy before fitting, glyph placement and selection geometry.
    ///
    /// # Errors
    /// Rejects nonpositive or nonfinite exact/minimum heights.
    pub fn with_line_height(mut self, height: ParagraphLineHeight) -> anyhow::Result<Self> {
        if let ParagraphLineHeight::Exact(value) | ParagraphLineHeight::AtLeast(value) = height {
            ensure!(
                f32::from(value).is_finite() && value > px(0.),
                "invalid paragraph line height"
            );
        }
        self.line_height = height;
        Ok(self)
    }

    /// Chooses native alignment without changing the resolved paragraph base direction.
    pub fn with_alignment(mut self, alignment: ParagraphAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Native sizing policy; natural metrics are the default.
    pub fn line_height(&self) -> ParagraphLineHeight {
        self.line_height
    }

    /// Native alignment; natural alignment is the default.
    pub fn alignment(&self) -> ParagraphAlignment {
        self.alignment
    }

    /// Requests native selection rectangles from this same final layout operation.
    /// Empty ranges produce no highlight; caret coverage remains independently configured.
    ///
    /// # Errors
    /// Rejects reversed, out-of-source or non-scalar UTF-8 ranges.
    pub fn with_selection(mut self, source: Range<usize>) -> anyhow::Result<Self> {
        ensure!(
            self.text.get(source.clone()).is_some(),
            "invalid paragraph selection range"
        );
        self.selection = Some(source);
        Ok(self)
    }

    /// Chooses insertion-position coverage without changing fitting, glyphs or selection rectangles.
    ///
    /// # Errors
    /// Rejects reversed, out-of-source or non-scalar UTF-8 ranges.
    pub fn with_carets(mut self, coverage: ParagraphCaretCoverage) -> anyhow::Result<Self> {
        if let ParagraphCaretCoverage::Source(source) = &coverage {
            ensure!(
                self.text.get(source.clone()).is_some(),
                "invalid paragraph caret range"
            );
        }
        self.carets = coverage;
        Ok(self)
    }

    /// Validated requested insertion-position coverage; full coverage is the default.
    pub fn carets(&self) -> &ParagraphCaretCoverage {
        &self.carets
    }

    /// The caller's validated UTF-8 selection, if requested.
    pub fn selection(&self) -> Option<&Range<usize>> {
        self.selection.as_ref()
    }

    /// Original UTF-8 source, unchanged by layout.
    pub fn text(&self) -> &'source str {
        self.text
    }
    /// Requested point size; native run-boundary adjustments remain part of returned glyphs.
    pub fn font_size(&self) -> Pixels {
        self.font_size
    }
    /// Scalar-aligned source byte lengths and platform-resolved font identifiers.
    /// For empty source, the last supplied zero-length run selects the font;
    /// no runs selects the backend's default font at the requested size.
    pub fn runs(&self) -> &[FontRun] {
        &self.runs
    }
    /// Explicit bounded or unbounded fitting policy.
    pub fn width(&self) -> ParagraphWidth {
        self.width
    }
    /// Caller-resolved paragraph base direction.
    pub fn direction(&self) -> ParagraphDirection {
        self.direction
    }
}

/// Native contextual layout availability. Backend failures are returned separately as errors.
#[derive(Debug)]
pub enum ParagraphLayout {
    /// The backend cannot provide final contextual paragraph geometry.
    Unsupported,
    /// Final consumed ranges, glyphs and requested insertion positions from one native operation.
    LaidOut(ParagraphGeometry),
}

/// Owned plain data from one final contextual layout operation.
#[derive(Debug)]
pub struct ParagraphGeometry {
    /// Complete source coverage and the canonical terminal caret row.
    pub lines: Vec<ParagraphLine>,
    /// Native rectangles for the requested nonempty selection, in paragraph coordinates.
    /// Native order and vertical coalescing are preserved, including zero-width rectangles.
    /// No selection or a collapsed selection produces an empty vector.
    pub selection_rects: Vec<Bounds<Pixels>>,
}

/// A final contextual glyph. Positions are in paragraph coordinates, with y increasing downwards.
#[derive(Debug)]
pub struct ParagraphGlyph {
    /// Registered in the text system that performed layout.
    pub font_id: FontId,
    /// Actual font point size, including any run-boundary adjustment.
    pub font_size: Pixels,
    /// Source index is paragraph-global UTF-8; position is the native paragraph baseline.
    pub glyph: ShapedGlyph,
    /// Native affine transform in a, b, c, d, tx, ty order. The linear part
    /// transforms each glyph at its supplied drawing position; tx/ty retain
    /// native metadata and do not shift that position.
    pub text_matrix: [f32; 6],
}

/// A native insertion position in display order, including coincident control stops.
#[derive(Debug)]
pub struct ParagraphCaret {
    /// Paragraph-global UTF-8 byte offset; may equal the consumed row's end.
    pub index: usize,
    /// Horizontal position in paragraph coordinates.
    pub x: Pixels,
}

/// A logically ordered source span with the resolved native glyph embedding level.
#[derive(Debug)]
pub struct ParagraphBidiRun {
    /// Paragraph-global UTF-8 source range, including hidden controls and terminators.
    pub source: Range<usize>,
    /// Native Unicode bidirectional embedding level; even is LTR and odd is RTL.
    pub level: u8,
}

/// One consumed source range and its final native layout, without substring reshaping.
#[derive(Debug)]
pub struct ParagraphLine {
    /// Reliable base direction when the platform exposes it. Native automatic
    /// layout may leave this unknown; glyph embedding levels are not a substitute.
    pub base_direction: Option<ResolvedParagraphDirection>,
    /// Consumed UTF-8 bytes, including any line terminator. A final empty caret row uses EOF..EOF.
    pub source: Range<usize>,
    /// Native used rectangle in paragraph coordinates.
    pub bounds: Bounds<Pixels>,
    /// Full native line fragment in paragraph coordinates, including the fitting width.
    pub fragment_bounds: Bounds<Pixels>,
    /// Contiguous logical source coverage, grouped by equal native embedding level.
    /// Empty terminal caret rows have no runs.
    pub bidi_runs: Vec<ParagraphBidiRun>,
    /// Final visible drawing glyphs; ordering follows native drawing callbacks.
    pub glyphs: Vec<ParagraphGlyph>,
    /// Insertion positions when this row is covered by the request.
    /// None means unavailable, rather than a row with no insertion points.
    pub carets: Option<ParagraphCarets>,
}

/// Requested insertion positions from the same final native layout.
#[derive(Debug)]
pub struct ParagraphCarets {
    /// Primary insertion points in display order, including coincident control stops.
    pub primary: Vec<ParagraphCaret>,
    /// Alternate insertion points at bidirectional boundaries, in display order.
    pub alternate: Vec<ParagraphCaret>,
}
