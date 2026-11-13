use crate::AppleTextSystem;
use gpui::{
    Font, ParagraphAlignment, ParagraphAttachment, ParagraphDirection, ParagraphEllipsize,
    ParagraphGlyphOrigin, ParagraphLayout, ParagraphLineHeight, ParagraphLineLimit,
    ParagraphStyleSpan, ParagraphWidth, PlatformTextSystem as _, TextRun, TextSystem,
    WindowTextSystem, px, size,
};
use objc2::{
    class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSRange, NSRect, NSSize, NSString};
use std::{num::NonZeroUsize, ops::Range, sync::Arc};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct OracleLine {
    source: NSRange,
    hidden: NSRange,
    used: NSRect,
    carets: Vec<(usize, f64)>,
}
struct Fixture<'a> {
    text: &'a str,
    split: usize,
    object: Option<usize>,
    lines: usize,
    mode: ParagraphEllipsize,
}

fn system() -> Result<WindowTextSystem> {
    let backend = Arc::new(AppleTextSystem::new());
    backend.font_id(&Font::default())?;
    Ok(WindowTextSystem::new(Arc::new(TextSystem::new(backend))))
}
fn utf16(text: &str, index: usize) -> usize {
    text[..index].encode_utf16().count()
}
fn paragraph(source: Range<usize>, height: f32) -> ParagraphStyleSpan {
    ParagraphStyleSpan {
        source,
        direction: ParagraphDirection::LeftToRight,
        alignment: ParagraphAlignment::Left,
        line_height: ParagraphLineHeight::AtLeast(px(height)),
        first_line_indent: px(4.),
        head_indent: px(2.),
        tail_indent: px(-3.),
        space_before: px(0.),
        space_after: px(2.),
    }
}

fn oracle(fixture: &Fixture<'_>) -> Vec<OracleLine> {
    // SAFETY: All native objects are retained on the calling thread. These public
    // AppKit selectors and structure encodings are independent of the bridge.
    unsafe {
        let text = fixture.text;
        let source = NSString::from_str(text);
        let storage: Allocated<AnyObject> = msg_send![class!(NSTextStorage), alloc];
        let storage: Retained<AnyObject> = msg_send![storage, initWithString: &*source];
        let font_key = NSString::from_str("NSFont");
        let paragraph_key = NSString::from_str("NSParagraphStyle");
        for (range, font_size, height) in [
            (0..fixture.split, 24_f64, 30_f64),
            (fixture.split..text.len(), 14., 22.),
        ] {
            if range.is_empty() {
                continue;
            }
            let font: Retained<AnyObject> = msg_send![class!(NSFont), systemFontOfSize: font_size];
            let style: Retained<AnyObject> = msg_send![class!(NSMutableParagraphStyle), new];
            let _: () = msg_send![&*style, setBaseWritingDirection: 0_isize];
            let _: () = msg_send![&*style, setAlignment: 0_isize];
            let _: () = msg_send![&*style, setMinimumLineHeight: height];
            let _: () = msg_send![&*style, setFirstLineHeadIndent: 4_f64];
            let _: () = msg_send![&*style, setHeadIndent: 2_f64];
            let _: () = msg_send![&*style, setTailIndent: -3_f64];
            let _: () = msg_send![&*style, setParagraphSpacing: 2_f64];
            let range = NSRange::new(utf16(text, range.start), text[range].encode_utf16().count());
            let _: () = msg_send![&*storage, addAttribute: &*font_key, value: &*font, range: range];
            if range.location == 0 && fixture.split > 0 {
                let key = NSString::from_str("NSBaselineOffset");
                let offset: Retained<AnyObject> =
                    msg_send![class!(NSNumber), numberWithDouble: 3_f64];
                let _: () =
                    msg_send![&*storage, addAttribute: &*key, value: &*offset, range: range];
            }
            let _: () =
                msg_send![&*storage, addAttribute: &*paragraph_key, value: &*style, range: range];
        }
        if let Some(index) = fixture.object {
            let attachment: Retained<AnyObject> = msg_send![class!(NSTextAttachment), new];
            let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let image: Retained<AnyObject> = msg_send![image, initWithSize: NSSize::new(20., 28.)];
            let cell: Allocated<AnyObject> = msg_send![class!(NSTextAttachmentCell), alloc];
            let cell: Retained<AnyObject> = msg_send![cell, initImageCell: &*image];
            let _: () = msg_send![&*attachment, setAttachmentCell: &*cell];
            let _: () = msg_send![&*attachment, setBounds: NSRect::new(objc2_foundation::NSPoint::new(0., 0.), NSSize::new(20., 28.))];
            let key = NSString::from_str("NSAttachment");
            let _: () = msg_send![&*storage, addAttribute: &*key, value: &*attachment, range: NSRange::new(utf16(text, index), 1)];
        }
        let layout: Retained<AnyObject> = msg_send![class!(NSLayoutManager), new];
        let _: () = msg_send![&*layout, setBackgroundLayoutEnabled: Bool::NO];
        let _: () = msg_send![&*layout, setAllowsNonContiguousLayout: Bool::NO];
        let _: () = msg_send![&*layout, setUsesFontLeading: Bool::NO];
        let container: Allocated<AnyObject> = msg_send![class!(NSTextContainer), alloc];
        let container: Retained<AnyObject> =
            msg_send![container, initWithContainerSize: NSSize::new(120., f64::MAX)];
        let _: () = msg_send![&*container, setLineFragmentPadding: 0_f64];
        let _: () = msg_send![&*container, setMaximumNumberOfLines: fixture.lines];
        let mode = if fixture.lines == 0 {
            0_usize
        } else {
            match fixture.mode {
                ParagraphEllipsize::Clip => 2,
                ParagraphEllipsize::Head => 3,
                ParagraphEllipsize::Tail => 4,
                ParagraphEllipsize::Middle => 5,
            }
        };
        let _: () = msg_send![&*container, setLineBreakMode: mode];
        let _: () = msg_send![&*layout, addTextContainer: &*container];
        let _: () = msg_send![&*storage, addLayoutManager: &*layout];
        let _: () = msg_send![&*layout, ensureLayoutForTextContainer: &*container];
        oracle_lines(&layout, &container, fixture.lines, fixture.mode)
    }
}

fn oracle_replacement_range(layout: &AnyObject, row: NSRange) -> Option<NSRange> {
    use core_foundation::base::TCFType as _;
    // SAFETY: The caller retains the fully laid out native graph. Font objects
    // are borrowed from its storage and retained by CTFont's get-rule wrapper.
    unsafe {
        let storage: Retained<AnyObject> = msg_send![layout, textStorage];
        let key = NSString::from_str("NSFont");
        for index in row.location..row.location + row.length {
            let shown: Bool = msg_send![layout, notShownAttributeForGlyphAtIndex: index];
            if shown.as_bool() {
                continue;
            }
            let character: usize = msg_send![layout, characterIndexForGlyphAtIndex: index];
            let font: Retained<AnyObject> = msg_send![&*storage, attribute: &*key, atIndex: character, effectiveRange: std::ptr::null_mut::<NSRange>()];
            let font = core_text::font::CTFont::wrap_under_get_rule(Retained::as_ptr(&font).cast());
            let mut ellipsis = 0_u16;
            if !font.get_glyphs_for_characters(&0x2026_u16, &mut ellipsis, 1) {
                continue;
            }
            let actual: u16 = msg_send![layout, CGGlyphAtIndex: index];
            if actual != ellipsis {
                continue;
            }
            let mut start = index;
            let mut end = index + 1;
            while start > row.location {
                let property: isize = msg_send![layout, propertyForGlyphAtIndex: start - 1];
                let hidden: Bool = msg_send![layout, notShownAttributeForGlyphAtIndex: start - 1];
                if property & 1 == 0 || !hidden.as_bool() {
                    break;
                }
                start -= 1;
            }
            while end < row.location + row.length {
                let property: isize = msg_send![layout, propertyForGlyphAtIndex: end];
                let hidden: Bool = msg_send![layout, notShownAttributeForGlyphAtIndex: end];
                if property & 1 == 0 || !hidden.as_bool() {
                    break;
                }
                end += 1;
            }
            if end - start > 1 {
                return Some(NSRange::new(start, end - start));
            }
        }
        None
    }
}

fn clipped_cluster_range(
    layout: &AnyObject,
    container: &AnyObject,
    glyphs: NSRange,
    fragment: NSRect,
) -> NSRange {
    // SAFETY: The caller retains a laid-out AppKit object graph. Cluster and
    // bounding ranges are copied from its native public layout queries.
    unsafe {
        let used: NSRect = msg_send![layout, lineFragmentUsedRectForGlyphAtIndex: glyphs.location, effectiveRange: std::ptr::null_mut::<NSRange>(), withoutAdditionalLayout: Bool::YES];
        if used.size.width < fragment.size.width {
            return msg_send![layout, characterRangeForGlyphRange: glyphs, actualGlyphRange: std::ptr::null_mut::<NSRange>()];
        }
        let mut first = usize::MAX;
        let mut end = 0;
        let mut cursor = glyphs.location;
        while cursor < glyphs.location + glyphs.length {
            let mut cluster = NSRange::new(0, 0);
            let characters: NSRange = msg_send![layout, characterRangeForGlyphRange: NSRange::new(cursor, 1), actualGlyphRange: &mut cluster];
            let bounds: NSRect =
                msg_send![layout, boundingRectForGlyphRange: cluster, inTextContainer: container];
            if bounds.size.width > 0.
                && bounds.size.height > 0.
                && bounds.origin.x < fragment.origin.x + fragment.size.width
                && bounds.origin.x + bounds.size.width > fragment.origin.x
                && bounds.origin.y < fragment.origin.y + fragment.size.height
                && bounds.origin.y + bounds.size.height > fragment.origin.y
            {
                first = first.min(characters.location);
                end = end.max(characters.location + characters.length);
            }
            cursor = (cursor + 1).max(cluster.location + cluster.length);
        }
        assert!(
            first < end,
            "native clip has no intersecting source cluster"
        );
        NSRange::new(first, end - first)
    }
}

fn oracle_lines(
    layout: &AnyObject,
    container: &AnyObject,
    limit: usize,
    mode: ParagraphEllipsize,
) -> Vec<OracleLine> {
    // SAFETY: The caller retains the fully laid out native object graph. Native
    // insertion count is queried before allocating its output buffers.
    unsafe {
        let allocated: NSRange = msg_send![layout, glyphRangeForTextContainer: container];
        let mut output = Vec::new();
        let mut cursor = allocated.location;
        while cursor < allocated.location + allocated.length && (limit == 0 || output.len() < limit)
        {
            let mut range = NSRange::new(0, 0);
            let fragment: NSRect = msg_send![layout, lineFragmentRectForGlyphAtIndex: cursor, effectiveRange: &mut range, withoutAdditionalLayout: Bool::YES];
            let used: NSRect = msg_send![layout, lineFragmentUsedRectForGlyphAtIndex: cursor, effectiveRange: std::ptr::null_mut::<NSRange>(), withoutAdditionalLayout: Bool::YES];
            let source: NSRange = msg_send![layout, characterRangeForGlyphRange: range, actualGlyphRange: std::ptr::null_mut::<NSRange>()];
            let mut hidden: NSRange =
                msg_send![layout, truncatedGlyphRangeInLineFragmentForGlyphAtIndex: cursor];
            if hidden.length == 0 {
                hidden = oracle_replacement_range(layout, range).unwrap_or(hidden);
            }
            let hidden = if mode == ParagraphEllipsize::Clip && limit > 0 {
                let visible = clipped_cluster_range(layout, container, range, fragment);
                if visible.location > source.location {
                    NSRange::new(source.location, visible.location - source.location)
                } else {
                    NSRange::new(
                        visible.location + visible.length,
                        source.location + source.length - visible.location - visible.length,
                    )
                }
            } else if hidden.length > 0 {
                msg_send![layout, characterRangeForGlyphRange: hidden, actualGlyphRange: std::ptr::null_mut::<NSRange>()]
            } else {
                NSRange::new(source.location + source.length, 0)
            };
            let count: usize = msg_send![layout, getLineFragmentInsertionPointsForCharacterAtIndex: source.location, alternatePositions: Bool::NO, inDisplayOrder: Bool::YES, positions: std::ptr::null_mut::<f64>(), characterIndexes: std::ptr::null_mut::<usize>()];
            let mut positions = vec![0_f64; count];
            let mut indices = vec![0_usize; count];
            let count: usize = msg_send![layout, getLineFragmentInsertionPointsForCharacterAtIndex: source.location, alternatePositions: Bool::NO, inDisplayOrder: Bool::YES, positions: positions.as_mut_ptr(), characterIndexes: indices.as_mut_ptr()];
            let carets = indices
                .into_iter()
                .zip(positions)
                .take(count)
                .filter(|(index, _)| {
                    hidden.length == 0
                        || hidden.location > source.location
                            && (source.location..=hidden.location).contains(index)
                        || hidden.location + hidden.length < source.location + source.length
                            && (hidden.location + hidden.length..=source.location + source.length)
                                .contains(index)
                })
                .map(|(index, x)| (index, x + fragment.origin.x))
                .collect();
            assert!(range.length > 0);
            output.push(OracleLine {
                source,
                hidden,
                used,
                carets,
            });
            cursor = range.location + range.length;
        }
        output
    }
}

fn compare(fixture: Fixture<'_>) -> Result<()> {
    let system = system()?;
    let text = fixture.text;
    let runs = [
        TextRun {
            len: fixture.split,
            ..Default::default()
        },
        TextRun {
            len: text.len() - fixture.split,
            ..Default::default()
        },
    ];
    let mut request = system
        .attributed_paragraph_request(
            text,
            px(20.),
            &runs,
            ParagraphWidth::Bounded(px(120.)),
            ParagraphDirection::Natural,
        )?
        .with_font_sizes(vec![px(24.), px(14.)])?
        .with_baseline_offsets(vec![px(3.), px(0.)])?
        .with_paragraph_styles(
            [
                paragraph(0..fixture.split, 30.),
                paragraph(fixture.split..text.len(), 22.),
            ]
            .into_iter()
            .filter(|span| !span.source.is_empty())
            .collect(),
        )?
        .with_selection(0..text.len())?;
    if let Some(index) = fixture.object {
        request = request.with_attachments(vec![ParagraphAttachment {
            source: index..index + '\u{fffc}'.len_utf8(),
            size: size(px(20.), px(28.)),
            baseline_offset: px(0.),
        }])?;
    }
    if let Some(max_lines) = NonZeroUsize::new(fixture.lines) {
        request = request.with_line_limit(ParagraphLineLimit {
            max_lines,
            mode: fixture.mode,
        });
    }
    let ParagraphLayout::LaidOut(actual) = system.layout_paragraph(request)? else {
        return Err("native layout unavailable".into());
    };
    let expected = oracle(&fixture);
    assert_eq!(actual.lines.len(), expected.len());
    for (line, expected) in actual.lines.iter().zip(expected) {
        assert_eq!(utf16(text, line.source.start), expected.source.location);
        assert_eq!(
            utf16(text, line.source.end),
            expected.source.location + expected.source.length
        );
        for (got, want) in [
            (line.bounds.left(), expected.used.origin.x),
            (line.bounds.top(), expected.used.origin.y),
            (line.bounds.size.width, expected.used.size.width),
            (line.bounds.size.height, expected.used.size.height),
        ] {
            assert!((f64::from(got) - want).abs() < 0.001, "{got:?} != {want}");
        }
        for range in &line.visible_source {
            let start = utf16(text, range.start);
            let end = utf16(text, range.end);
            assert!(
                expected.hidden.length == 0
                    || end <= expected.hidden.location
                    || start >= expected.hidden.location + expected.hidden.length
            );
        }
        let carets = &line.carets.as_ref().ok_or("missing carets")?.primary;
        assert_eq!(
            carets.len(),
            expected.carets.len(),
            "{:?} {} source {:?} hidden {:?} visible {:?}",
            fixture.mode,
            fixture.lines,
            line.source,
            expected.hidden,
            line.visible_source
        );
        for (actual, (index, x)) in carets.iter().zip(expected.carets) {
            assert_eq!(utf16(text, actual.index), index);
            assert!((f64::from(actual.x) - x).abs() < 0.001);
        }
        for glyph in &line.glyphs {
            if glyph.origin == ParagraphGlyphOrigin::Source {
                assert!(
                    line.visible_source
                        .iter()
                        .any(|range| range.contains(&glyph.glyph.index))
                );
            } else {
                assert!(expected.hidden.length > 0);
            }
        }
    }
    if fixture.lines == 0 {
        assert!(!actual.truncated);
    }
    if let Some(index) = fixture.object {
        let object = actual
            .attachments
            .iter()
            .find(|object| object.source.start == index)
            .ok_or("visible attachment missing")?;
        assert_eq!(object.source, index..index + 3);
        assert_eq!(object.bounds.size, size(px(20.), px(28.)));
    }
    assert_eq!(text, fixture.text);
    Ok(())
}

#[test]
fn document_line_limit_modes_match_independent_textkit_source_and_carets() -> Result<()> {
    let text = "ordinary words are longer than this viewport and contain אבג Unicode";
    for mode in [
        ParagraphEllipsize::Head,
        ParagraphEllipsize::Middle,
        ParagraphEllipsize::Tail,
        ParagraphEllipsize::Clip,
    ] {
        compare(Fixture {
            text,
            split: 0,
            object: None,
            lines: 1,
            mode,
        })?;
    }
    Ok(())
}

#[test]
fn document_limit_keeps_mixed_fonts_paragraphs_and_attachment_metrics_together() -> Result<()> {
    let text =
        "Heading\nsmall \u{fffc} words wrap over several native visual lines after the heading";
    let object = text.find('\u{fffc}').ok_or("object")?;
    for lines in [0, 3] {
        compare(Fixture {
            text,
            split: 8,
            object: Some(object),
            lines,
            mode: ParagraphEllipsize::Tail,
        })?;
    }
    Ok(())
}

#[test]
fn attributed_requests_preserve_metric_boundaries_and_plain_requests_still_coalesce() -> Result<()>
{
    let system = system()?;
    let runs = [
        TextRun {
            len: 1,
            ..Default::default()
        },
        TextRun {
            len: 1,
            ..Default::default()
        },
    ];
    let width = ParagraphWidth::Bounded(px(120.));
    let ordinary =
        system.paragraph_request("ab", px(20.), &runs, width, ParagraphDirection::LeftToRight)?;
    assert_eq!(ordinary.runs().len(), 1);
    let attributed = system
        .attributed_paragraph_request("ab", px(20.), &runs, width, ParagraphDirection::LeftToRight)?
        .with_font_sizes(vec![px(12.), px(24.)])?
        .with_baseline_offsets(vec![px(3.), px(-2.)])?;
    assert_eq!(
        attributed
            .runs()
            .iter()
            .map(|run| run.len)
            .collect::<Vec<_>>(),
        [1, 1]
    );
    assert_eq!(attributed.font_sizes(), Some([px(12.), px(24.)].as_slice()));
    assert_eq!(
        attributed.baseline_offsets(),
        Some([px(3.), px(-2.)].as_slice())
    );
    let ParagraphLayout::LaidOut(geometry) = system.layout_paragraph(attributed)? else {
        return Err("native layout unavailable".into());
    };
    for (index, expected) in [(0, 12_f32), (1, 24_f32)] {
        let glyph = geometry
            .lines
            .iter()
            .flat_map(|line| &line.glyphs)
            .find(|glyph| glyph.glyph.index == index)
            .ok_or("source glyph missing")?;
        assert!((f32::from(glyph.font_size) - expected).abs() < 0.001);
    }
    Ok(())
}

fn mixed_weight_layout(
    text: &str,
    mode: ParagraphEllipsize,
    width: f32,
) -> Result<gpui::ParagraphGeometry> {
    let system = system()?;
    let mut normal = gpui::font("Geist");
    normal.features = gpui::FontFeatures(Arc::new(vec![
        ("ss03".into(), 0),
        ("ss05".into(), 1),
        ("ss09".into(), 1),
    ]));
    let mut strong = normal.clone();
    strong.weight = gpui::FontWeight::MEDIUM;
    let runs = [
        TextRun {
            len: 4,
            font: strong,
            ..Default::default()
        },
        TextRun {
            len: text.len() - 4,
            font: normal,
            ..Default::default()
        },
    ];
    let request = system
        .attributed_paragraph_request(
            text,
            px(20.),
            &runs,
            ParagraphWidth::Bounded(px(width)),
            ParagraphDirection::Natural,
        )?
        .with_font_sizes(vec![px(20.), px(20.)])?
        .with_baseline_offsets(vec![px(0.), px(0.)])?
        .with_paragraph_styles(vec![ParagraphStyleSpan {
            source: 0..text.len(),
            direction: ParagraphDirection::Natural,
            alignment: ParagraphAlignment::Natural,
            line_height: ParagraphLineHeight::Exact(px(30.)),
            first_line_indent: px(0.),
            head_indent: px(0.),
            tail_indent: px(0.),
            space_before: px(0.),
            space_after: px(0.),
        }])?
        .with_line_limit(ParagraphLineLimit {
            max_lines: NonZeroUsize::new(1).ok_or("one line")?,
            mode,
        });
    match system.layout_paragraph(request)? {
        ParagraphLayout::LaidOut(geometry) => Ok(geometry),
        ParagraphLayout::Unsupported => Err("native layout unavailable".into()),
    }
}

#[test]
fn mixed_weight_truncation_retains_only_visible_source_and_native_replacements() -> Result<()> {
    for mode in [
        ParagraphEllipsize::Head,
        ParagraphEllipsize::Middle,
        ParagraphEllipsize::Tail,
        ParagraphEllipsize::Clip,
    ] {
        let geometry = mixed_weight_layout("AAAA bbbb cccc dddd eeee ffff GGGG", mode, 100.)
            .map_err(|error| format!("{mode:?}: {error}"))?;
        assert!(geometry.truncated, "{mode:?}: {geometry:?}");
        assert_eq!(geometry.lines.len(), 1);
        let line = &geometry.lines[0];
        assert_eq!(line.source, 0..34);
        let visible = &line.visible_source;
        if mode == ParagraphEllipsize::Clip {
            assert_eq!(visible, &[0..9]);
        }
        assert!(!visible.is_empty());
        assert!(visible.iter().map(std::ops::Range::len).sum::<usize>() < 34);
        let replacement = line
            .glyphs
            .iter()
            .any(|glyph| glyph.origin == ParagraphGlyphOrigin::Ellipsis);
        assert_eq!(replacement, mode != ParagraphEllipsize::Clip);
        assert!(
            line.glyphs
                .iter()
                .filter(|glyph| glyph.origin == ParagraphGlyphOrigin::Source)
                .all(|glyph| visible
                    .iter()
                    .any(|range| range.contains(&glyph.glyph.index)))
        );
        assert!(
            line.carets
                .as_ref()
                .ok_or("carets")?
                .primary
                .iter()
                .all(|caret| visible
                    .iter()
                    .any(|range| range.start <= caret.index && caret.index <= range.end))
        );
        match mode {
            ParagraphEllipsize::Head => {
                assert!(visible[0].start > 0);
                assert_eq!(visible.last().ok_or("suffix")?.end, 34);
            }
            ParagraphEllipsize::Middle => {
                assert_eq!(visible[0].start, 0);
                assert_eq!(visible.last().ok_or("suffix")?.end, 34);
            }
            ParagraphEllipsize::Tail | ParagraphEllipsize::Clip => {
                assert_eq!(visible[0].start, 0);
                assert!(visible.last().ok_or("prefix")?.end < 34);
            }
        }
    }
    Ok(())
}

#[test]
fn existing_control_and_ligature_glyph_slots_do_not_become_truncation() -> Result<()> {
    for mode in [
        ParagraphEllipsize::Head,
        ParagraphEllipsize::Middle,
        ParagraphEllipsize::Tail,
        ParagraphEllipsize::Clip,
    ] {
        let geometry = mixed_weight_layout("AAAA office \u{200e}", mode, 500.)?;
        assert!(!geometry.truncated, "{mode:?}: {geometry:?}");
        assert_eq!(geometry.lines.len(), 1);
        assert_eq!(geometry.lines[0].source, 0..15);
        assert_eq!(geometry.lines[0].visible_source, [0..15]);
        assert!(
            geometry.lines[0]
                .glyphs
                .iter()
                .all(|glyph| glyph.origin == ParagraphGlyphOrigin::Source)
        );
        let carets = &geometry.lines[0].carets.as_ref().ok_or("carets")?.primary;
        assert!(carets.iter().any(|caret| caret.index == 12));
        assert!(carets.iter().any(|caret| caret.index == 15));
        let literal = mixed_weight_layout("AAAA …\u{200e}", mode, 500.)?;
        assert!(!literal.truncated, "{mode:?}: {literal:?}");
        assert_eq!(literal.lines[0].visible_source, [0..11]);
        assert!(
            literal.lines[0]
                .glyphs
                .iter()
                .all(|glyph| glyph.origin == ParagraphGlyphOrigin::Source)
        );
    }
    Ok(())
}

#[test]
fn literal_ellipsis_at_truncated_source_start_does_not_hide_omitted_content() -> Result<()> {
    let geometry = mixed_weight_layout(
        "… ordinary words exceed the viewport and end GGGG",
        ParagraphEllipsize::Head,
        100.,
    )?;
    assert!(geometry.truncated, "{geometry:?}");
    assert_eq!(geometry.lines.len(), 1);
    assert!(geometry.lines[0].visible_source[0].start > 0);
    assert!(
        geometry.lines[0]
            .glyphs
            .iter()
            .any(|glyph| glyph.origin == ParagraphGlyphOrigin::Ellipsis)
    );
    Ok(())
}

fn clipped_bounds_oracle(text: &str, direction: isize) -> NSRange {
    // SAFETY: All objects are retained on this thread; AppKit owns its fitted glyph
    // geometry, and the character-range result is copied before releasing them.
    unsafe {
        let source = NSString::from_str(text);
        let storage: Allocated<AnyObject> = msg_send![class!(NSTextStorage), alloc];
        let storage: Retained<AnyObject> = msg_send![storage, initWithString: &*source];
        let font: Retained<AnyObject> = msg_send![class!(NSFont), systemFontOfSize: 20_f64];
        let style: Retained<AnyObject> = msg_send![class!(NSMutableParagraphStyle), new];
        let _: () = msg_send![&*style, setBaseWritingDirection: direction];
        let _: () = msg_send![&*style, setAlignment: 4_isize];
        let font_key = NSString::from_str("NSFont");
        let style_key = NSString::from_str("NSParagraphStyle");
        let range = NSRange::new(0, text.encode_utf16().count());
        let _: () = msg_send![&*storage, addAttribute: &*font_key, value: &*font, range: range];
        let _: () = msg_send![&*storage, addAttribute: &*style_key, value: &*style, range: range];
        let layout: Retained<AnyObject> = msg_send![class!(NSLayoutManager), new];
        let _: () = msg_send![&*layout, setBackgroundLayoutEnabled: Bool::NO];
        let _: () = msg_send![&*layout, setAllowsNonContiguousLayout: Bool::NO];
        let _: () = msg_send![&*layout, setUsesFontLeading: Bool::NO];
        let container: Allocated<AnyObject> = msg_send![class!(NSTextContainer), alloc];
        let container: Retained<AnyObject> =
            msg_send![container, initWithContainerSize: NSSize::new(100., f64::MAX)];
        let _: () = msg_send![&*container, setLineFragmentPadding: 0_f64];
        let _: () = msg_send![&*container, setLineBreakMode: 2_usize];
        let _: () = msg_send![&*container, setMaximumNumberOfLines: 1_usize];
        let _: () = msg_send![&*layout, addTextContainer: &*container];
        let _: () = msg_send![&*storage, addLayoutManager: &*layout];
        let _: () = msg_send![&*layout, ensureLayoutForTextContainer: &*container];
        let fragment: NSRect = msg_send![&*layout, lineFragmentRectForGlyphAtIndex: 0_usize, effectiveRange: std::ptr::null_mut::<NSRange>(), withoutAdditionalLayout: Bool::YES];
        let glyphs: NSRange = msg_send![&*layout, glyphRangeForTextContainer: &*container];
        clipped_cluster_range(&layout, &container, glyphs, fragment)
    }
}

#[test]
fn clipped_ltr_and_rtl_source_coverage_matches_native_container_intersection() -> Result<()> {
    let system = system()?;
    for (text, direction, native_direction) in [
        (
            "AAAA bbbb cccc dddd eeee ffff GGGG",
            ParagraphDirection::LeftToRight,
            0_isize,
        ),
        ("אבגדהוזחטיכלמנסעפצקרשת", ParagraphDirection::RightToLeft, 1),
    ] {
        let runs = [TextRun {
            len: text.len(),
            ..Default::default()
        }];
        let request = system
            .attributed_paragraph_request(
                text,
                px(20.),
                &runs,
                ParagraphWidth::Bounded(px(100.)),
                direction,
            )?
            .with_line_limit(ParagraphLineLimit {
                max_lines: NonZeroUsize::new(1).ok_or("one line")?,
                mode: ParagraphEllipsize::Clip,
            });
        let ParagraphLayout::LaidOut(geometry) = system.layout_paragraph(request)? else {
            return Err("native layout".into());
        };
        assert!(geometry.truncated, "{direction:?}: {geometry:?}");
        let expected = clipped_bounds_oracle(text, native_direction);
        assert_eq!(geometry.lines[0].visible_source.len(), 1);
        let visible = &geometry.lines[0].visible_source[0];
        assert_eq!(utf16(text, visible.start), expected.location);
        assert_eq!(
            utf16(text, visible.end),
            expected.location + expected.length
        );
        assert!(visible.len() < text.len());
        assert!(
            geometry.lines[0]
                .glyphs
                .iter()
                .all(|glyph| glyph.origin == ParagraphGlyphOrigin::Source
                    && visible.contains(&glyph.glyph.index))
        );
        assert!(
            geometry.lines[0]
                .carets
                .as_ref()
                .ok_or("carets")?
                .primary
                .iter()
                .all(|caret| visible.start <= caret.index && caret.index <= visible.end)
        );
    }
    Ok(())
}

#[test]
fn attachment_frame_converts_native_baseline_anchor_to_flipped_top_left() -> Result<()> {
    let system = system()?;
    let text = "\u{fffc}";
    let runs = [TextRun {
        len: text.len(),
        ..Default::default()
    }];
    let request = system
        .attributed_paragraph_request(
            text,
            px(14.),
            &runs,
            ParagraphWidth::Bounded(px(160.)),
            ParagraphDirection::LeftToRight,
        )?
        .with_paragraph_styles(vec![ParagraphStyleSpan {
            source: 0..text.len(),
            direction: ParagraphDirection::LeftToRight,
            alignment: ParagraphAlignment::Left,
            line_height: ParagraphLineHeight::Exact(px(30.)),
            first_line_indent: px(0.),
            head_indent: px(0.),
            tail_indent: px(0.),
            space_before: px(0.),
            space_after: px(0.),
        }])?
        .with_attachments(vec![ParagraphAttachment {
            source: 0..text.len(),
            size: size(px(100.), px(24.)),
            baseline_offset: px(0.),
        }])?
        .with_line_limit(ParagraphLineLimit {
            max_lines: NonZeroUsize::new(1).ok_or("one line")?,
            mode: ParagraphEllipsize::Tail,
        });
    let ParagraphLayout::LaidOut(geometry) = system.layout_paragraph(request)? else {
        return Err("native layout".into());
    };
    assert_eq!(geometry.attachments.len(), 1);
    let attachment = &geometry.attachments[0];
    let line = &geometry.lines[0];
    assert_eq!(attachment.bounds.size, size(px(100.), px(24.)));
    assert_eq!(line.fragment_bounds.top(), px(0.));
    assert_eq!(line.fragment_bounds.bottom(), px(30.));
    assert_eq!(attachment.bounds.top(), px(-1.));
    assert_eq!(attachment.bounds.bottom(), px(23.));
    let native = standard_attachment_bounds(text, 0, 24.);
    assert_eq!(native.origin.y, -1.);
    assert_eq!(native.size.height, 31.);
    assert_eq!(f64::from(attachment.bounds.top()), native.origin.y);
    let short = standard_attachment_bounds(text, 0, 4.);
    assert_eq!(short.origin.y, 0.);
    assert_eq!(short.size.height, 30.);
    assert_eq!(attachment.source, 0..3);
    // SAFETY: The image and its standard AppKit attachment cell are retained on
    // this thread; the cell baseline is returned by value.
    unsafe {
        let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        let image: Retained<AnyObject> = msg_send![image, initWithSize: NSSize::new(100., 24.)];
        let cell: Allocated<AnyObject> = msg_send![class!(NSTextAttachmentCell), alloc];
        let cell: Retained<AnyObject> = msg_send![cell, initImageCell: &*image];
        let offset: objc2_foundation::NSPoint = msg_send![&*cell, cellBaselineOffset];
        assert_eq!(offset, objc2_foundation::NSPoint::new(0., 0.));
        let container: Allocated<AnyObject> = msg_send![class!(NSTextContainer), alloc];
        let container: Retained<AnyObject> =
            msg_send![container, initWithContainerSize: NSSize::new(160., 30.)];
        let fragment = NSRect::new(
            objc2_foundation::NSPoint::new(0., 0.),
            NSSize::new(160., 30.),
        );
        let position = objc2_foundation::NSPoint::new(0., 23.);
        let frame: NSRect = msg_send![&*cell, cellFrameForTextContainer: &*container, proposedLineFragment: fragment, glyphPosition: position, characterIndex: 0_usize];
        assert_eq!(
            frame,
            NSRect::new(
                objc2_foundation::NSPoint::new(0., 0.),
                NSSize::new(100., 24.)
            )
        );
        assert_eq!(attachment.bounds.origin, gpui::point(px(0.), px(-1.)));
    }
    Ok(())
}

#[test]
fn attachment_frame_retains_its_prefix_advance_and_second_visual_line() -> Result<()> {
    let system = system()?;
    let text = "aa\nx\u{fffc}";
    let runs = [TextRun {
        len: text.len(),
        ..Default::default()
    }];
    let request = system
        .attributed_paragraph_request(
            text,
            px(14.),
            &runs,
            ParagraphWidth::Bounded(px(160.)),
            ParagraphDirection::LeftToRight,
        )?
        .with_paragraph_styles(vec![ParagraphStyleSpan {
            source: 0..text.len(),
            direction: ParagraphDirection::LeftToRight,
            alignment: ParagraphAlignment::Left,
            line_height: ParagraphLineHeight::Exact(px(30.)),
            first_line_indent: px(0.),
            head_indent: px(0.),
            tail_indent: px(0.),
            space_before: px(0.),
            space_after: px(0.),
        }])?
        .with_attachments(vec![ParagraphAttachment {
            source: 4..7,
            size: size(px(20.), px(24.)),
            baseline_offset: px(0.),
        }])?;
    let ParagraphLayout::LaidOut(geometry) = system.layout_paragraph(request)? else {
        return Err("native layout".into());
    };
    assert_eq!(geometry.lines.len(), 2);
    assert_eq!(geometry.attachments.len(), 1);
    let line = &geometry.lines[1];
    let attachment = &geometry.attachments[0];
    let start = line
        .carets
        .as_ref()
        .ok_or("carets")?
        .primary
        .iter()
        .find(|caret| caret.index == 4)
        .ok_or("attachment caret")?;
    assert!(start.x > px(0.));
    assert_eq!(attachment.bounds.left(), start.x, "{geometry:?}");
    assert_eq!(line.fragment_bounds.top(), px(30.));
    assert_eq!(attachment.bounds.top(), px(26.));
    assert_eq!(attachment.bounds.bottom(), px(50.));
    assert_eq!(attachment.source, 4..7);
    Ok(())
}

fn standard_attachment_bounds(text: &str, index: usize, height: f64) -> NSRect {
    // SAFETY: The independent AppKit text storage, image cell, and native layout
    // graph remain retained until its container-coordinate bounds are copied.
    unsafe {
        let source = NSString::from_str(text);
        let storage: Allocated<AnyObject> = msg_send![class!(NSTextStorage), alloc];
        let storage: Retained<AnyObject> = msg_send![storage, initWithString: &*source];
        let range = NSRange::new(0, text.encode_utf16().count());
        let font: Retained<AnyObject> = msg_send![class!(NSFont), systemFontOfSize: 14_f64];
        let style: Retained<AnyObject> = msg_send![class!(NSMutableParagraphStyle), new];
        let _: () = msg_send![&*style, setBaseWritingDirection: 0_isize];
        let _: () = msg_send![&*style, setAlignment: 0_isize];
        let _: () = msg_send![&*style, setMinimumLineHeight: 30_f64];
        let _: () = msg_send![&*style, setMaximumLineHeight: 30_f64];
        let font_key = NSString::from_str("NSFont");
        let style_key = NSString::from_str("NSParagraphStyle");
        let _: () = msg_send![&*storage, addAttribute: &*font_key, value: &*font, range: range];
        let _: () = msg_send![&*storage, addAttribute: &*style_key, value: &*style, range: range];
        let ascent: f64 = msg_send![&*font, ascender];
        let descent: f64 = msg_send![&*font, descender];
        let offset: Retained<AnyObject> =
            msg_send![class!(NSNumber), numberWithDouble: ((30. - ascent + descent) / 2.).ceil()];
        let baseline_key = NSString::from_str("NSBaselineOffset");
        let _: () =
            msg_send![&*storage, addAttribute: &*baseline_key, value: &*offset, range: range];
        let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        let image: Retained<AnyObject> = msg_send![image, initWithSize: NSSize::new(100., height)];
        let cell: Allocated<AnyObject> = msg_send![class!(NSTextAttachmentCell), alloc];
        let cell: Retained<AnyObject> = msg_send![cell, initImageCell: &*image];
        let attachment: Retained<AnyObject> = msg_send![class!(NSTextAttachment), new];
        let _: () = msg_send![&*attachment, setAttachmentCell: &*cell];
        let attachment_key = NSString::from_str("NSAttachment");
        let _: () = msg_send![&*storage, addAttribute: &*attachment_key, value: &*attachment, range: NSRange::new(index, 1)];
        let layout: Retained<AnyObject> = msg_send![class!(NSLayoutManager), new];
        let _: () = msg_send![&*layout, setBackgroundLayoutEnabled: Bool::NO];
        let _: () = msg_send![&*layout, setAllowsNonContiguousLayout: Bool::NO];
        let _: () = msg_send![&*layout, setUsesFontLeading: Bool::NO];
        let container: Allocated<AnyObject> = msg_send![class!(NSTextContainer), alloc];
        let container: Retained<AnyObject> =
            msg_send![container, initWithContainerSize: NSSize::new(160., f64::MAX)];
        let _: () = msg_send![&*container, setLineFragmentPadding: 0_f64];
        let _: () = msg_send![&*layout, addTextContainer: &*container];
        let _: () = msg_send![&*storage, addLayoutManager: &*layout];
        let _: () = msg_send![&*layout, ensureLayoutForTextContainer: &*container];
        let glyph: usize = msg_send![&*layout, glyphIndexForCharacterAtIndex: index];
        msg_send![&*layout, boundingRectForGlyphRange: NSRange::new(glyph, 1), inTextContainer: &*container]
    }
}
