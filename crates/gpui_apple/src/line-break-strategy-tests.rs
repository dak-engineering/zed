use crate::AppleTextSystem;
use gpui::{
    Font, ParagraphDirection, ParagraphLayout, ParagraphLineBreakStrategy, ParagraphWidth,
    PlatformTextSystem as _, TextRun, TextSystem, WindowTextSystem, px,
};
use objc2::{
    class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSRange, NSSize, NSString};
use std::{ops::Range, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn oracle(text: &str, width: f64, strategy: usize) -> Vec<Range<usize>> {
    // SAFETY: Retained objects keep the storage/layout/container graph alive on this
    // thread; selector argument and result encodings match the macOS SDK.
    unsafe {
        let source = NSString::from_str(text);
        let storage: Allocated<AnyObject> = msg_send![class!(NSTextStorage), alloc];
        let storage: Retained<AnyObject> = msg_send![storage, initWithString: &*source];
        let style: Retained<AnyObject> = msg_send![class!(NSMutableParagraphStyle), new];
        let _: () = msg_send![&*style, setBaseWritingDirection: 0_isize];
        let _: () = msg_send![&*style, setAlignment: 0_isize];
        let _: () = msg_send![&*style, setLineBreakStrategy: strategy];
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
            msg_send![container, initWithContainerSize: NSSize::new(width, f64::MAX)];
        let _: () = msg_send![&*container, setLineFragmentPadding: 0_f64];
        let _: () = msg_send![&*container, setLineBreakMode: 0_usize];
        let _: () = msg_send![&*layout, addTextContainer: &*container];
        let _: () = msg_send![&*storage, addLayoutManager: &*layout];
        let _: () = msg_send![&*layout, ensureLayoutForTextContainer: &*container];
        let total: usize = msg_send![&*layout, numberOfGlyphs];
        let mut output = Vec::new();
        let mut cursor = 0;
        while cursor < total {
            let mut glyphs = NSRange::new(0, 0);
            let _: objc2_foundation::NSRect = msg_send![&*layout, lineFragmentRectForGlyphAtIndex: cursor, effectiveRange: &mut glyphs, withoutAdditionalLayout: Bool::YES];
            let source: NSRange = msg_send![&*layout, characterRangeForGlyphRange: glyphs, actualGlyphRange: std::ptr::null_mut::<NSRange>()];
            assert!(glyphs.length > 0);
            output.push(source.location..source.location + source.length);
            cursor += glyphs.length;
        }
        output
    }
}

#[test]
fn native_line_break_strategies_match_independent_textkit_fitting() -> Result<()> {
    let backend = Arc::new(AppleTextSystem::new());
    backend.font_id(&Font::default())?;
    let system = WindowTextSystem::new(Arc::new(TextSystem::new(backend)));
    let mut changed = false;
    for text in [
        "한국어 단어를 중간에서 나누지 않고 자연스럽게 줄을 바꿉니다",
        "A short sentence with a tiny orphan at its end.",
        "😀 한국어와 English words together in one paragraph",
    ] {
        for width in [80_f32, 120., 160.] {
            let baseline = oracle(text, f64::from(width), 0);
            for (strategy, native) in [
                (ParagraphLineBreakStrategy::None, 0),
                (ParagraphLineBreakStrategy::Standard, 0xffff),
                (ParagraphLineBreakStrategy::HangulWord, 2),
                (ParagraphLineBreakStrategy::PushOut, 1),
            ] {
                let expected = oracle(text, f64::from(width), native);
                changed |= expected != baseline;
                let runs = [TextRun {
                    len: text.len(),
                    ..Default::default()
                }];
                let request = system
                    .paragraph_request(
                        text,
                        px(20.),
                        &runs,
                        ParagraphWidth::Bounded(px(width)),
                        ParagraphDirection::LeftToRight,
                    )?
                    .with_line_break_strategy(strategy)
                    .with_selection(0..text.len())?;
                assert_eq!(request.line_break_strategy(), strategy);
                let ParagraphLayout::LaidOut(actual) = system.layout_paragraph(request)? else {
                    return Err("native strategy layout unavailable".into());
                };
                let ranges: Vec<_> = actual
                    .lines
                    .iter()
                    .map(|line| {
                        text[..line.source.start].encode_utf16().count()
                            ..text[..line.source.end].encode_utf16().count()
                    })
                    .collect();
                assert_eq!(ranges, expected, "{strategy:?} {width} {text:?}");
                assert!(!actual.selection_rects.is_empty());
            }
        }
    }
    assert!(
        changed,
        "oracle fixtures must distinguish at least one strategy from None"
    );
    Ok(())
}
