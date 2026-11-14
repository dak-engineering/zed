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
    std::vector<GPUITextKitSelection> visible;
    std::vector<GPUITextKitSelection> allVisible;
    std::vector<GPUITextKitAttachmentRect> attachments;
    bool truncated = false;
    size_t emitted = 0;
    void truncateGlyphs(size_t glyphCount, size_t runCount) {
        glyphs.resize(glyphCount);
        for (size_t index = runCount; index < runs.size(); ++index) CFRelease(runs[index].font);
        runs.resize(runCount);
    }
    bool emit(const GPUITextKitOptions *options) {
        if (!options->emit(options->context, this)) return false;
        allVisible.insert(allVisible.end(), visible.begin(), visible.end());
        ++emitted;
        visible.clear();
        attachments.clear();
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
            if ([layout notShownAttributeForGlyphAtIndex:index]
                || [layout.textStorage attribute:NSAttachmentAttributeName atIndex:characters[local] effectiveRange:nullptr] != nil) continue;
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

NSMutableParagraphStyle *paragraphStyle(GPUITextKitDirection direction, const GPUITextKitOptions &options) {
    const auto &height = options.lineHeight;
    if (!std::isfinite(height.height)) return nil;
    NSMutableParagraphStyle *style = [[NSMutableParagraphStyle alloc] init];
    switch (direction) {
        case GPUITextKitDirection::Natural: style.baseWritingDirection = NSWritingDirectionNatural; break;
        case GPUITextKitDirection::LeftToRight: style.baseWritingDirection = NSWritingDirectionLeftToRight; break;
        case GPUITextKitDirection::RightToLeft: style.baseWritingDirection = NSWritingDirectionRightToLeft; break;
        default: return nil;
    }
    style.lineBreakMode = NSLineBreakByWordWrapping;
    switch (options.lineBreakStrategy) {
        case GPUITextKitLineBreakStrategy::None: style.lineBreakStrategy = NSLineBreakStrategyNone; break;
        case GPUITextKitLineBreakStrategy::Standard:
            if (@available(macOS 11.0, *)) style.lineBreakStrategy = NSLineBreakStrategyStandard;
            else return nil;
            break;
        case GPUITextKitLineBreakStrategy::HangulWord:
            if (@available(macOS 11.0, *)) style.lineBreakStrategy = NSLineBreakStrategyHangulWordPriority;
            else return nil;
            break;
        case GPUITextKitLineBreakStrategy::PushOut: style.lineBreakStrategy = NSLineBreakStrategyPushOut; break;
        default: return nil;
    }
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
        CGFloat offset = run.baselineOffset;
        if (height.mode == GPUITextKitLineHeightMode::Exact) {
            CGFloat natural = CTFontGetAscent(run.font) + CTFontGetDescent(run.font);
            if (height.height >= natural) offset += std::ceil((height.height - natural) / 2);
        }
        if (offset != 0) [attributed addAttribute:NSBaselineOffsetAttributeName value:@(offset) range:range];
    }
    return attributed;
}

@interface GPUITextKitObjectCell : NSTextAttachmentCell
@property(nonatomic, assign) NSSize objectSize;
@property(nonatomic, assign) CGFloat objectBaseline;
@end
@implementation GPUITextKitObjectCell
- (NSSize)cellSize { return self.objectSize; }
- (NSPoint)cellBaselineOffset { return NSMakePoint(0, self.objectBaseline); }
@end

bool configureDocument(NSMutableAttributedString *source, const GPUITextKitOptions &options,
    const GPUITextKitFontRun *runs, size_t count) {
    for (size_t index = 0; index < options.paragraphCount; ++index) {
        const auto &span = options.paragraphs[index];
        if (span.start > source.length || span.length > source.length - span.start) return false;
        GPUITextKitOptions local = options;
        local.lineHeight = span.lineHeight;
        local.alignment = span.alignment;
        NSMutableParagraphStyle *style = paragraphStyle(span.direction, local);
        if (!style) return false;
        style.firstLineHeadIndent = span.firstLineIndent;
        style.headIndent = span.headIndent;
        style.tailIndent = span.tailIndent;
        style.paragraphSpacingBefore = span.spaceBefore;
        style.paragraphSpacing = span.spaceAfter;
        NSRange range = NSMakeRange(span.start, span.length);
        [source addAttribute:NSParagraphStyleAttributeName value:style range:range];
        [source removeAttribute:NSBaselineOffsetAttributeName range:range];
        // The font-run baseline attribute is reapplied below after each span's
        // height policy is known, preventing global exact-height offsets leaking.

    }
    for (size_t index = 0; index < count; ++index) {
        const auto &run = runs[index];
        NSRange range = NSMakeRange(run.start, run.length);
        [source enumerateAttribute:NSParagraphStyleAttributeName inRange:range options:0
            usingBlock:^(id value, NSRange segment, BOOL *) {
            NSParagraphStyle *style = value;
            CGFloat offset = run.baselineOffset;
            CGFloat natural = CTFontGetAscent(run.font) + CTFontGetDescent(run.font);
            if (style.maximumLineHeight > 0 && style.maximumLineHeight >= natural)
                offset += std::ceil((style.maximumLineHeight - natural) / 2);
            if (offset != 0) [source addAttribute:NSBaselineOffsetAttributeName value:@(offset) range:segment];
        }];
    }
    for (size_t index = 0; index < options.attachmentCount; ++index) {
        const auto &object = options.attachments[index];
        if (object.start >= source.length || object.length != 1
            || [source.string characterAtIndex:object.start] != NSAttachmentCharacter) return false;
        GPUITextKitObjectCell *cell = [[GPUITextKitObjectCell alloc] init];
        cell.objectSize = NSMakeSize(object.width, object.height);
        cell.objectBaseline = object.baselineOffset;
        NSTextAttachment *attachment = [[NSTextAttachment alloc] init];
        attachment.attachmentCell = cell;
        attachment.bounds = NSMakeRect(0, object.baselineOffset, object.width, object.height);
        [source addAttribute:NSAttachmentAttributeName value:attachment
            range:NSMakeRange(object.start, object.length)];
    }
    return true;
}

NSLineBreakMode breakMode(const GPUITextKitOptions &options) {
    if (!options.maxLines) return NSLineBreakByWordWrapping;
    switch (options.ellipsize) {
        case GPUITextKitEllipsize::Head: return NSLineBreakByTruncatingHead;
        case GPUITextKitEllipsize::Middle: return NSLineBreakByTruncatingMiddle;
        case GPUITextKitEllipsize::Tail: return NSLineBreakByTruncatingTail;
        case GPUITextKitEllipsize::Clip: return NSLineBreakByClipping;
    }
    return NSLineBreakByWordWrapping;
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
                default:
                    if (self.emptyStyle.baseWritingDirection == NSWritingDirectionNatural) x = used.origin.x;
                    else if (self.emptyRightToLeft) x = NSMaxX(fragment);
                    break;
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
            glyphs[index], run, {positions[index].x, positions[index].y}, false});
    }
}
- (void)showAttachmentCell:(NSCell *)cell inRect:(NSRect)rect characterIndex:(NSUInteger)index {
    rect.origin.y -= rect.size.height;
    self.capture->attachments.push_back({index, 1, {rect.origin.x, rect.origin.y, rect.size.width, rect.size.height}});
}
@end

bool visibleAt(const GPUITextKitLayout &output, NSUInteger index) {
    for (const auto &range : output.visible) {
        if (range.start <= index && index <= range.start + range.length) return true;
    }
    return false;
}

void filterCarets(GPUITextKitLayout &output) {
    auto old = std::move(output.carets);
    size_t primary = output.line.primary_count;
    output.line.primary_count = output.line.alternate_count = 0;
    for (size_t index = 0; index < old.size(); ++index) {
        if (!visibleAt(output, old[index].source)) continue;
        output.carets.push_back(old[index]);
        if (index < primary) ++output.line.primary_count;
        else ++output.line.alternate_count;
    }
}

bool sameDrawing(const GPUITextKitGlyph &left, const GPUITextKitLayout &a,
    const GPUITextKitGlyph &right, const GPUITextKitLayout &b) {
    const auto &first = a.runs[left.run];
    const auto &second = b.runs[right.run];
    return left.glyph == right.glyph && left.position[0] == right.position[0]
        && left.position[1] == right.position[1] && CFEqual(first.font, second.font)
        && std::equal(first.matrix, first.matrix + 6, second.matrix);
}

bool captureTruncated(GPUITextKitCapture *layout, NSRange row, NSRange hidden,
    GPUITextKitLayout &output, const GPUITextKitOptions &options) {
    GPUITextKitLayout drawn;
    drawn.glyphs.swap(output.glyphs);
    drawn.runs.swap(output.runs);
    output.attachments.clear();
    for (NSUInteger index = row.location; index < NSMaxRange(row); ++index) {
        if (NSLocationInRange(index, hidden) || [layout notShownAttributeForGlyphAtIndex:index]) continue;
        layout.sourceGlyph = index;
        [layout drawGlyphsForGlyphRange:NSMakeRange(index, 1) atPoint:NSZeroPoint];
    }
    if (!layout.valid) return false;
    std::vector<bool> matched(drawn.glyphs.size(), false);
    for (const auto &glyph : output.glyphs) {
        size_t found = 0;
        while (found < drawn.glyphs.size()
            && (matched[found] || !sameDrawing(glyph, output, drawn.glyphs[found], drawn))) ++found;
        if (found == drawn.glyphs.size()) return false;
        matched[found] = true;
        drawn.glyphs[found].source = glyph.source;
    }
    NSRange missing = [layout characterRangeForGlyphRange:hidden actualGlyphRange:nullptr];
    for (size_t index = 0; index < drawn.glyphs.size(); ++index) {
        if (matched[index]) continue;
        if (options.ellipsize == GPUITextKitEllipsize::Clip) return false;
        drawn.glyphs[index].source = missing.location;
        drawn.glyphs[index].synthetic = true;
    }
    output.truncateGlyphs(0, 0);
    output.glyphs.swap(drawn.glyphs);
    output.runs.swap(drawn.runs);
    return true;
}

bool nativeEllipsisAt(GPUITextKitCapture *layout, NSUInteger index, NSPoint origin,
    const GPUITextKitLayout &output) {
    CGGlyph identifier = [layout CGGlyphAtIndex:index];
    NSPoint location = [layout locationForGlyphAtIndex:index];
    for (const auto &glyph : output.glyphs) {
        if (glyph.glyph != identifier || glyph.position[0] != location.x + origin.x
            || glyph.position[1] != location.y + origin.y) continue;
        UniChar character = 0x2026;
        CGGlyph replacement = 0;
        if (CTFontGetGlyphsForCharacters(output.runs[glyph.run].font, &character, &replacement, 1)
            && replacement == identifier) return true;
    }
    return false;
}

bool visibleSourceInNullSpan(GPUITextKitCapture *layout, NSRange glyphs) {
    NSRange source = [layout characterRangeForGlyphRange:glyphs actualGlyphRange:nullptr];
    NSCharacterSet *controls = NSCharacterSet.controlCharacterSet;
    for (NSUInteger index = source.location; index < NSMaxRange(source); ++index) {
        if (![controls characterIsMember:[layout.textStorage.string characterAtIndex:index]]) return true;
    }
    return false;
}

bool omittedWithoutNativeRange(GPUITextKitCapture *layout, NSRange row, NSPoint origin,
    const GPUITextKitLayout &output, NSRange &hidden) {
    for (NSUInteger index = row.location; index < NSMaxRange(row);) {
        if (!([layout propertyForGlyphAtIndex:index] & NSGlyphPropertyNull)
            || ![layout notShownAttributeForGlyphAtIndex:index]) { ++index; continue; }
        NSUInteger start = index++;
        while (index < NSMaxRange(row) && ([layout propertyForGlyphAtIndex:index] & NSGlyphPropertyNull)
            && [layout notShownAttributeForGlyphAtIndex:index]) ++index;
        if (!visibleSourceInNullSpan(layout, NSMakeRange(start, index - start))) continue;
        NSUInteger replacement = NSNotFound;
        for (NSUInteger candidate : {start > row.location ? start - 1 : NSNotFound,
            index < NSMaxRange(row) ? index : NSNotFound}) {
            if (candidate == NSNotFound || [layout notShownAttributeForGlyphAtIndex:candidate]
                || !nativeEllipsisAt(layout, candidate, origin, output)) continue;
            if (replacement != NSNotFound) return false;
            replacement = candidate;
        }
        if (replacement == NSNotFound) continue;
        if (hidden.location != NSNotFound) return false;
        NSUInteger first = std::min(start, replacement);
        hidden = NSMakeRange(first, std::max(index, replacement + 1) - first);
    }
    return true;
}

void clippedSource(GPUITextKitCapture *layout, NSTextContainer *container, NSRange row,
    NSRange source, NSRect fragment, NSRect used, GPUITextKitLayout &output) {
    if (used.size.width < fragment.size.width) {
        output.visible.push_back({source.location, source.length});
        return;
    }
    NSRange candidates = NSIntersectionRange(row,
        [layout glyphRangeForBoundingRect:fragment inTextContainer:container]);
    for (NSUInteger index = candidates.location; index < NSMaxRange(candidates);) {
        NSRange cluster;
        NSRange characters = [layout characterRangeForGlyphRange:NSMakeRange(index, 1) actualGlyphRange:&cluster];
        cluster = NSIntersectionRange(row, cluster);
        NSRect bounds = [layout boundingRectForGlyphRange:cluster inTextContainer:container];
        if (NSIntersectsRect(fragment, bounds)) {
            characters = NSIntersectionRange(source, characters);
            NSCharacterSet *controls = NSCharacterSet.controlCharacterSet;
            while (characters.location > source.location
                && [controls characterIsMember:[layout.textStorage.string characterAtIndex:characters.location - 1]]) {
                --characters.location;
                ++characters.length;
            }
            while (NSMaxRange(characters) < NSMaxRange(source)
                && [controls characterIsMember:[layout.textStorage.string characterAtIndex:NSMaxRange(characters)]])
                ++characters.length;
            if (characters.length) output.visible.push_back({characters.location, characters.length});
        }
        index = std::max(index + 1, NSMaxRange(cluster));
    }
    std::sort(output.visible.begin(), output.visible.end(), [](const auto &left, const auto &right) {
        return left.start < right.start;
    });
    size_t merged = 0;
    for (const auto &range : output.visible) {
        if (merged && range.start <= output.visible[merged - 1].start + output.visible[merged - 1].length) {
            auto &last = output.visible[merged - 1];
            last.length = std::max(last.start + last.length, range.start + range.length) - last.start;
        } else output.visible[merged++] = range;
    }
    output.visible.resize(merged);
    if (output.visible.size() != 1 || output.visible[0].start != source.location
        || output.visible[0].length != source.length) output.truncated = true;
}

void filterClippedDrawing(NSRect fragment, GPUITextKitLayout &output) {
    auto visible = [&](size_t index) {
        for (const auto &range : output.visible)
            if (range.start <= index && index < range.start + range.length) return true;
        return false;
    };
    output.glyphs.erase(std::remove_if(output.glyphs.begin(), output.glyphs.end(),
        [&](const auto &glyph) { return !visible(glyph.source); }), output.glyphs.end());
    output.attachments.erase(std::remove_if(output.attachments.begin(), output.attachments.end(),
        [&](const auto &object) {
            NSRect bounds = NSMakeRect(object.bounds[0], object.bounds[1], object.bounds[2], object.bounds[3]);
            return !visible(object.start) || !NSIntersectsRect(fragment, bounds);
        }), output.attachments.end());
}

bool captureRows(GPUITextKitCapture *layout, NSTextContainer *container, const GPUITextKitOptions &options,
    GPUITextKitLayout *output) {
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
        NSUInteger total = options.maxLines ? NSMaxRange([layout glyphRangeForTextContainer:container]) : layout.numberOfGlyphs;
        for (NSUInteger cursor = 0; cursor < total;) {
            NSRange range;
            NSRect fragment = [layout lineFragmentRectForGlyphAtIndex:cursor effectiveRange:&range withoutAdditionalLayout:YES];
            NSRect used = [layout lineFragmentUsedRectForGlyphAtIndex:cursor effectiveRange:nullptr withoutAdditionalLayout:YES];
            if (range.location != cursor || range.length == 0 || range.length > total - cursor) return false;
            NSRange sourceRange = [layout characterRangeForGlyphRange:range actualGlyphRange:nullptr];
            output->line = {sourceRange.location, sourceRange.length,
                {used.origin.x, used.origin.y, used.size.width, used.size.height},
                {fragment.origin.x, fragment.origin.y, fragment.size.width, fragment.size.height}, 0, 0, false, 0};
            NSRange hidden = options.maxLines
                ? [layout truncatedGlyphRangeInLineFragmentForGlyphAtIndex:cursor] : NSMakeRange(NSNotFound, 0);
            NSParagraphStyle *resolved = [layout.textStorage attribute:NSParagraphStyleAttributeName
                atIndex:sourceRange.location effectiveRange:nullptr];
            output->line.base_direction = resolved.baseWritingDirection == NSWritingDirectionLeftToRight ? 1
                : resolved.baseWritingDirection == NSWritingDirectionRightToLeft ? 2 : 0;
            output->line.carets_available = wantsCarets(wanted, sourceRange.location, sourceRange.length);
            if (output->line.carets_available
                && !carets.append(layout, output->line, fragment.origin.x, output->carets)) return false;
            slots.append(layout, range, fragment.origin, *output);
            if (!slots.valid) return false;
            output->glyphs.reserve(range.length);
            layout.sourceGlyph = NSNotFound;
            [layout drawGlyphsForGlyphRange:range atPoint:NSZeroPoint];
            if (options.maxLines && options.ellipsize != GPUITextKitEllipsize::Clip
                && hidden.location == NSNotFound
                && !omittedWithoutNativeRange(layout, range, fragment.origin, *output, hidden)) return false;
            if (options.maxLines && options.ellipsize == GPUITextKitEllipsize::Clip) {
                clippedSource(layout, container, range, sourceRange, fragment, used, *output);
            } else if (hidden.location != NSNotFound && hidden.length) {
                NSRange missing = [layout characterRangeForGlyphRange:hidden actualGlyphRange:nullptr];
                if (missing.location > sourceRange.location)
                    output->visible.push_back({sourceRange.location, missing.location - sourceRange.location});
                if (NSMaxRange(missing) < NSMaxRange(sourceRange))
                    output->visible.push_back({NSMaxRange(missing), NSMaxRange(sourceRange) - NSMaxRange(missing)});
                output->truncated = true;
            } else {
                output->visible.push_back({sourceRange.location, sourceRange.length});
            }
            if (options.maxLines) filterCarets(*output);
            if (hidden.location != NSNotFound && hidden.length) {
                if (!captureTruncated(layout, range, hidden, *output, options)) return false;
            } else if (!slots.assign(output)) {
                output->truncateGlyphs(0, 0);
                output->attachments.clear();
                for (NSUInteger index = range.location; index < NSMaxRange(range); ++index) {
                    if ([layout notShownAttributeForGlyphAtIndex:index]) continue;
                    layout.sourceGlyph = index;
                    [layout drawGlyphsForGlyphRange:NSMakeRange(index, 1) atPoint:NSZeroPoint];
                }
            }
            if (options.maxLines && options.ellipsize == GPUITextKitEllipsize::Clip)
                filterClippedDrawing(fragment, *output);
            if (!layout.valid || !output->emit(&options)) return false;
            cursor = NSMaxRange(range);
            if (options.maxLines && output->emitted >= options.maxLines) {
                if (NSMaxRange(sourceRange) < layout.textStorage.length) output->truncated = true;
                break;
            }
        }
    } @finally {
        NSGraphicsContext.currentContext = previous;
        CGContextRelease(context);
    }
    if (!layout.valid) return false;
    return true;
}

GPUITextKitLayout *gpui_textkit_layout(const uint16_t *text, size_t length,
    const GPUITextKitFontRun *runs, size_t count, double width, GPUITextKitDirection direction, const GPUITextKitOptions *options) {
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
                NSMutableParagraphStyle *style = paragraphStyle(direction, *options);
                if (!style) return nullptr;
                NSMutableAttributedString *attributed = configuredSource(text, length, runs, count, style, options->lineHeight);
                if (!attributed || !configureDocument(attributed, *options, runs, count)) return nullptr;
                NSString *source = attributed.string;
                NSTextStorage *storage = [[NSTextStorage alloc] initWithAttributedString:attributed];
                if (![storage.string isEqualToString:source]) return nullptr;
                GPUITextKitCapture *layout = [[GPUITextKitCapture alloc] init];
                layout.capture = output.get();
                layout.valid = YES;
                if (length == 0) layout.emptyFont = (__bridge NSFont *)runs[count - 1].font;
                layout.emptyRightToLeft = direction == GPUITextKitDirection::RightToLeft;
                layout.emptyStyle = style;
                layout.backgroundLayoutEnabled = NO;
                layout.allowsNonContiguousLayout = NO;
                layout.usesFontLeading = NO;
                NSTextContainer *container = [[NSTextContainer alloc] initWithContainerSize:NSMakeSize(width, CGFLOAT_MAX)];
                container.lineFragmentPadding = 0;
                container.lineBreakMode = breakMode(*options);
                container.maximumNumberOfLines = options->maxLines;
                [layout addTextContainer:container];
                [storage addLayoutManager:layout];
                [layout ensureLayoutForTextContainer:container];
                if (!captureRows(layout, container, *options, output.get())) return nullptr;
                if (layout.extraLineFragmentTextContainer == container
                    && (!options->maxLines || output->emitted < options->maxLines)) {
                    NSRect fragment = layout.extraLineFragmentRect;
                    NSRect used = layout.extraLineFragmentUsedRect;
                    bool available = wantsCarets(wanted, length, 0);
                    if (available) output->carets.push_back({length, used.origin.x});
                    output->line = {length, 0,
                        {used.origin.x, used.origin.y, used.size.width, used.size.height},
                        {fragment.origin.x, fragment.origin.y, fragment.size.width, fragment.size.height}, size_t(available), 0, available, 0};
                    output->line.base_direction = static_cast<uint8_t>(direction);
                    output->visible.push_back({length, 0});
                    if (!output->emit(options)) return nullptr;
                }
                if (selection && selection->length > 0) {
                    NSRange selected = NSMakeRange(selection->start, selection->length);
                    auto visible = options->maxLines ? output->allVisible
                        : std::vector<GPUITextKitSelection>{{selected.location, selected.length}};
                    for (const auto &span : visible) {
                        NSRange range = NSIntersectionRange(selected, NSMakeRange(span.start, span.length));
                        if (!range.length) continue;
                        NSUInteger count = 0;
                        NSRectArray rectangles = [layout rectArrayForCharacterRange:range
                            withinSelectedCharacterRange:range inTextContainer:container rectCount:&count];
                        if (count && !rectangles) return nullptr;
                        for (NSUInteger index = 0; index < count; ++index) {
                            NSRect value = rectangles[index];
                            output->selectionRects.push_back({{value.origin.x, value.origin.y, value.size.width, value.size.height}});
                        }
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

const GPUITextKitSelection *gpui_textkit_visible_source(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->visible.size();
    return layout->visible.data();
}
const GPUITextKitAttachmentRect *gpui_textkit_attachments(const GPUITextKitLayout *layout, size_t *count) {
    *count = layout->attachments.size();
    return layout->attachments.data();
}
bool gpui_textkit_truncated(const GPUITextKitLayout *layout) { return layout->truncated; }
