#pragma once
#include <CoreText/CoreText.h>
#include <stddef.h>
#include <stdint.h>

struct GPUITextKitSelection {
    size_t start;
    size_t length;
};
struct GPUITextKitRect {
    double bounds[4];
};
struct GPUITextKitFontRun {
    size_t start;
    size_t length;
    CTFontRef font;
    double baselineOffset;
};
struct GPUITextKitLine {
    size_t start;
    size_t length;
    double bounds[4];
    double fragment[4];
    size_t primary_count;
    size_t alternate_count;
    bool carets_available;
    uint8_t base_direction;
};
struct GPUITextKitBidiRun {
    size_t start;
    size_t length;
    uint8_t level;
};
struct GPUITextKitCaret {
    size_t source;
    double x;
};
struct GPUITextKitGlyph {
    size_t source;
    uint32_t glyph;
    size_t run;
    double position[2];
    bool synthetic;
};
struct GPUITextKitRun {
    CTFontRef font;
    double matrix[6];
};
struct GPUITextKitLayout;
extern "C" {
// Row arrays are borrowed only until this synchronous calling-thread callback returns. Retain fonts
// before keeping their identities; return false to stop. Never unwind through C++.
typedef bool (*GPUITextKitEmit)(void *context, const GPUITextKitLayout *row);
}
enum class GPUITextKitDirection : uint8_t { Natural, LeftToRight, RightToLeft };
enum class GPUITextKitCaretMode : uint8_t { All, None, Source };
struct GPUITextKitCaretRequest {
    GPUITextKitCaretMode mode;
    size_t start;
    size_t length;
};
enum class GPUITextKitLineHeightMode : uint8_t { Natural, Exact, AtLeast };
struct GPUITextKitLineHeight {
    GPUITextKitLineHeightMode mode = GPUITextKitLineHeightMode::Natural;
    double height = 0;
};
enum class GPUITextKitAlignment : uint8_t { Natural, Left, Center, Right, Justified };
enum class GPUITextKitEllipsize : uint8_t { Head, Middle, Tail, Clip };
struct GPUITextKitParagraphSpan {
    size_t start;
    size_t length;
    GPUITextKitDirection direction;
    GPUITextKitAlignment alignment;
    GPUITextKitLineHeight lineHeight;
    double firstLineIndent;
    double headIndent;
    double tailIndent;
    double spaceBefore;
    double spaceAfter;
};
struct GPUITextKitAttachment {
    size_t start;
    size_t length;
    double width;
    double height;
    double baselineOffset;
};
struct GPUITextKitAttachmentRect {
    size_t start;
    size_t length;
    double bounds[4];
};
struct GPUITextKitOptions {
    const GPUITextKitSelection *selection;
    void *context;
    GPUITextKitEmit emit;
    GPUITextKitCaretRequest carets;
    GPUITextKitLineHeight lineHeight{};
    GPUITextKitAlignment alignment = GPUITextKitAlignment::Natural;
    const GPUITextKitParagraphSpan *paragraphs = nullptr;
    size_t paragraphCount = 0;
    const GPUITextKitAttachment *attachments = nullptr;
    size_t attachmentCount = 0;
    size_t maxLines = 0;
    GPUITextKitEllipsize ellipsize = GPUITextKitEllipsize::Tail;
};
extern "C" {
GPUITextKitLayout *gpui_textkit_layout(const uint16_t *text, size_t length,
    const GPUITextKitFontRun *runs, size_t count, double width, GPUITextKitDirection direction, const GPUITextKitOptions *options);
const GPUITextKitSelection *gpui_textkit_visible_source(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitAttachmentRect *gpui_textkit_attachments(const GPUITextKitLayout *layout, size_t *count);
bool gpui_textkit_truncated(const GPUITextKitLayout *layout);
void gpui_textkit_layout_drop(GPUITextKitLayout *layout);
const GPUITextKitRect *gpui_textkit_selection_rects(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitLine *gpui_textkit_line(const GPUITextKitLayout *layout);
const GPUITextKitRun *gpui_textkit_runs(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitBidiRun *gpui_textkit_bidi_runs(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitCaret *gpui_textkit_carets(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitGlyph *gpui_textkit_glyphs(const GPUITextKitLayout *layout, size_t *count);
}
