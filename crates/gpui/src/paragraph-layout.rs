use crate::{Bounds, FontId, FontRun, Pixels, ShapedGlyph, Size, px};
use anyhow::{Context as _, ensure};
use std::{borrow::Cow, num::NonZeroUsize, ops::Range};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParagraphLineBreakStrategy {
    #[default]
    None,
    Standard,
    HangulWord,
    PushOut,
}

/// Replacement policy applied by the native document text container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParagraphEllipsize {
    /// Keep the end of the final visible native line.
    Head,
    /// Keep both ends of the final visible native line.
    Middle,
    /// Keep the beginning of the final visible native line.
    Tail,
    /// Omit overflowing glyphs without a replacement.
    Clip,
}

/// A document-wide maximum number of native visual lines.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphLineLimit {
    /// Positive maximum number of visual lines across all paragraph spans.
    pub max_lines: NonZeroUsize,
    /// Native replacement behavior on the final visible line.
    pub mode: ParagraphEllipsize,
}

/// A scalar-aligned complete paragraph range and its native presentation.
#[derive(Clone, Debug)]
pub struct ParagraphStyleSpan {
    /// UTF-8 paragraph range, including its terminator when present.
    pub source: Range<usize>,
    /// Native base-direction policy.
    pub direction: ParagraphDirection,
    /// Native physical alignment.
    pub alignment: ParagraphAlignment,
    /// Native line-height policy.
    pub line_height: ParagraphLineHeight,
    /// Physical leading indentation of the first line.
    pub first_line_indent: Pixels,
    /// Physical leading indentation of subsequent lines.
    pub head_indent: Pixels,
    /// Native tail indentation (nonpositive values inset from the trailing edge).
    pub tail_indent: Pixels,
    /// Native spacing before the paragraph.
    pub space_before: Pixels,
    /// Native spacing after the paragraph.
    pub space_after: Pixels,
}

/// An inline object measured by native layout and painted by the caller.
#[derive(Clone, Debug)]
pub struct ParagraphAttachment {
    /// Exactly one U+FFFC in the original UTF-8 source.
    pub source: Range<usize>,
    /// Object dimensions in points.
    pub size: Size<Pixels>,
    /// Native attachment baseline displacement.
    pub baseline_offset: Pixels,
}

/// Native bounds of a visible original-source attachment.
#[derive(Debug)]
pub struct ParagraphAttachmentGeometry {
    /// Original UTF-8 object range.
    pub source: Range<usize>,
    /// Final native drawing rectangle.
    pub bounds: Bounds<Pixels>,
}

/// Whether a drawing glyph belongs to source or the native truncation replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParagraphGlyphOrigin {
    /// Original source, addressed by the glyph's UTF-8 index.
    Source,
    /// Synthetic native replacement. Its index is a style anchor, not source text.
    Ellipsis,
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
    line_break_strategy: ParagraphLineBreakStrategy,
    carets: ParagraphCaretCoverage,
    font_sizes: Option<Vec<Pixels>>,
    baseline_offsets: Option<Vec<Pixels>>,
    paragraph_styles: Vec<ParagraphStyleSpan>,
    attachments: Vec<ParagraphAttachment>,
    line_limit: Option<ParagraphLineLimit>,
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
            line_break_strategy: ParagraphLineBreakStrategy::None,
            carets: ParagraphCaretCoverage::All,
            font_sizes: None,
            baseline_offsets: None,
            paragraph_styles: Vec::new(),
            attachments: Vec::new(),
            line_limit: None,
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

    /// Overrides each resolved font run's point size without changing source coverage.
    ///
    /// # Errors
    /// Rejects mismatched run counts and nonpositive/nonfinite sizes.
    pub fn with_font_sizes(mut self, sizes: Vec<Pixels>) -> anyhow::Result<Self> {
        ensure!(
            sizes.len() == self.runs.len(),
            "font size count differs from runs"
        );
        ensure!(
            sizes
                .iter()
                .all(|size| f32::from(*size).is_finite() && *size > px(0.)),
            "invalid run font size"
        );
        self.font_sizes = Some(sizes);
        Ok(self)
    }

    /// Raises each font run by a finite signed offset in native point coordinates.
    /// Exact-height centering, when requested, is added to this offset.
    ///
    /// # Errors
    /// Rejects mismatched run counts and nonfinite offsets.
    pub fn with_baseline_offsets(mut self, offsets: Vec<Pixels>) -> anyhow::Result<Self> {
        ensure!(
            offsets.len() == self.runs.len(),
            "baseline offset count differs from runs"
        );
        ensure!(
            offsets.iter().all(|offset| f32::from(*offset).is_finite()),
            "invalid run baseline offset"
        );
        self.baseline_offsets = Some(offsets);
        Ok(self)
    }

    /// Optional native baseline offset for every supplied run.
    pub fn baseline_offsets(&self) -> Option<&[Pixels]> {
        self.baseline_offsets.as_deref()
    }

    /// Applies ordered nonoverlapping complete-paragraph style spans.
    ///
    /// # Errors
    /// Rejects invalid source/paragraph boundaries, overlap, or nonfinite metrics.
    pub fn with_paragraph_styles(mut self, spans: Vec<ParagraphStyleSpan>) -> anyhow::Result<Self> {
        let mut end = 0;
        for span in &spans {
            ensure!(
                self.text.get(span.source.clone()).is_some() && span.source.start >= end,
                "invalid paragraph style range"
            );
            ensure!(
                span.source.start == 0 || self.text[..span.source.start].ends_with('\n'),
                "style must start at paragraph boundary"
            );
            ensure!(
                span.source.end == self.text.len() || self.text[..span.source.end].ends_with('\n'),
                "style must end at paragraph boundary"
            );
            let metrics = [
                span.first_line_indent,
                span.head_indent,
                span.tail_indent,
                span.space_before,
                span.space_after,
            ];
            ensure!(
                metrics.iter().all(|value| f32::from(*value).is_finite()),
                "nonfinite paragraph metric"
            );
            if let ParagraphLineHeight::Exact(value) | ParagraphLineHeight::AtLeast(value) =
                span.line_height
            {
                ensure!(
                    f32::from(value).is_finite() && value > px(0.),
                    "invalid span line height"
                );
            }
            end = span.source.end;
        }
        self.paragraph_styles = spans;
        Ok(self)
    }

    /// Supplies inline metrics at original-source object placeholders.
    ///
    /// # Errors
    /// Rejects overlapping objects, non-placeholder ranges and invalid dimensions.
    pub fn with_attachments(mut self, objects: Vec<ParagraphAttachment>) -> anyhow::Result<Self> {
        let mut end = 0;
        for object in &objects {
            ensure!(
                self.text.get(object.source.clone()) == Some("\u{fffc}")
                    && object.source.start >= end,
                "invalid attachment source"
            );
            let metrics = [
                object.size.width,
                object.size.height,
                object.baseline_offset,
            ];
            ensure!(
                metrics.iter().all(|value| f32::from(*value).is_finite())
                    && object.size.width >= px(0.)
                    && object.size.height >= px(0.),
                "invalid attachment metrics"
            );
            end = object.source.end;
        }
        self.attachments = objects;
        Ok(self)
    }

    /// Limits the complete native document; no limit preserves full source coverage.
    pub fn with_line_limit(mut self, limit: ParagraphLineLimit) -> Self {
        self.line_limit = Some(limit);
        self
    }

    /// Optional font size for every supplied run.
    pub fn font_sizes(&self) -> Option<&[Pixels]> {
        self.font_sizes.as_deref()
    }
    /// Ordered complete-paragraph overrides.
    pub fn paragraph_styles(&self) -> &[ParagraphStyleSpan] {
        &self.paragraph_styles
    }
    /// Original-source inline object measurements.
    pub fn attachments(&self) -> &[ParagraphAttachment] {
        &self.attachments
    }
    /// Optional document-wide native line limit.
    pub fn line_limit(&self) -> Option<ParagraphLineLimit> {
        self.line_limit
    }

    /// Chooses native alignment without changing the resolved paragraph base direction.
    pub fn with_alignment(mut self, alignment: ParagraphAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn with_line_break_strategy(mut self, strategy: ParagraphLineBreakStrategy) -> Self {
        self.line_break_strategy = strategy;
        self
    }

    pub fn line_break_strategy(&self) -> ParagraphLineBreakStrategy {
        self.line_break_strategy
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
    /// Visible object rectangles from the same native layout.
    pub attachments: Vec<ParagraphAttachmentGeometry>,
    /// True when the native container omitted original source.
    pub truncated: bool,
}

/// A final contextual glyph. Positions are in paragraph coordinates, with y increasing downwards.
#[derive(Debug)]
pub struct ParagraphGlyph {
    /// Original source or a synthetic native replacement.
    pub origin: ParagraphGlyphOrigin,
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
    /// Ordered original-source segments that remain visible (replacement glyphs excluded).
    pub visible_source: Vec<Range<usize>>,
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
