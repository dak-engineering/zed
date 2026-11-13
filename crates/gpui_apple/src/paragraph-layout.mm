#import <AppKit/AppKit.h>
#include "paragraph-layout.h"
#include <cmath>
#include <memory>
#include <algorithm>
#include <tuple>
#include <vector>

struct GlyphPosition {
    CGGlyph glyph;
    double x;
    double y;
    bool operator<(const GlyphPosition &other) const noexcept {
        return std::tie(y, x, glyph) < std::tie(other.y, other.x, other.glyph);
    }
    bool operator==(const GlyphPosition &other) const noexcept {
        return glyph == other.glyph && x == other.x && y == other.y;
    }
};
struct GPUITextKitLayout {
    GPUITextKitLine line{};
    std::vector<GPUITextKitGlyph> glyphs;
    std::vector<GPUITextKitRun> runs;
    std::vector<GPUITextKitCaret> carets;
    std::vector<GPUITextKitBidiRun> bidi;
    std::vector<GPUITextKitRect> selectionRects;
    void truncateGlyphs(size_t glyphCount, size_t runCount) {
        glyphs.resize(glyphCount);
        for (size_t index = runCount; index < runs.size(); ++index) CFRelease(runs[index].font);
        runs.resize(runCount);
    }
    bool emit(const GPUITextKitOptions *options) {
        if (!options->emit(options->context, this)) return false;
        truncateGlyphs(0, 0);
        carets.clear();
        bidi.clear();
        return true;
    }
    ~GPUITextKitLayout() { truncateGlyphs(0, 0); }

};

struct GlyphSlot {
    GlyphPosition position;
    NSUInteger character;
    bool used;
};

class GlyphSlots {
    std::vector<CGGlyph> glyphs;
    std::vector<NSUInteger> characters;
    std::vector<unsigned char> levels;
    std::vector<GlyphSlot> slots;
public:
    bool valid = true;

    void append(NSLayoutManager *layout, NSRange range, NSPoint origin, GPUITextKitLayout &output) {
        slots.clear();
        slots.reserve(range.length);
        glyphs.resize(range.length);
        characters.resize(range.length);
        levels.resize(range.length);
        if ([layout getGlyphsInRange:range glyphs:glyphs.data() properties:nullptr
            characterIndexes:characters.data() bidiLevels:levels.data()] != range.length) {
            valid = false;
            return;
        }
        auto &row = output.line;
        for (NSUInteger local = 0; local < range.length; ++local) {
            NSUInteger start = characters[local];
            NSUInteger end = local + 1 < range.length ? characters[local + 1] : row.start + row.length;
            if (end == start) {
                if (local + 1 == range.length || levels[local] != levels[local + 1]) { valid = false; return; }
            } else {
                if (start < row.start || end <= start || end > row.start + row.length
                    || (local == 0 && start != row.start)) { valid = false; return; }
                if (!output.bidi.empty() && output.bidi.back().level == levels[local]) {
                    output.bidi.back().length = end - output.bidi.back().start;
                } else {
                    output.bidi.push_back({start, end - start, levels[local]});
                }
            }
            NSUInteger index = range.location + local;
            if ([layout notShownAttributeForGlyphAtIndex:index]) continue;
            NSPoint location = [layout locationForGlyphAtIndex:index];
            GlyphPosition position{glyphs[local], location.x + origin.x, location.y + origin.y};
            if (!std::isfinite(position.x) || !std::isfinite(position.y)) { valid = false; return; }
            slots.push_back({position, characters[local], false});
        }
    }

    bool assign(GPUITextKitLayout *output) {
        if (output->glyphs.size() != slots.size()) return false;
        std::sort(slots.begin(), slots.end(), [](const GlyphSlot &left, const GlyphSlot &right) {
            return left.position < right.position;
        });
        for (size_t index = 1; index < slots.size(); ++index) {
            if (slots[index - 1].position == slots[index].position) return false;
        }
        for (size_t index = 0; index < output->glyphs.size(); ++index) {
            auto &glyph = output->glyphs[index];
            GlyphPosition position{static_cast<CGGlyph>(glyph.glyph), glyph.position[0], glyph.position[1]};
            if (!std::isfinite(position.x) || !std::isfinite(position.y)) return false;
            auto found = std::lower_bound(slots.begin(), slots.end(), position,
                [](const GlyphSlot &slot, const GlyphPosition &key) { return slot.position < key; });
            if (found == slots.end() || !(found->position == position) || found->used) return false;
            glyph.source = found->character;
            found->used = true;
        }
        return true;
    }

};

bool wantsCarets(const GPUITextKitCaretRequest &wanted, size_t start, size_t length) {
    switch (wanted.mode) {
        case GPUITextKitCaretMode::All: return true;
        case GPUITextKitCaretMode::None: return false;
        case GPUITextKitCaretMode::Source:
            return start <= wanted.start + wanted.length && wanted.start <= start + length;
    }
    return false;
}

class CaretPositions {
    std::vector<CGFloat> positions;
    std::vector<NSUInteger> indices;
public:
    bool append(NSLayoutManager *layout, GPUITextKitLine &line, CGFloat origin,
        std::vector<GPUITextKitCaret> &output) {
        positions.resize(line.length + 1);
        indices.resize(line.length + 1);
        for (bool alternate : {false, true}) {
            NSUInteger count = [layout getLineFragmentInsertionPointsForCharacterAtIndex:line.start
                alternatePositions:alternate inDisplayOrder:YES
                positions:positions.data() characterIndexes:indices.data()];
            if (count > positions.size()) return false;
            if (alternate) line.alternate_count = count;
            else line.primary_count = count;
            for (NSUInteger index = 0; index < count; ++index) {
                output.push_back({indices[index], positions[index] + origin});
            }
        }
        return true;
    }
};

NSMutableParagraphStyle *paragraphStyle(bool rtl, const GPUITextKitOptions &options) {
    const auto &height = options.lineHeight;
    if (!std::isfinite(height.height)) return nil;
    NSMutableParagraphStyle *style = [[NSMutableParagraphStyle alloc] init];
    style.baseWritingDirection = rtl ? NSWritingDirectionRightToLeft : NSWritingDirectionLeftToRight;
    style.lineBreakMode = NSLineBreakByWordWrapping;
    switch (height.mode) {
        case GPUITextKitLineHeightMode::Natural:
            if (height.height != 0) return nil;
            break;
        case GPUITextKitLineHeightMode::Exact:
            if (height.height <= 0) return nil;
            style.maximumLineHeight = height.height;
            [[fallthrough]];
        case GPUITextKitLineHeightMode::AtLeast:
            if (height.height <= 0) return nil;
            style.minimumLineHeight = height.height;
            break;
        default: return nil;
    }
    switch (options.alignment) {
        case GPUITextKitAlignment::Natural: style.alignment = NSTextAlignmentNatural; break;
        case GPUITextKitAlignment::Left: style.alignment = NSTextAlignmentLeft; break;
        case GPUITextKitAlignment::Center: style.alignment = NSTextAlignmentCenter; break;
        case GPUITextKitAlignment::Right: style.alignment = NSTextAlignmentRight; break;
        case GPUITextKitAlignment::Justified: style.alignment = NSTextAlignmentJustified; break;
        default: return nil;
    }
    return style;
}

NSMutableAttributedString *configuredSource(const uint16_t *text, size_t length,
    const GPUITextKitFontRun *runs, size_t count, NSParagraphStyle *style, GPUITextKitLineHeight height) {
    NSString *source = [[NSString alloc] initWithCharacters:text length:length];
    NSMutableAttributedString *attributed = [[NSMutableAttributedString alloc] initWithString:source];
    [attributed addAttribute:NSParagraphStyleAttributeName value:style range:NSMakeRange(0, length)];
    for (size_t index = 0; index < count; ++index) {
        const auto &run = runs[index];
        if (run.start > length || run.length > length - run.start || !run.font) return nil;
        NSRange range = NSMakeRange(run.start, run.length);
        [attributed addAttribute:NSFontAttributeName value:(__bridge NSFont *)run.font range:range];
        if (height.mode == GPUITextKitLineHeightMode::Exact) {
            CGFloat natural = CTFontGetAscent(run.font) + CTFontGetDescent(run.font);
            if (height.height >= natural) {
                CGFloat offset = std::ceil((height.height - natural) / 2);
                [attributed addAttribute:NSBaselineOffsetAttributeName value:@(offset) range:range];
            }
        }
    }
    return attributed;
}

@interface GPUITextKitCapture : NSLayoutManager
@property(nonatomic, assign) GPUITextKitLayout *capture;
@property(nonatomic, assign) NSUInteger sourceGlyph;
@property(nonatomic, assign) BOOL valid;
@property(nonatomic, strong) NSFont *emptyFont;
@property(nonatomic, assign) BOOL emptyRightToLeft;
@property(nonatomic, strong) NSParagraphStyle *emptyStyle;
@end

@implementation GPUITextKitCapture
- (void)setExtraLineFragmentRect:(NSRect)fragment usedRect:(NSRect)used textContainer:(NSTextContainer *)container {
    // An empty storage has no font attribute. Apply the requested typing metrics
    // while the typesetter commits its extra fragment, without creating a view.
    if (self.emptyFont && self.textStorage.length == 0) {
        CGFloat height = [self defaultLineHeightForFont:self.emptyFont];
        if (self.emptyStyle.maximumLineHeight > 0) height = self.emptyStyle.maximumLineHeight;
        else height = std::max(height, self.emptyStyle.minimumLineHeight);
        fragment.size.height = height;
        used.size.height = height;
        CGFloat x = NSMinX(fragment);
        if (container.containerSize.width > 0) {
            switch (self.emptyStyle.alignment) {
                case NSTextAlignmentLeft: break;
                case NSTextAlignmentCenter: x = NSMidX(fragment); break;
                case NSTextAlignmentRight: x = NSMaxX(fragment); break;
                default: if (self.emptyRightToLeft) x = NSMaxX(fragment); break;
            }
        }
        used.origin.x = x;
    }
    [super setExtraLineFragmentRect:fragment usedRect:used textContainer:container];
}
- (void)showCGGlyphs:(const CGGlyph *)glyphs positions:(const CGPoint *)positions
    count:(NSInteger)count font:(NSFont *)font textMatrix:(CGAffineTransform)matrix
    attributes:(NSDictionary<NSAttributedStringKey, id> *)attributes inContext:(CGContextRef)context {
    if (count <= 0) return;
    CTFontRef native = (__bridge CTFontRef)font;
    size_t run = self.capture->runs.size();
    self.capture->runs.push_back({native, {matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty}});
    CFRetain(native);
    for (NSInteger index = 0; index < count; ++index) {
        if (self.sourceGlyph != NSNotFound && glyphs[index] != [self CGGlyphAtIndex:self.sourceGlyph]) {
            self.valid = NO;
            return;
        }
        self.capture->glyphs.push_back({self.sourceGlyph == NSNotFound ? NSNotFound : [self characterIndexForGlyphAtIndex:self.sourceGlyph],
            glyphs[index], run, {positions[index].x, positions[index].y}});
    }
}
@end

bool captureRows(GPUITextKitCapture *layout, const GPUITextKitOptions &options, GPUITextKitLayout *output) {
    const auto &wanted = options.carets;
    CaretPositions carets;
    CGColorSpaceRef color = CGColorSpaceCreateDeviceRGB();
    CGContextRef context = CGBitmapContextCreate(nullptr, 1, 1, 8, 4, color, kCGImageAlphaPremultipliedLast);
    CGColorSpaceRelease(color);
    if (!context) return false;
    NSGraphicsContext *previous = NSGraphicsContext.currentContext;
    @try {
        NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithCGContext:context flipped:YES];
        GlyphSlots slots;
        NSUInteger total = layout.numberOfGlyphs;
        for (NSUInteger cursor = 0; cursor < total;) {
            NSRange range;
            NSRect fragment = [layout lineFragmentRectForGlyphAtIndex:cursor effectiveRange:&range withoutAdditionalLayout:YES];
            NSRect used = [layout lineFragmentUsedRectForGlyphAtIndex:cursor effectiveRange:nullptr withoutAdditionalLayout:YES];
            if (range.location != cursor || range.length == 0 || range.length > total - cursor) return false;
            NSRange sourceRange = [layout characterRangeForGlyphRange:range actualGlyphRange:nullptr];
            output->line = {sourceRange.location, sourceRange.length,
                {used.origin.x, used.origin.y, used.size.width, used.size.height},
                {fragment.origin.x, fragment.origin.y, fragment.size.width, fragment.size.height}, 0, 0, false};
            output->line.carets_available = wantsCarets(wanted, sourceRange.location, sourceRange.length);
            if (output->line.carets_available
                && !carets.append(layout, output->line, fragment.origin.x, output->carets)) return false;
            slots.append(layout, range, fragment.origin, *output);
            if (!slots.valid) return false;
            output->glyphs.reserve(range.length);
            layout.sourceGlyph = NSNotFound;
            [layout drawGlyphsForGlyphRange:range atPoint:NSZeroPoint];
            if (!slots.assign(output)) {
                output->truncateGlyphs(0, 0);
                for (NSUInteger index = range.location; index < NSMaxRange(range); ++index) {
                    if ([layout notShownAttributeForGlyphAtIndex:index]) continue;
                    layout.sourceGlyph = index;
                    [layout drawGlyphsForGlyphRange:NSMakeRange(index, 1) atPoint:NSZeroPoint];
                }
            }
            if (!layout.valid || !output->emit(&options)) return false;
            cursor = NSMaxRange(range);
        }
    } @finally {
        NSGraphicsContext.currentContext = previous;
        CGContextRelease(context);
    }
    if (!layout.valid) return false;
    return true;
}

GPUITextKitLayout *gpui_textkit_layout(const uint16_t *text, size_t length,
    const GPUITextKitFontRun *runs, size_t count, double width, bool rtl, const GPUITextKitOptions *options) {
    try {
        @autoreleasepool {
            @try {
                if (!std::isfinite(width) || width < 0 || (length == 0 && count == 0)) return nullptr;
                if (!options || !options->emit) return nullptr;
                const auto &wanted = options->carets;
                if (wanted.mode != GPUITextKitCaretMode::All && wanted.mode != GPUITextKitCaretMode::None
                    && wanted.mode != GPUITextKitCaretMode::Source) return nullptr;
                if (wanted.mode == GPUITextKitCaretMode::Source
                    && (wanted.start > length || wanted.length > length - wanted.start)) return nullptr;
                const auto *selection = options->selection;
                if (selection && (selection->start > length || selection->length > length - selection->start)) return nullptr;
                auto output = std::make_unique<GPUITextKitLayout>();
                NSMutableParagraphStyle *style = paragraphStyle(rtl, *options);
                if (!style) return nullptr;
                NSMutableAttributedString *attributed = configuredSource(text, length, runs, count, style, options->lineHeight);
                if (!attributed) return nullptr;
                NSString *source = attributed.string;
                NSTextStorage *storage = [[NSTextStorage alloc] initWithAttributedString:attributed];
                if (![storage.string isEqualToString:source]) return nullptr;
                GPUITextKitCapture *layout = [[GPUITextKitCapture alloc] init];
                layout.capture = output.get();
                layout.valid = YES;
                if (length == 0) layout.emptyFont = (__bridge NSFont *)runs[count - 1].font;
                layout.emptyRightToLeft = rtl;
                layout.emptyStyle = style;
                layout.backgroundLayoutEnabled = NO;
                layout.allowsNonContiguousLayout = NO;
                layout.usesFontLeading = NO;
                NSTextContainer *container = [[NSTextContainer alloc] initWithContainerSize:NSMakeSize(width, CGFLOAT_MAX)];
                container.lineFragmentPadding = 0;
                container.lineBreakMode = NSLineBreakByWordWrapping;
                [layout addTextContainer:container];
                [storage addLayoutManager:layout];
                [layout ensureLayoutForTextContainer:container];
                if (!captureRows(layout, *options, output.get())) return nullptr;
                if (layout.extraLineFragmentTextContainer == container) {
                    NSRect fragment = layout.extraLineFragmentRect;
                    NSRect used = layout.extraLineFragmentUsedRect;
                    bool available = wantsCarets(wanted, length, 0);
                    if (available) output->carets.push_back({length, used.origin.x});
                    output->line = {length, 0,
                        {used.origin.x, used.origin.y, used.size.width, used.size.height},
                        {fragment.origin.x, fragment.origin.y, fragment.size.width, fragment.size.height}, size_t(available), 0, available};
                    if (!output->emit(options)) return nullptr;
                }
                if (selection && selection->length > 0) {
                    NSRange selected = NSMakeRange(selection->start, selection->length);
                    NSUInteger count = 0;
                    // Character rectangles preserve partial ligatures. Glyph-only enumeration
                    // expands those selections, and per-character unions lose selection context.
                    NSRectArray rectangles = [layout rectArrayForCharacterRange:selected
                        withinSelectedCharacterRange:selected inTextContainer:container rectCount:&count];
                    if (count && !rectangles) return nullptr;
                    output->selectionRects.reserve(count);
                    // The layout manager reuses this buffer; copy before any further native query.
                    for (NSUInteger index = 0; index < count; ++index) {
                        NSRect value = rectangles[index];
                        output->selectionRects.push_back({{value.origin.x, value.origin.y, value.size.width, value.size.height}});
                    }
                }
                return output.release();
            } @catch (NSException *exception) {
                return nullptr;
            }
        }
    } catch (...) {
        return nullptr;
    }
}
void gpui_textkit_layout_drop(GPUITextKitLayout *layout) { delete layout; }
const GPUITextKitLine *gpui_textkit_line(const GPUITextKitLayout *layout) {
    return &layout->line;
}
const GPUITextKitGlyph *gpui_textkit_glyphs(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->glyphs.size();
    return layout->glyphs.data();
}

const GPUITextKitRun *gpui_textkit_runs(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->runs.size();
    return layout->runs.data();
}
const GPUITextKitCaret *gpui_textkit_carets(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->carets.size();
    return layout->carets.data();
}

const GPUITextKitBidiRun *gpui_textkit_bidi_runs(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->bidi.size();
    return layout->bidi.data();
}

const GPUITextKitRect *gpui_textkit_selection_rects(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->selectionRects.size();
    return layout->selectionRects.data();
}
