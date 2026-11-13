use crate::AppleTextSystem;
use gpui::{
    Font, ParagraphLayout, ParagraphWidth, PlatformTextSystem as _, TextRun, TextSystem,
    WindowTextSystem, px,
};
use std::sync::Arc;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn system() -> Result<WindowTextSystem> {
    let backend = Arc::new(AppleTextSystem::new());
    backend.font_id(&Font::default())?;
    Ok(WindowTextSystem::new(Arc::new(TextSystem::new(backend))))
}
use gpui::{ParagraphDirection, ResolvedParagraphDirection};
use objc2::{
    class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSRange, NSRect, NSSize, NSString};

struct OracleLine {
    source: NSRange,
    used: NSRect,
    fragment: NSRect,
    carets: Vec<(usize, f64)>,
}

fn oracle(text: &str, direction: isize) -> Vec<OracleLine> {
    // SAFETY: All objects are owned on this thread and retained throughout layout.
    // Selectors and scalar/structure encodings match the macOS SDK declarations.
    unsafe {
        let source = NSString::from_str(text);
        let storage: Allocated<AnyObject> = msg_send![class!(NSTextStorage), alloc];
        let storage: Retained<AnyObject> = msg_send![storage, initWithString: &*source];
        let style: Retained<AnyObject> = msg_send![class!(NSMutableParagraphStyle), new];
        let _: () = msg_send![&*style, setBaseWritingDirection: direction];
        let _: () = msg_send![&*style, setAlignment: 4_isize];
        let font: Retained<AnyObject> = msg_send![class!(NSFont), systemFontOfSize: 20_f64];
        let font_key = NSString::from_str("NSFont");
        let paragraph_key = NSString::from_str("NSParagraphStyle");
        let range = NSRange::new(0, text.encode_utf16().count());
        let _: () = msg_send![&*storage, addAttribute: &*font_key, value: &*font, range: range];
        let _: () =
            msg_send![&*storage, addAttribute: &*paragraph_key, value: &*style, range: range];
        let layout: Retained<AnyObject> = msg_send![class!(NSLayoutManager), new];
        let _: () = msg_send![&*layout, setBackgroundLayoutEnabled: Bool::NO];
        let _: () = msg_send![&*layout, setAllowsNonContiguousLayout: Bool::NO];
        let _: () = msg_send![&*layout, setUsesFontLeading: Bool::NO];
        let container: Allocated<AnyObject> = msg_send![class!(NSTextContainer), alloc];
        let container: Retained<AnyObject> =
            msg_send![container, initWithContainerSize: NSSize::new(120., f64::MAX)];
        let _: () = msg_send![&*container, setLineFragmentPadding: 0_f64];
        let _: () = msg_send![&*container, setLineBreakMode: 0_usize];
        let _: () = msg_send![&*layout, addTextContainer: &*container];
        let _: () = msg_send![&*storage, addLayoutManager: &*layout];
        let _: () = msg_send![&*layout, ensureLayoutForTextContainer: &*container];
        oracle_lines(&layout)
    }
}

fn oracle_lines(layout: &AnyObject) -> Vec<OracleLine> {
    // SAFETY: The caller keeps the layout/storage/container graph alive and fully
    // laid out. TextKit reports the required array length before filling buffers.
    unsafe {
        let total: usize = msg_send![layout, numberOfGlyphs];
        let mut output = Vec::new();
        let mut cursor = 0;
        while cursor < total {
            let mut range = NSRange::new(0, 0);
            let fragment: NSRect = msg_send![layout, lineFragmentRectForGlyphAtIndex: cursor, effectiveRange: &mut range, withoutAdditionalLayout: Bool::YES];
            let used: NSRect = msg_send![layout, lineFragmentUsedRectForGlyphAtIndex: cursor, effectiveRange: std::ptr::null_mut::<NSRange>(), withoutAdditionalLayout: Bool::YES];
            let source: NSRange = msg_send![layout, characterRangeForGlyphRange: range, actualGlyphRange: std::ptr::null_mut::<NSRange>()];
            assert!(range.length > 0);
            assert_eq!(range.location, cursor);
            let required: usize = msg_send![layout, getLineFragmentInsertionPointsForCharacterAtIndex: source.location, alternatePositions: Bool::NO, inDisplayOrder: Bool::YES, positions: std::ptr::null_mut::<f64>(), characterIndexes: std::ptr::null_mut::<usize>()];
            let mut positions = vec![0_f64; required];
            let mut indices = vec![0_usize; required];
            let count: usize = msg_send![layout, getLineFragmentInsertionPointsForCharacterAtIndex: source.location, alternatePositions: Bool::NO, inDisplayOrder: Bool::YES, positions: positions.as_mut_ptr(), characterIndexes: indices.as_mut_ptr()];
            assert!(count <= positions.len());
            let carets = indices
                .into_iter()
                .zip(positions)
                .take(count)
                .map(|(index, x)| (index, x + fragment.origin.x))
                .collect();
            output.push(OracleLine {
                source,
                used,
                fragment,
                carets,
            });
            cursor += range.length;
        }
        output
    }
}

fn utf16_index(text: &str, index: usize) -> usize {
    text[..index].encode_utf16().count()
}

fn compare(text: &str, direction: ParagraphDirection, native_direction: isize) -> Result<()> {
    let system = system()?;
    let runs = [TextRun {
        len: text.len(),
        ..TextRun::default()
    }];
    let request = system.paragraph_request(
        text,
        px(20.),
        &runs,
        ParagraphWidth::Bounded(px(120.)),
        direction,
    )?;
    let ParagraphLayout::LaidOut(actual) = system.layout_paragraph(request)? else {
        return Err("native layout unavailable".into());
    };
    let expected = oracle(text, native_direction);
    assert_eq!(actual.lines.len(), expected.len(), "{text:?} {direction:?}");
    for (line, expected) in actual.lines.iter().zip(expected) {
        let resolved = match direction {
            ParagraphDirection::Natural => None,
            ParagraphDirection::LeftToRight => Some(ResolvedParagraphDirection::LeftToRight),
            ParagraphDirection::RightToLeft => Some(ResolvedParagraphDirection::RightToLeft),
        };
        assert_eq!(line.base_direction, resolved);
        assert_eq!(
            utf16_index(text, line.source.start),
            expected.source.location
        );
        assert_eq!(
            utf16_index(text, line.source.end),
            expected.source.location + expected.source.length
        );
        for (actual, expected) in [
            (line.bounds.left(), expected.used.origin.x),
            (line.bounds.top(), expected.used.origin.y),
            (line.bounds.size.width, expected.used.size.width),
            (line.bounds.size.height, expected.used.size.height),
            (
                line.fragment_bounds.size.width,
                expected.fragment.size.width,
            ),
        ] {
            assert!(
                (f64::from(actual) - expected).abs() < 0.001,
                "{text:?} {direction:?}: {actual:?} != {expected}"
            );
        }
        let carets = &line.carets.as_ref().ok_or("native carets missing")?.primary;
        assert_eq!(carets.len(), expected.carets.len());
        for (actual, (source, x)) in carets.iter().zip(expected.carets) {
            assert_eq!(utf16_index(text, actual.index), source);
            assert!((f64::from(actual.x) - x).abs() < 0.001);
        }
    }
    Ok(())
}

#[test]
fn natural_paragraph_geometry_and_carets_match_independent_view_free_textkit() -> Result<()> {
    for text in [
        "abc אב",
        "אב abc",
        "אבג",
        "123 ---",
        "a אבג def ghi jkl mno",
        "\u{200f}abc",
        "\u{202b}abc\u{202c}",
        "\u{2067}abc\u{2069}",
        "אב\nabc",
    ] {
        compare(text, ParagraphDirection::Natural, -1)?;
    }
    Ok(())
}

#[test]
fn explicit_paragraph_direction_keeps_native_geometry_and_reliable_base_metadata() -> Result<()> {
    for text in [
        "abc אב",
        "אב abc",
        "123 ---",
        "\u{200f}abc",
        "\u{202b}abc\u{202c}",
    ] {
        compare(text, ParagraphDirection::LeftToRight, 0)?;
        compare(text, ParagraphDirection::RightToLeft, 1)?;
    }
    Ok(())
}
