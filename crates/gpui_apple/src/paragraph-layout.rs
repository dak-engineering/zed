#[path = "callback-boundary.rs"]
mod callback_boundary;

use crate::source_indices::SourceIndices;
use anyhow::{Context as _, ensure};
use callback_boundary::CallbackBoundary;
use collections::HashMap;
use core_foundation::base::TCFType;
use core_text::font::{CTFont, CTFontRef};
use gpui::{
    Bounds, FontId, GlyphId, ParagraphAlignment, ParagraphAttachmentGeometry, ParagraphBidiRun,
    ParagraphCaret, ParagraphCaretCoverage, ParagraphCarets, ParagraphDirection,
    ParagraphEllipsize, ParagraphGeometry, ParagraphGlyph, ParagraphGlyphOrigin, ParagraphLine,
    ParagraphLineHeight, ParagraphRequest, ParagraphWidth, Pixels, ResolvedParagraphDirection,
    ShapedGlyph, point, px, size,
};
use std::{ffi::c_void, ops::Range, ptr::NonNull};

#[repr(C)]
struct NativeSelection {
    start: usize,
    length: usize,
}

#[repr(C)]
struct NativeRect {
    bounds: [f64; 4],
}

#[repr(C)]
struct NativeFontRun {
    start: usize,
    length: usize,
    font: CTFontRef,
    baseline_offset: f64,
}

#[repr(C)]
struct NativeLine {
    start: usize,
    length: usize,
    bounds: [f64; 4],
    fragment: [f64; 4],
    primary_count: usize,
    alternate_count: usize,
    carets_available: bool,
    base_direction: u8,
}

#[repr(C)]
struct NativeBidiRun {
    start: usize,
    length: usize,
    level: u8,
}

#[repr(C)]
struct NativeCaret {
    source: usize,
    x: f64,
}

#[repr(C)]
struct NativeGlyph {
    source: usize,
    glyph: u32,
    run: usize,
    position: [f64; 2],
    synthetic: bool,
}

#[repr(C)]
struct NativeRun {
    font: CTFontRef,
    matrix: [f64; 6],
}

struct ResolvedRun {
    font_id: FontId,
    font_size: Pixels,
    is_emoji: bool,
    matrix: [f32; 6],
}

#[repr(C)]
struct NativeLayout {
    private: [u8; 0],
}

#[repr(C)]
struct NativeOptions {
    selection: *const NativeSelection,
    context: *mut c_void,
    emit: extern "C" fn(*mut c_void, *const NativeLayout) -> bool,
    carets: NativeCaretRequest,
    line_height: NativeLineHeight,
    alignment: u8,
    paragraphs: *const NativeParagraphSpan,
    paragraph_count: usize,
    attachments: *const NativeAttachment,
    attachment_count: usize,
    max_lines: usize,
    ellipsize: u8,
    line_break_strategy: u8,
}

impl NativeOptions {
    fn new<F: FnMut(CTFont) -> (FontId, bool)>(
        request: &ParagraphRequest<'_>,
        selected: Option<&NativeSelection>,
        boundary: &mut CallbackBoundary<Receiver<F>, anyhow::Error>,
    ) -> Self {
        Self {
            selection: selected.map_or(std::ptr::null(), |value| value),
            context: std::ptr::from_mut(boundary).cast(),
            emit: receive::<F>,
            carets: NativeCaretRequest::from_request(request),
            line_height: request.line_height().into(),
            alignment: alignment(request.alignment()),
            line_break_strategy: match request.line_break_strategy() {
                gpui::ParagraphLineBreakStrategy::None => 0,
                gpui::ParagraphLineBreakStrategy::Standard => 1,
                gpui::ParagraphLineBreakStrategy::HangulWord => 2,
                gpui::ParagraphLineBreakStrategy::PushOut => 3,
            },
            paragraphs: std::ptr::null(),
            paragraph_count: 0,
            attachments: std::ptr::null(),
            attachment_count: 0,
            max_lines: request
                .line_limit()
                .map_or(0, |limit| limit.max_lines.get()),
            ellipsize: request.line_limit().map_or(2, |limit| match limit.mode {
                ParagraphEllipsize::Head => 0,
                ParagraphEllipsize::Middle => 1,
                ParagraphEllipsize::Tail => 2,
                ParagraphEllipsize::Clip => 3,
            }),
        }
    }
}

#[repr(C)]
struct NativeParagraphSpan {
    start: usize,
    length: usize,
    direction: u8,
    alignment: u8,
    line_height: NativeLineHeight,
    first_line_indent: f64,
    head_indent: f64,
    tail_indent: f64,
    space_before: f64,
    space_after: f64,
}
#[repr(C)]
struct NativeAttachment {
    start: usize,
    length: usize,
    width: f64,
    height: f64,
    baseline_offset: f64,
}
#[repr(C)]
struct NativeAttachmentRect {
    start: usize,
    length: usize,
    bounds: [f64; 4],
}

fn alignment(value: ParagraphAlignment) -> u8 {
    match value {
        ParagraphAlignment::Natural => 0,
        ParagraphAlignment::Left => 1,
        ParagraphAlignment::Center => 2,
        ParagraphAlignment::Right => 3,
        ParagraphAlignment::Justified => 4,
    }
}
fn direction(value: ParagraphDirection) -> u8 {
    match value {
        ParagraphDirection::Natural => 0,
        ParagraphDirection::LeftToRight => 1,
        ParagraphDirection::RightToLeft => 2,
    }
}

fn document_options(
    request: &ParagraphRequest<'_>,
) -> anyhow::Result<(Vec<NativeParagraphSpan>, Vec<NativeAttachment>)> {
    let mut utf16 = 0;
    let mut boundaries = Vec::new();
    let ascii = request.text().is_ascii();
    if !ascii {
        for (byte, character) in request.text().char_indices() {
            boundaries.push((byte, utf16));
            utf16 += character.len_utf16();
        }
        boundaries.push((request.text().len(), utf16));
    }
    let offset = |index| -> anyhow::Result<usize> {
        if ascii {
            ensure!(
                index <= request.text().len(),
                "document offset exceeds source"
            );
            Ok(index)
        } else {
            let at = boundaries
                .binary_search_by_key(&index, |(byte, _)| *byte)
                .map_err(|_| anyhow::anyhow!("document offset splits a source scalar"))?;
            Ok(boundaries[at].1)
        }
    };
    let paragraphs = request
        .paragraph_styles()
        .iter()
        .map(|span| {
            let start = offset(span.source.start)?;
            Ok(NativeParagraphSpan {
                start,
                length: offset(span.source.end)? - start,
                direction: direction(span.direction),
                alignment: alignment(span.alignment),
                line_height: span.line_height.into(),
                first_line_indent: span.first_line_indent.into(),
                head_indent: span.head_indent.into(),
                tail_indent: span.tail_indent.into(),
                space_before: span.space_before.into(),
                space_after: span.space_after.into(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let attachments = request
        .attachments()
        .iter()
        .map(|object| {
            let start = offset(object.source.start)?;
            Ok(NativeAttachment {
                start,
                length: offset(object.source.end)? - start,
                width: object.size.width.into(),
                height: object.size.height.into(),
                baseline_offset: object.baseline_offset.into(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((paragraphs, attachments))
}

#[repr(C)]
struct NativeLineHeight {
    mode: u8,
    height: f64,
}

impl From<ParagraphLineHeight> for NativeLineHeight {
    fn from(height: ParagraphLineHeight) -> Self {
        let (mode, height) = match height {
            ParagraphLineHeight::Natural => (0, 0.),
            ParagraphLineHeight::Exact(value) => (1, f64::from(value)),
            ParagraphLineHeight::AtLeast(value) => (2, f64::from(value)),
        };
        Self { mode, height }
    }
}

#[repr(C)]
struct NativeCaretRequest {
    mode: u8,
    start: usize,
    length: usize,
}

impl NativeCaretRequest {
    fn from_request(request: &ParagraphRequest<'_>) -> Self {
        let text = request.text();
        match request.carets() {
            ParagraphCaretCoverage::All => NativeCaretRequest {
                mode: 0,
                start: 0,
                length: 0,
            },
            ParagraphCaretCoverage::None => NativeCaretRequest {
                mode: 1,
                start: 0,
                length: 0,
            },
            ParagraphCaretCoverage::Source(source) => NativeCaretRequest {
                mode: 2,
                start: text[..source.start].encode_utf16().count(),
                length: text[source.clone()].encode_utf16().count(),
            },
        }
    }
}

struct CachedFont {
    // A raw identity can otherwise be reused after C++ releases an emitted row.
    _retained_font: CTFont,
    font_id: FontId,
    font_size: Pixels,
    is_emoji: bool,
}

struct Receiver<F> {
    length: usize,
    boundaries: SourceIndices,
    carets: ParagraphCaretCoverage,
    direction: ParagraphDirection,
    lines: Vec<ParagraphLine>,
    attachments: Vec<ParagraphAttachmentGeometry>,
    fonts: HashMap<CTFontRef, CachedFont>,
    register: F,
}

impl<F: FnMut(CTFont) -> (FontId, bool)> Receiver<F> {
    fn append(&mut self, native: &NativeLayout) -> anyhow::Result<()> {
        ensure!(
            self.lines.last().is_none_or(|line| !line.source.is_empty()),
            "native row follows terminal caret row"
        );
        let consumed = self.lines.last().map_or(0, |line| line.source.end);
        let boundaries = &self.boundaries;
        let byte = |index| {
            boundaries
                .get(index)
                .context("native offset splits a source scalar")
        };
        let mut line = source_line(
            native,
            self.length,
            consumed,
            &self.carets,
            self.direction,
            &byte,
        )?;
        shaped_glyphs(
            native,
            &mut line,
            &byte,
            &mut self.fonts,
            &mut self.register,
        )?;
        self.attachments.extend(source_attachments(native, &byte)?);
        self.lines.push(line);
        Ok(())
    }
}

extern "C" fn receive<F: FnMut(CTFont) -> (FontId, bool)>(
    context: *mut c_void,
    native: *const NativeLayout,
) -> bool {
    // SAFETY: C++ invokes synchronously on this thread, only while this stack owner lives.
    let boundary = unsafe { &mut *context.cast::<CallbackBoundary<Receiver<F>, anyhow::Error>>() };
    boundary.accept(|receiver| {
        // SAFETY: C++ lends its initialized row and arrays until this callback returns.
        let native = unsafe { native.as_ref() }.context("missing native row")?;
        receiver.append(native)
    })
}

unsafe extern "C" {
    fn gpui_textkit_layout(
        text: *const u16,
        length: usize,
        runs: *const NativeFontRun,
        count: usize,
        width: f64,
        direction: u8,
        options: *const NativeOptions,
    ) -> *mut NativeLayout;
    fn gpui_textkit_layout_drop(layout: *mut NativeLayout);
    fn gpui_textkit_selection_rects(
        layout: *const NativeLayout,
        count: *mut usize,
    ) -> *const NativeRect;
    fn gpui_textkit_line(layout: *const NativeLayout) -> *const NativeLine;
    fn gpui_textkit_runs(layout: *const NativeLayout, count: *mut usize) -> *const NativeRun;
    fn gpui_textkit_glyphs(layout: *const NativeLayout, count: *mut usize) -> *const NativeGlyph;
    fn gpui_textkit_bidi_runs(
        layout: *const NativeLayout,
        count: *mut usize,
    ) -> *const NativeBidiRun;
    fn gpui_textkit_visible_source(
        layout: *const NativeLayout,
        count: *mut usize,
    ) -> *const NativeSelection;
    fn gpui_textkit_attachments(
        layout: *const NativeLayout,
        count: *mut usize,
    ) -> *const NativeAttachmentRect;
    fn gpui_textkit_truncated(layout: *const NativeLayout) -> bool;
    fn gpui_textkit_carets(layout: *const NativeLayout, count: *mut usize) -> *const NativeCaret;
}

struct OwnedLayout(NonNull<NativeLayout>);

impl Drop for OwnedLayout {
    fn drop(&mut self) {
        // SAFETY: The native allocation is uniquely owned and never moved across threads.
        unsafe { gpui_textkit_layout_drop(self.0.as_ptr()) };
    }
}

pub(super) fn layout<F: FnMut(CTFont) -> (FontId, bool)>(
    request: &ParagraphRequest<'_>,
    fonts: &[(usize, CTFont)],
    register: F,
) -> anyhow::Result<ParagraphGeometry> {
    let text = request.text();
    let width = match request.width() {
        ParagraphWidth::Unbounded => px(0.),
        ParagraphWidth::Bounded(width) => width,
    };
    let utf16: Vec<_> = text.encode_utf16().collect();
    let boundaries = SourceIndices::new(text);
    let mut consumed = 0usize;
    let mut utf16_start = 0usize;
    let mut runs = Vec::with_capacity(fonts.len());
    for (index, (length, font)) in fonts.iter().enumerate() {
        let end = consumed.checked_add(*length).context("font run overflow")?;
        let segment = text
            .get(consumed..end)
            .context("invalid font run boundary")?;
        let utf16_length = segment.encode_utf16().count();
        runs.push(NativeFontRun {
            start: utf16_start,
            length: utf16_length,
            font: font.as_concrete_TypeRef(),
            baseline_offset: request
                .baseline_offsets()
                .and_then(|offsets| offsets.get(index))
                .copied()
                .map_or(0., f64::from),
        });
        consumed = end;
        utf16_start += utf16_length;
    }
    ensure!(consumed == text.len(), "font runs must cover the source");
    let selected = request
        .selection()
        .map(|source| -> anyhow::Result<_> {
            let segment = text
                .get(source.clone())
                .context("invalid paragraph selection")?;
            Ok(NativeSelection {
                start: text[..source.start].encode_utf16().count(),
                length: segment.encode_utf16().count(),
            })
        })
        .transpose()?;
    let receiver = Receiver {
        length: text.len(),
        boundaries,
        carets: request.carets().clone(),
        direction: request.direction(),
        lines: Vec::new(),
        attachments: Vec::new(),
        fonts: HashMap::default(),
        register,
    };
    let mut boundary = CallbackBoundary::<_, anyhow::Error>::new(receiver);
    let (paragraphs, attachments) = document_options(request)?;
    let mut options = NativeOptions::new(request, selected.as_ref(), &mut boundary);
    options.paragraphs = paragraphs.as_ptr();
    options.paragraph_count = paragraphs.len();
    options.attachments = attachments.as_ptr();
    options.attachment_count = attachments.len();
    // SAFETY: Input slices/fonts/options outlive this synchronous call. Each row is decoded
    // before native buffers are reused. Panics are held until after C++ returns and cleans up.
    let native = unsafe {
        gpui_textkit_layout(
            utf16.as_ptr(),
            utf16.len(),
            runs.as_ptr(),
            runs.len(),
            width.into(),
            match request.direction() {
                ParagraphDirection::Natural => 0,
                ParagraphDirection::LeftToRight => 1,
                ParagraphDirection::RightToLeft => 2,
            },
            &options,
        )
    };
    let native = NonNull::new(native).map(OwnedLayout);
    let receiver = boundary.finish()?;
    let native = native.context("native paragraph layout failed")?;
    ensure!(!receiver.lines.is_empty(), "native layout emitted no rows");
    ensure!(
        request.line_limit().is_some()
            || receiver
                .lines
                .last()
                .is_some_and(|line| line.source.end == text.len()),
        "native layout dropped source text"
    );
    // SAFETY: The native result owns its copied selection rectangles until native drops.
    let view = unsafe { native.0.as_ref() };
    Ok(ParagraphGeometry {
        lines: receiver.lines,
        selection_rects: selection_rects(view)?,
        attachments: receiver.attachments,
        // SAFETY: The native allocation remains owned by native for this synchronous query.
        truncated: unsafe { gpui_textkit_truncated(view) },
    })
}

fn selection_rects(native: &NativeLayout) -> anyhow::Result<Vec<Bounds<Pixels>>> {
    let mut count = 0;
    // SAFETY: Native owns the initialized array until this call's OwnedLayout drops.
    let pointer = unsafe { gpui_textkit_selection_rects(native, &mut count) };
    if count == 0 {
        return Ok(Vec::new());
    }
    ensure!(!pointer.is_null(), "missing native selection rectangles");
    // SAFETY: The getter borrows count initialized rectangles from native.
    let rectangles = unsafe { std::slice::from_raw_parts(pointer, count) };
    rectangles
        .iter()
        .map(|value| native_bounds(value.bounds))
        .collect()
}

fn source_line(
    native: &NativeLayout,
    length: usize,
    consumed: usize,
    wanted: &ParagraphCaretCoverage,
    _direction: ParagraphDirection,
    byte: &impl Fn(usize) -> anyhow::Result<usize>,
) -> anyhow::Result<ParagraphLine> {
    let carets = native_carets(native)?;
    let bidi = native_bidi_runs(native)?;
    // SAFETY: The current native row is initialized and borrowed for this callback only.
    let line = unsafe { gpui_textkit_line(native).as_ref() }.context("missing native line")?;
    let source = byte(line.start)?
        ..byte(
            line.start
                .checked_add(line.length)
                .context("native range overflow")?,
        )?;
    ensure!(
        source.start == consumed
            && (source.end > source.start || (source.start == length && source.end == length)),
        "native rows must progress contiguously"
    );
    if source.is_empty() {
        ensure!(
            line.bounds[3] > 0. && line.fragment[3] > 0.,
            "empty native caret row has no height"
        );
    }
    ensure!(
        line.carets_available == wanted.covers(&source),
        "native caret availability differs from request"
    );
    ensure!(
        line.carets_available || carets.is_empty(),
        "unrequested native carets"
    );
    let split = line.primary_count;
    let end = split
        .checked_add(line.alternate_count)
        .context("native caret range overflow")?;
    ensure!(end == carets.len(), "invalid native caret coverage");
    let primary = carets
        .get(..split)
        .context("invalid native primary caret range")?;
    let alternate = carets
        .get(split..end)
        .context("invalid native alternate caret range")?;
    let visible_source = visible_source(native, byte)?;
    let retain_visible = |carets: Vec<ParagraphCaret>| {
        carets
            .into_iter()
            .filter(|caret| {
                visible_source
                    .iter()
                    .any(|span| span.start <= caret.index && caret.index <= span.end)
            })
            .collect()
    };
    Ok(ParagraphLine {
        base_direction: match line.base_direction {
            1 => Some(ResolvedParagraphDirection::LeftToRight),
            2 => Some(ResolvedParagraphDirection::RightToLeft),
            _ => None,
        },
        carets: if line.carets_available {
            Some(ParagraphCarets {
                primary: retain_visible(source_carets(primary, &source, byte)?),
                alternate: retain_visible(source_carets(alternate, &source, byte)?),
            })
        } else {
            None
        },
        bidi_runs: source_bidi_runs(bidi, &source, byte)?,
        source,
        visible_source,
        bounds: native_bounds(line.bounds)?,
        fragment_bounds: native_bounds(line.fragment)?,
        glyphs: Vec::new(),
    })
}

fn native_bidi_runs(native: &NativeLayout) -> anyhow::Result<&[NativeBidiRun]> {
    let mut count = 0;
    // SAFETY: Native lends the initialized row array only for this synchronous callback.
    let pointer = unsafe { gpui_textkit_bidi_runs(native, &mut count) };
    if count == 0 {
        return Ok(&[]);
    }
    ensure!(!pointer.is_null(), "missing native bidi runs");
    // SAFETY: The getter returns count initialized elements, borrowed from native.
    Ok(unsafe { std::slice::from_raw_parts(pointer, count) })
}

fn source_bidi_runs(
    runs: &[NativeBidiRun],
    source: &Range<usize>,
    byte: &impl Fn(usize) -> anyhow::Result<usize>,
) -> anyhow::Result<Vec<ParagraphBidiRun>> {
    let mut output = Vec::with_capacity(runs.len());
    let mut consumed = source.start;
    for run in runs {
        let start = byte(run.start)?;
        let end = byte(
            run.start
                .checked_add(run.length)
                .context("native bidi range overflow")?,
        )?;
        ensure!(
            start == consumed && end > start && end <= source.end,
            "invalid native bidi source coverage"
        );
        output.push(ParagraphBidiRun {
            source: start..end,
            level: run.level,
        });
        consumed = end;
    }
    ensure!(consumed == source.end, "native bidi runs dropped source");
    Ok(output)
}

fn native_bounds(values: [f64; 4]) -> anyhow::Result<Bounds<Pixels>> {
    ensure!(
        values
            .iter()
            .all(|value| value.is_finite() && value.abs() <= f64::from(f32::MAX)),
        "non-finite native line bounds"
    );
    ensure!(
        values[2] >= 0. && values[3] >= 0.,
        "negative native line dimensions"
    );
    Ok(Bounds {
        origin: point(px(values[0] as f32), px(values[1] as f32)),
        size: size(px(values[2] as f32), px(values[3] as f32)),
    })
}

fn native_carets(native: &NativeLayout) -> anyhow::Result<&[NativeCaret]> {
    let mut count = 0;
    // SAFETY: The native owner retains the initialized caret array throughout decoding.
    let pointer = unsafe { gpui_textkit_carets(native, &mut count) };
    if count == 0 {
        return Ok(&[]);
    }
    ensure!(!pointer.is_null(), "missing native carets");
    // SAFETY: The getter returns count initialized elements borrowed from native.
    Ok(unsafe { std::slice::from_raw_parts(pointer, count) })
}

fn source_carets(
    carets: &[NativeCaret],
    source: &Range<usize>,
    byte: &impl Fn(usize) -> anyhow::Result<usize>,
) -> anyhow::Result<Vec<ParagraphCaret>> {
    carets
        .iter()
        .map(|caret| {
            let index = byte(caret.source)?;
            ensure!(
                source.start <= index && index <= source.end,
                "caret is outside its source row"
            );
            ensure!(
                caret.x.is_finite() && caret.x.abs() <= f64::from(f32::MAX),
                "non-finite native caret position"
            );
            Ok(ParagraphCaret {
                index,
                x: px(caret.x as f32),
            })
        })
        .collect()
}

fn resolved_runs(
    native: &NativeLayout,
    fonts: &mut HashMap<CTFontRef, CachedFont>,
    register: &mut impl FnMut(CTFont) -> (FontId, bool),
) -> anyhow::Result<Vec<ResolvedRun>> {
    let mut count = 0;
    // SAFETY: Runs and their retained fonts belong to native and remain live throughout decoding.
    let pointer = unsafe { gpui_textkit_runs(native, &mut count) };
    let runs = if count == 0 {
        &[]
    } else {
        ensure!(!pointer.is_null(), "missing native drawing runs");
        // SAFETY: The getter provides count initialized runs, borrowed from the native owner.
        unsafe { std::slice::from_raw_parts(pointer, count) }
    };
    let mut resolved = Vec::with_capacity(runs.len());
    for run in runs {
        ensure!(!run.font.is_null(), "native run has no font");
        ensure!(
            run.matrix
                .iter()
                .all(|value| value.is_finite() && value.abs() <= f64::from(f32::MAX)),
            "non-finite native text matrix"
        );
        let (font_id, font_size, is_emoji) = if let Some(font) = fonts.get(&run.font) {
            (font.font_id, font.font_size, font.is_emoji)
        } else {
            // SAFETY: The native run retains the font through the row callback. Get-rule wrapping
            // retains it independently before registration uses the font's identity.
            let font = unsafe { CTFont::wrap_under_get_rule(run.font) };
            let font_size = px(font.pt_size() as f32);
            ensure!(
                f32::from(font_size).is_finite() && font_size > px(0.),
                "invalid native font size"
            );
            let retained_font = font.clone();
            let (font_id, is_emoji) = register(font);
            fonts.insert(
                run.font,
                CachedFont {
                    _retained_font: retained_font,
                    font_id,
                    font_size,
                    is_emoji,
                },
            );
            (font_id, font_size, is_emoji)
        };
        resolved.push(ResolvedRun {
            font_id,
            font_size,
            is_emoji,
            matrix: run.matrix.map(|value| value as f32),
        });
    }
    Ok(resolved)
}

fn shaped_glyphs(
    native: &NativeLayout,
    line: &mut ParagraphLine,
    byte: &impl Fn(usize) -> anyhow::Result<usize>,
    fonts: &mut HashMap<CTFontRef, CachedFont>,
    register: &mut impl FnMut(CTFont) -> (FontId, bool),
) -> anyhow::Result<()> {
    let runs = resolved_runs(native, fonts, register)?;
    let mut glyph_count = 0;
    // SAFETY: The returned array and its run indices are owned by native for this call.
    let glyph_pointer = unsafe { gpui_textkit_glyphs(native, &mut glyph_count) };
    let glyphs = if glyph_count == 0 {
        &[]
    } else {
        ensure!(!glyph_pointer.is_null(), "missing native glyphs");
        // SAFETY: The getter returns glyph_count initialized elements borrowed from native.
        unsafe { std::slice::from_raw_parts(glyph_pointer, glyph_count) }
    };
    line.glyphs.reserve_exact(glyphs.len());
    for glyph in glyphs {
        let source = byte(glyph.source)?;
        let run = runs
            .get(glyph.run)
            .context("invalid native glyph drawing run")?;
        ensure!(
            line.source.contains(&source),
            "glyph is outside its source row"
        );
        ensure!(
            glyph
                .position
                .iter()
                .all(|value| value.is_finite() && value.abs() <= f64::from(f32::MAX)),
            "non-finite native glyph position"
        );
        line.glyphs.push(ParagraphGlyph {
            origin: if glyph.synthetic {
                ParagraphGlyphOrigin::Ellipsis
            } else {
                ParagraphGlyphOrigin::Source
            },
            font_id: run.font_id,
            font_size: run.font_size,
            glyph: ShapedGlyph {
                id: GlyphId(glyph.glyph),
                position: point(px(glyph.position[0] as f32), px(glyph.position[1] as f32)),
                index: source,
                is_emoji: run.is_emoji,
            },
            text_matrix: run.matrix,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "natural-direction-tests.rs"]
mod natural_direction_tests;

fn visible_source(
    native: &NativeLayout,
    byte: &impl Fn(usize) -> anyhow::Result<usize>,
) -> anyhow::Result<Vec<Range<usize>>> {
    let mut count = 0;
    // SAFETY: This getter lends initialized current-row ranges until the callback returns.
    let pointer = unsafe { gpui_textkit_visible_source(native, &mut count) };
    if count == 0 {
        return Ok(Vec::new());
    }
    ensure!(!pointer.is_null(), "missing native visible source");
    // SAFETY: Native provides exactly count initialized ranges for this synchronous callback.
    unsafe { std::slice::from_raw_parts(pointer, count) }
        .iter()
        .map(|span| {
            Ok(byte(span.start)?
                ..byte(
                    span.start
                        .checked_add(span.length)
                        .context("visible source overflow")?,
                )?)
        })
        .collect()
}

fn source_attachments(
    native: &NativeLayout,
    byte: &impl Fn(usize) -> anyhow::Result<usize>,
) -> anyhow::Result<Vec<ParagraphAttachmentGeometry>> {
    let mut count = 0;
    // SAFETY: This getter lends initialized current-row objects until the callback returns.
    let pointer = unsafe { gpui_textkit_attachments(native, &mut count) };
    if count == 0 {
        return Ok(Vec::new());
    }
    ensure!(!pointer.is_null(), "missing native attachment rectangles");
    // SAFETY: Native provides exactly count initialized objects for this synchronous callback.
    unsafe { std::slice::from_raw_parts(pointer, count) }
        .iter()
        .map(|object| {
            Ok(ParagraphAttachmentGeometry {
                source: byte(object.start)?
                    ..byte(
                        object
                            .start
                            .checked_add(object.length)
                            .context("attachment source overflow")?,
                    )?,
                bounds: native_bounds(object.bounds)?,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "document-truncation-tests.rs"]
mod document_truncation_tests;

#[cfg(test)]
#[path = "line-break-strategy-tests.rs"]
mod line_break_strategy_tests;
