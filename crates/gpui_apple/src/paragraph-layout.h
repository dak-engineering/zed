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
};
struct GPUITextKitLine {
    size_t start;
    size_t length;
    double bounds[4];
    double fragment[4];
    size_t primary_count;
    size_t alternate_count;
    bool carets_available;
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
struct GPUITextKitOptions {
    const GPUITextKitSelection *selection;
    void *context;
    GPUITextKitEmit emit;
    GPUITextKitCaretRequest carets;
    GPUITextKitLineHeight lineHeight{};
    GPUITextKitAlignment alignment = GPUITextKitAlignment::Natural;
};
extern "C" {
GPUITextKitLayout *gpui_textkit_layout(const uint16_t *text, size_t length,
    const GPUITextKitFontRun *runs, size_t count, double width, GPUITextKitDirection direction, const GPUITextKitOptions *options);
void gpui_textkit_layout_drop(GPUITextKitLayout *layout);
const GPUITextKitRect *gpui_textkit_selection_rects(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitLine *gpui_textkit_line(const GPUITextKitLayout *layout);
const GPUITextKitRun *gpui_textkit_runs(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitBidiRun *gpui_textkit_bidi_runs(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitCaret *gpui_textkit_carets(const GPUITextKitLayout *layout, size_t *count);
const GPUITextKitGlyph *gpui_textkit_glyphs(const GPUITextKitLayout *layout, size_t *count);
}
