#include "app.h"
#include <dwmapi.h>
#include <uxtheme.h>
#include <psapi.h>

#ifndef DWMWA_USE_IMMERSIVE_DARK_MODE
#define DWMWA_USE_IMMERSIVE_DARK_MODE 20
#endif

// No C runtime is linked (it would cost a DLL load and startup time); the compiler still emits
// calls to these for struct copies and zeroing.
void *memset(void *dst, int c, size_t n)
{
    unsigned char *d = dst;
    while (n--)
        *d++ = (unsigned char)c;
    return dst;
}

void *memcpy(void *dst, const void *src, size_t n)
{
    unsigned char *d = dst;
    const unsigned char *s = src;
    while (n--)
        *d++ = *s++;
    return dst;
}

typedef struct {
    COLORREF bg, panel, fg, dim, sel, border, edit_bg;
    COLORREF added, modified, deleted, untracked, renamed, conflict;
    COLORREF added_bg, removed_bg;
    COLORREF added_word_bg, removed_word_bg;  // changed words inside a changed line
    COLORREF syntax[8];  // indexed by TOK_*
} Theme;

static const Theme dark_theme = {
    RGB(30, 30, 30), RGB(37, 37, 38), RGB(212, 212, 212), RGB(140, 140, 140), RGB(9, 71, 113),
    RGB(69, 69, 69), RGB(49, 49, 49),
    RGB(115, 201, 145), RGB(226, 192, 141), RGB(241, 76, 76), RGB(78, 201, 176), RGB(197, 134, 192),
    RGB(244, 135, 113),
    RGB(35, 57, 38), RGB(70, 35, 37),
    RGB(46, 98, 55), RGB(125, 45, 48),
    {RGB(86, 156, 214), RGB(197, 134, 192), RGB(78, 201, 176), RGB(220, 220, 170), RGB(206, 145, 120),
     RGB(181, 206, 168), RGB(106, 153, 85), RGB(155, 155, 155)},
};

static const Theme light_theme = {
    RGB(255, 255, 255), RGB(243, 243, 243), RGB(30, 30, 30), RGB(110, 110, 110), RGB(204, 228, 247),
    RGB(200, 200, 200), RGB(255, 255, 255),
    RGB(46, 125, 50), RGB(154, 103, 0), RGB(198, 40, 40), RGB(0, 121, 107), RGB(142, 36, 170),
    RGB(216, 67, 21),
    RGB(218, 251, 225), RGB(255, 235, 233),
    RGB(172, 238, 187), RGB(255, 196, 192),
    {RGB(0, 0, 255), RGB(175, 0, 219), RGB(38, 127, 153), RGB(121, 94, 38), RGB(163, 21, 21),
     RGB(9, 134, 88), RGB(0, 128, 0), RGB(128, 128, 128)},
};

// Layout sizes at 96 DPI.
enum { PAD = 8, BAR_H = 40, LIST_W = 340, STATUS_H = 24, REF_W = 150, ARROW_W = 24, TOGGLE_W = 180, NAV_W = 150 };
enum { SPLIT_W = 5, MIN_LIST_W = 120, MIN_DIFF_W = 200 };  // draggable strip between list and diff
enum { CHANGE_CONTEXT = 3 };  // rows left above a change that Prev/Next moves to
enum { EDIT_PATH, EDIT_BASE, EDIT_TARGET, EDIT_COUNT };
enum { REFS_W = 440, REFS_ROWS = 14 };  // ref dropdown width and most rows shown at once
enum { LANGUAGE_W = 150, EOL_W = 170 };  // status bar fields at the right end: highlighter, checkbox

// Indexed by LANG_*. Fixed width, not pointers: see the keyword lists in syntax.c.
static const wchar_t language_names[LANG_COUNT][12] = {
    L"Auto",   L"Plain text", L"C / C++", L"C#",   L"CSS",   L"Go",    L"HTML",       L"Java", L"JavaScript", L"JSON",
    L"Kotlin", L"PowerShell", L"Python",  L"Rust", L"Shell", L"SQL",   L"TypeScript", L"XML",  L"YAML",       L"Zig",
};

#define SCROLL_END 0x3FFFFFFF

static struct {
    HWND main, list, diff_wnd;
    HWND edits[EDIT_COUNT];
    RECT frames[EDIT_COUNT];
    RECT toggle, nav;      // [Unified | Side by side], [Prev | Next]
    RECT language_field;   // status bar, right end: opens the highlighter menu
    RECT eol_field;        // status bar, left of language_field: the "Ignore line endings" checkbox
    UINT dpi;
    const Theme *theme;
    HFONT ui_font, bold_font, code_font;
    HBRUSH edit_brush, border_brush;
    int text_h, row_h;
    int code_h, code_row_h, char_w;
    wchar_t folder[1024];
    StatusResult *status;  // NULL until the first list arrives
    int loading;
    int sel, top, wheel_rest;
    int list_w96;          // file list width at 96 DPI, set by dragging the split
    int drag_dx;           // while dragging the split: mouse x minus list width; -1 otherwise
    DiffResult *diff;      // NULL until the selected file's diff arrives
    int diff_generation;
    int keep_scroll_generation;  // a diff reloaded by auto-refresh keeps its scroll position
    int language_pick;     // LANG_AUTO, or the highlighter picked in the menu for the open file
    int ignore_eol;        // diffs ignore CR at the end of lines (git diff --ignore-cr-at-eol)
    int side_by_side;
    int diff_top, diff_left, diff_wheel_rest;
    int change_row;         // first row of the change Prev/Next last moved to, -1 for none
    int change_generation;  // diff request made when Prev/Next crossed to another file
    int change_dir;         // that file then opens on its first (+1) or last (-1) change
    HWND refs_wnd;          // ref dropdown, created on first use
    RefList *refs;          // last ref list read; NULL until the first arrives
    int refs_edit;          // EDIT_BASE / EDIT_TARGET while the dropdown is open, -1 when closed
    int refs_generation;
    int refs_top, refs_sel; // first visible and highlighted row of refs->shown; refs_sel -1 for none
    int refs_typed;         // typed in the box since the dropdown opened: filter by the text
    int refs_setting_text;  // the box text is being set by a pick, not typed
    int perf;
    int exit_code;
    DWORD startup_ms;      // 0 until the first paint
} app;

static WNDPROC edit_proc_original;
// UI thread only; static to keep them off the stack.
static wchar_t line_text[8192];
static Token tokens[1024];

void fatal(const wchar_t *what)
{
    wchar_t msg[256];
    wsprintfW(msg, L"%s failed.", what);
    MessageBoxW(NULL, msg, L"DiffViewer", MB_ICONERROR);
    ExitProcess(1);
}

int utf8_to_wide(const char *src, int len, wchar_t *dst, int cap)
{
    if (len <= 0)
        return 0;
    return MultiByteToWideChar(CP_UTF8, 0, src, len, dst, cap);
}

void arena_init(Arena *a, size_t reserve)
{
    a->base = VirtualAlloc(NULL, reserve, MEM_RESERVE, PAGE_READWRITE);
    if (!a->base)
        fatal(L"Reserving memory");
    a->used = a->committed = 0;
    a->reserved = reserve;
}

static void arena_commit(Arena *a, size_t need)
{
    if (need <= a->committed)
        return;
    size_t target = (need + 0xFFFF) & ~(size_t)0xFFFF;
    if (target > a->reserved || !VirtualAlloc(a->base + a->committed, target - a->committed, MEM_COMMIT, PAGE_READWRITE))
        fatal(L"Allocating memory");
    a->committed = target;
}

void *arena_push(Arena *a, size_t size)
{
    a->used = (a->used + 7) & ~(size_t)7;
    arena_commit(a, a->used + size);
    void *p = a->base + a->used;
    a->used += size;
    return p;
}

// Free space at the end, for reading data of unknown size; the caller advances `used`.
char *arena_tail(Arena *a, size_t min_free)
{
    arena_commit(a, a->used + min_free);
    return a->base + a->used;
}

void arena_free(Arena *a)
{
    VirtualFree(a->base, 0, MEM_RELEASE);
    a->base = NULL;
}

static int scale(int v)
{
    return MulDiv(v, (int)app.dpi, 96);
}

static void fill(HDC dc, const RECT *r, COLORREF color)
{
    SetBkColor(dc, color);
    ExtTextOutW(dc, 0, 0, ETO_OPAQUE, r, NULL, 0, NULL);
}

// New position for a WM_VSCROLL/WM_HSCROLL request; the caller clamps it.
static int scroll_target(HWND hwnd, int bar, WPARAM wp, int pos, int page, int step)
{
    SCROLLINFO si = {sizeof si, SIF_TRACKPOS};
    switch (LOWORD(wp)) {
    case SB_LINEUP: return pos - step;
    case SB_LINEDOWN: return pos + step;
    case SB_PAGEUP: return pos - page;
    case SB_PAGEDOWN: return pos + page;
    case SB_TOP: return 0;
    case SB_BOTTOM: return SCROLL_END;
    case SB_THUMBTRACK:
    case SB_THUMBPOSITION:
        GetScrollInfo(hwnd, bar, &si);
        return si.nTrackPos;
    }
    return pos;
}

static DWORD ms_since_process_start(void)
{
    FILETIME created, exited, kernel, user, now;
    GetProcessTimes(GetCurrentProcess(), &created, &exited, &kernel, &user);
    GetSystemTimePreciseAsFileTime(&now);
    ULONGLONG start = ((ULONGLONG)created.dwHighDateTime << 32) | created.dwLowDateTime;
    ULONGLONG end = ((ULONGLONG)now.dwHighDateTime << 32) | now.dwLowDateTime;
    return (DWORD)((end - start) / 10000);
}

static void apply_theme(void)
{
    DWORD light = 1, size = sizeof light;
    RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                 L"AppsUseLightTheme", RRF_RT_REG_DWORD, NULL, &light, &size);
    BOOL dark = light == 0;
    app.theme = dark ? &dark_theme : &light_theme;
    DwmSetWindowAttribute(app.main, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark, sizeof dark);
    SetWindowTheme(app.list, dark ? L"DarkMode_Explorer" : L"Explorer", NULL);
    SetWindowTheme(app.diff_wnd, dark ? L"DarkMode_Explorer" : L"Explorer", NULL);
    if (app.refs_wnd)
        SetWindowTheme(app.refs_wnd, dark ? L"DarkMode_Explorer" : L"Explorer", NULL);
    if (app.edit_brush)
        DeleteObject(app.edit_brush);
    if (app.border_brush)
        DeleteObject(app.border_brush);
    app.edit_brush = CreateSolidBrush(app.theme->edit_bg);
    app.border_brush = CreateSolidBrush(app.theme->border);
}

static void make_fonts(void)
{
    if (app.ui_font)
        DeleteObject(app.ui_font);
    if (app.bold_font)
        DeleteObject(app.bold_font);
    if (app.code_font)
        DeleteObject(app.code_font);
    int height = -MulDiv(9, (int)app.dpi, 72);
    app.ui_font = CreateFontW(height, 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET, 0, 0, CLEARTYPE_QUALITY, 0, L"Segoe UI");
    app.bold_font = CreateFontW(height, 0, 0, 0, FW_BOLD, 0, 0, 0, DEFAULT_CHARSET, 0, 0, CLEARTYPE_QUALITY, 0, L"Segoe UI");
    app.code_font = CreateFontW(-MulDiv(10, (int)app.dpi, 72), 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET, 0, 0,
                                CLEARTYPE_QUALITY, FIXED_PITCH, L"Consolas");
    HDC dc = GetDC(app.main);
    TEXTMETRICW tm;
    SelectObject(dc, app.ui_font);
    GetTextMetricsW(dc, &tm);
    app.text_h = tm.tmHeight;
    app.row_h = app.text_h + scale(8);
    SelectObject(dc, app.code_font);
    GetTextMetricsW(dc, &tm);
    app.code_h = tm.tmHeight;
    app.code_row_h = app.code_h + scale(2);
    app.char_w = tm.tmAveCharWidth;
    ReleaseDC(app.main, dc);
    for (int i = 0; i < EDIT_COUNT; i++) {
        if (app.edits[i])
            SendMessageW(app.edits[i], WM_SETFONT, (WPARAM)app.ui_font, TRUE);
    }
}

static int list_width(void)
{
    RECT c;
    GetClientRect(app.main, &c);
    int w = scale(app.list_w96), max = c.right - scale(SPLIT_W) - scale(MIN_DIFF_W);
    if (w > max)
        w = max;
    if (w < scale(MIN_LIST_W))
        w = scale(MIN_LIST_W);
    return w < c.right ? w : c.right;
}

static RECT split_rect(void)
{
    RECT c;
    GetClientRect(app.main, &c);
    int x = list_width();
    return (RECT){x, scale(BAR_H), x + scale(SPLIT_W), c.bottom - scale(STATUS_H)};
}

static void layout(void)
{
    RECT c;
    GetClientRect(app.main, &c);
    int bar = scale(BAR_H), status = scale(STATUS_H), pad = scale(PAD);

    // Top bar: [path ........] [base] -> [target] [Prev | Next] [Unified | Side by side]
    int right = c.right - pad;
    app.toggle = (RECT){right - scale(TOGGLE_W), pad, right, bar - pad};
    right = app.toggle.left - pad;
    app.nav = (RECT){right - scale(NAV_W), pad, right, bar - pad};
    right = app.nav.left - pad;
    app.frames[EDIT_TARGET] = (RECT){right - scale(REF_W), pad, right, bar - pad};
    right = app.frames[EDIT_TARGET].left - scale(ARROW_W);
    app.frames[EDIT_BASE] = (RECT){right - scale(REF_W), pad, right, bar - pad};
    right = app.frames[EDIT_BASE].left - pad;
    app.frames[EDIT_PATH] = (RECT){pad, pad, right > pad ? right : pad, bar - pad};
    for (int i = 0; i < EDIT_COUNT; i++) {
        const RECT *f = &app.frames[i];
        int w = f->right - f->left - scale(12);
        MoveWindow(app.edits[i], f->left + scale(6), f->top + (f->bottom - f->top - app.text_h) / 2, w > 0 ? w : 0,
                   app.text_h, TRUE);
    }

    app.language_field = (RECT){c.right - scale(LANGUAGE_W), c.bottom - status, c.right - pad, c.bottom};
    app.eol_field = (RECT){app.language_field.left - scale(EOL_W), c.bottom - status, app.language_field.left - pad,
                           c.bottom};

    int list_w = list_width();
    int pane_h = c.bottom - bar - status;
    pane_h = pane_h > 0 ? pane_h : 0;
    MoveWindow(app.list, 0, bar, list_w, pane_h, TRUE);
    int split = scale(SPLIT_W);
    int diff_w = c.right - list_w - split;
    MoveWindow(app.diff_wnd, list_w + split, bar, diff_w > 0 ? diff_w : 0, pane_h, TRUE);
}

static int row_count(void)
{
    return app.status ? app.status->row_count : 0;
}

static void perf_print(const char *text)
{
    HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
    if (!out || out == INVALID_HANDLE_VALUE) {
        if (!AttachConsole(ATTACH_PARENT_PROCESS))
            return;
        out = CreateFileW(L"CONOUT$", GENERIC_WRITE, FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
    }
    DWORD written;
    WriteFile(out, text, (DWORD)lstrlenA(text), &written, NULL);
}

// PROCESS_MEMORY_COUNTERS_EX2, missing from the mingw headers. PrivateWorkingSetSize is the number
// Task Manager shows; the peak working set is mostly shared system DLL pages.
typedef struct {
    DWORD cb, PageFaultCount;
    SIZE_T PeakWorkingSetSize, WorkingSetSize, QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage,
        QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage, PagefileUsage, PeakPagefileUsage, PrivateUsage,
        PrivateWorkingSetSize;
    ULONG64 SharedCommitUsage;
} MemoryCounters;

static MemoryCounters memory_counters(void)
{
    MemoryCounters mem = {sizeof mem};
    GetProcessMemoryInfo(GetCurrentProcess(), (PROCESS_MEMORY_COUNTERS *)&mem, sizeof mem);
    return mem;
}

// Called once the list and the first diff are loaded (or failed): prints the numbers and exits.
static void perf_finish(const wchar_t *error)
{
    UpdateWindow(app.list);  // painted with the data before memory is measured
    UpdateWindow(app.diff_wnd);
    char line[1200];
    if (error[0]) {
        int n = WideCharToMultiByte(CP_UTF8, 0, error, -1, line + 7, 1100, NULL, NULL);
        CopyMemory(line, "error: ", 7);
        line[7 + (n > 0 ? n - 1 : 0)] = 0;
        perf_print(line);
        perf_print("\r\n");
        app.exit_code = 1;
    } else {
        MemoryCounters mem = memory_counters();
        wsprintfA(line, "private_kb=%lu peak_ws_kb=%lu startup_ms=%lu status_ms=%lu diff_ms=%lu files=%d diff_lines=%d\r\n",
                  (DWORD)(mem.PrivateWorkingSetSize / 1024), (DWORD)(mem.PeakWorkingSetSize / 1024),
                  app.startup_ms, app.status->elapsed_ms, app.diff ? app.diff->elapsed_ms : 0,
                  app.status->file_count, app.diff ? app.diff->line_count : 0);
        perf_print(line);
    }
    DestroyWindow(app.main);
}

// ---- diff pane -------------------------------------------------------------------------------

static int diff_rows(void)
{
    if (!app.diff || app.diff->error[0])
        return 0;
    return app.side_by_side ? app.diff->side_count : app.diff->line_count;
}

static int line_number_width(void)
{
    unsigned n = app.diff ? app.diff->max_line_no : 0;
    int digits = 1;
    while (n >= 10) {
        n /= 10;
        digits++;
    }
    return (digits < 3 ? 3 : digits) * app.char_w;
}

// Unified gutter: old line number, new line number, +/- marker, then the text. Returns the text's x.
static int unified_gutter(int *old_right, int *new_right, int *marker_x)
{
    int num_w = line_number_width();
    *old_right = scale(8) + num_w;
    *new_right = *old_right + scale(10) + num_w;
    *marker_x = *new_right + scale(10);
    return *marker_x + 2 * app.char_w;
}

// Side-by-side: each half is [line number][text]; returns the text's x within its half.
static int side_text_offset(void)
{
    return scale(8) + line_number_width() + scale(10);
}

static int diff_visible_rows(void)
{
    RECT c;
    GetClientRect(app.diff_wnd, &c);
    int rows = c.bottom / app.code_row_h;
    return rows > 0 ? rows : 1;
}

static void diff_scroll_to(int top, int left)
{
    RECT c;
    GetClientRect(app.diff_wnd, &c);
    int rows = diff_rows(), visible = diff_visible_rows();
    if (top > rows - visible)
        top = rows - visible;
    if (top < 0)
        top = 0;
    int page = app.side_by_side ? (c.right - 1) / 2 : c.right;
    int width = 0;
    if (rows) {
        int old_right, new_right, marker_x;
        int text_x = app.side_by_side ? side_text_offset() : unified_gutter(&old_right, &new_right, &marker_x);
        width = text_x + ((int)app.diff->max_line_len + 1) * app.char_w;
    }
    if (left > width - page)
        left = width - page;
    if (left < 0)
        left = 0;
    app.diff_top = top;
    app.diff_left = left;
    SCROLLINFO si = {sizeof si, SIF_RANGE | SIF_PAGE | SIF_POS, 0, rows - 1, (UINT)visible, top};
    SetScrollInfo(app.diff_wnd, SB_VERT, &si, TRUE);
    si = (SCROLLINFO){sizeof si, SIF_RANGE | SIF_PAGE | SIF_POS, 0, width - 1, (UINT)page, left};
    SetScrollInfo(app.diff_wnd, SB_HORZ, &si, TRUE);
    InvalidateRect(app.diff_wnd, NULL, FALSE);
}

// Advances a UTF-16 index over UTF-8 bytes [from, to), as MultiByteToWideChar converts them.
static int wide_index_after(const char *s, int from, int to, int wide)
{
    for (int i = from; i < to; i++) {
        unsigned char b = (unsigned char)s[i];
        if ((b & 0xC0) != 0x80)
            wide += b >= 0xF0 ? 2 : 1;  // 4-byte sequences become surrogate pairs
    }
    return wide;
}

// Draws one diff line's text at x (scrolled), clipped to `clip`, highlighted unless it is plain text.
static void draw_code(HDC dc, const DiffResult *d, int index, int new_side, int x, int y, const RECT *clip)
{
    const DiffLine *line = &d->lines[index];
    const char *s = d->text + line->offset;
    int len = (int)line->length < 8192 ? (int)line->length : 8192;
    int n = utf8_to_wide(s, len, line_text, 8192);
    int count = 0;
    if (d->states)
        syntax_scan_line(d->language, s, len, d->states[index * 2 + new_side], tokens, 1024, &count);

    int saved = SaveDC(dc);
    IntersectClipRect(dc, clip->left, clip->top, clip->right, clip->bottom);
    int tab_w = 4 * app.char_w;

    // Changed-word backgrounds: this line's spans, found by binary search (they are in line order).
    int lo = 0, hi = d->word_span_count;
    while (lo < hi) {
        int mid = (lo + hi) / 2;
        if (d->word_spans[mid].line < index)
            lo = mid + 1;
        else
            hi = mid;
    }
    COLORREF word_bg = line->kind == LINE_ADDED ? app.theme->added_word_bg : app.theme->removed_word_bg;
    for (; lo < d->word_span_count && d->word_spans[lo].line == index; lo++) {
        const WordSpan *w = &d->word_spans[lo];
        if (w->start >= len)
            break;
        int end = w->end < len ? w->end : len;
        int from = wide_index_after(s, 0, w->start, 0);
        int to = wide_index_after(s, w->start, end, from);
        from = from < n ? from : n;
        to = to < n ? to : n;
        RECT mark = {x + (int)LOWORD(GetTabbedTextExtentW(dc, line_text, from, 1, &tab_w)), clip->top,
                     x + (int)LOWORD(GetTabbedTextExtentW(dc, line_text, to, 1, &tab_w)), clip->bottom};
        fill(dc, &mark, word_bg);
    }
    int byte = 0, wide = 0, cx = x;
    // Plain text up to each token, then the token in its color; token offsets are bytes.
    for (int t = 0; t <= count && cx < clip->right; t++) {
        int starts[2] = {t < count ? tokens[t].start : len, t < count ? tokens[t].end : len};
        for (int part = 0; part < 2; part++) {
            int to = wide_index_after(s, byte, starts[part], wide);
            to = to < n ? to : n;
            if (to > wide) {
                SetTextColor(dc, part == 0 ? app.theme->fg : app.theme->syntax[tokens[t].kind]);
                cx += LOWORD(TabbedTextOutW(dc, cx, y, line_text + wide, to - wide, 1, &tab_w, x));
            }
            byte = starts[part];
            wide = to;
        }
    }
    RestoreDC(dc, saved);
}

static void draw_number(HDC dc, unsigned no, int right, int y)
{
    wchar_t num[16];
    int n = wsprintfW(num, L"%u", no);
    SetTextColor(dc, app.theme->dim);
    ExtTextOutW(dc, right - n * app.char_w, y, 0, NULL, num, (UINT)n, NULL);
}

static void paint_unified_row(HDC dc, const DiffResult *d, int index, const RECT *r, int y, unsigned old_no,
                              unsigned new_no)
{
    const Theme *t = app.theme;
    const DiffLine *line = &d->lines[index];
    int old_right, new_right, marker_x;
    int text_x = unified_gutter(&old_right, &new_right, &marker_x);
    fill(dc, r, line->kind == LINE_ADDED ? t->added_bg : line->kind == LINE_REMOVED ? t->removed_bg
              : line->kind == LINE_GAP   ? t->panel : t->bg);
    if (line->kind == LINE_GAP) {
        SetTextColor(dc, t->dim);
        ExtTextOutW(dc, marker_x, y, 0, NULL, L"\x22ef", 1, NULL);
        return;
    }
    if (line->kind != LINE_ADDED)
        draw_number(dc, old_no, old_right, y);
    if (line->kind != LINE_REMOVED)
        draw_number(dc, new_no, new_right, y);
    if (line->kind != LINE_CONTEXT) {
        SetTextColor(dc, line->kind == LINE_ADDED ? t->added : t->deleted);
        ExtTextOutW(dc, marker_x, y, 0, NULL, line->kind == LINE_ADDED ? L"+" : L"-", 1, NULL);
    }
    RECT clip = {text_x, r->top, r->right, r->bottom};
    draw_code(dc, d, index, line->kind != LINE_REMOVED, text_x - app.diff_left, y, &clip);
}

// Line numbers for side-by-side cells, which are visited in increasing line order on each side.
typedef struct {
    int index;  // -1 until first used
    unsigned old_no, new_no;
} NumberCursor;

static unsigned number_at(const DiffResult *d, NumberCursor *c, int index, int new_side)
{
    if (c->index < 0 || index < c->index) {
        diff_line_numbers(d, index, &c->old_no, &c->new_no);
        c->index = index;
    }
    for (; c->index < index; c->index++)
        diff_advance_numbers(&d->lines[c->index], &c->old_no, &c->new_no);
    return new_side ? c->new_no : c->old_no;
}

static void paint_side_cell(HDC dc, const DiffResult *d, int index, int new_side, int x0, int x1, int top, int y,
                            NumberCursor *cursor)
{
    const Theme *t = app.theme;
    RECT cell = {x0, top, x1, top + app.code_row_h};
    if (index < 0) {
        fill(dc, &cell, t->panel);  // filler opposite a block of added or removed lines
        return;
    }
    const DiffLine *line = &d->lines[index];
    fill(dc, &cell, line->kind == LINE_ADDED ? t->added_bg : line->kind == LINE_REMOVED ? t->removed_bg
                  : line->kind == LINE_GAP   ? t->panel : t->bg);
    int num_right = x0 + scale(8) + line_number_width();
    if (line->kind == LINE_GAP) {
        SetTextColor(dc, t->dim);
        ExtTextOutW(dc, num_right + scale(10), y, 0, NULL, L"\x22ef", 1, NULL);
        return;
    }
    draw_number(dc, number_at(d, cursor, index, new_side), num_right, y);
    int text_x = x0 + side_text_offset();
    RECT clip = {text_x, cell.top, x1, cell.bottom};
    draw_code(dc, d, index, new_side, text_x - app.diff_left, y, &clip);
}

static void diff_paint(HWND hwnd)
{
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(hwnd, &ps);
    RECT c;
    GetClientRect(hwnd, &c);
    const Theme *t = app.theme;
    const DiffResult *d = app.diff;
    int rows = diff_rows();
    int first = app.diff_top + ps.rcPaint.top / app.code_row_h;
    int last = app.diff_top + (ps.rcPaint.bottom + app.code_row_h - 1) / app.code_row_h;
    int text_dy = (app.code_row_h - app.code_h) / 2;
    int half = (c.right - 1) / 2;
    unsigned old_no = 0, new_no = 0;
    if (!app.side_by_side && first < rows)
        diff_line_numbers(d, first, &old_no, &new_no);
    NumberCursor left_numbers = {-1, 0, 0}, right_numbers = {-1, 0, 0};
    SelectObject(dc, app.code_font);
    SetBkMode(dc, TRANSPARENT);

    for (int row = first; row < last; row++) {
        RECT r = {0, (row - app.diff_top) * app.code_row_h, c.right, (row - app.diff_top + 1) * app.code_row_h};
        int y = r.top + text_dy;
        if (row >= rows) {
            fill(dc, &r, t->bg);
        } else if (app.side_by_side) {
            const SideRow *side = &d->side[row];
            paint_side_cell(dc, d, side->left, 0, 0, half, r.top, y, &left_numbers);
            RECT separator = {half, r.top, half + 1, r.bottom};
            fill(dc, &separator, t->border);
            paint_side_cell(dc, d, side->right, 1, half + 1, c.right, r.top, y, &right_numbers);
        } else {
            paint_unified_row(dc, d, row, &r, y, old_no, new_no);
            diff_advance_numbers(&d->lines[row], &old_no, &new_no);
        }
    }

    const wchar_t *message = NULL;
    if (d && d->error[0])
        message = d->error;
    else if (d && d->binary)
        message = L"Binary file: no text diff.";
    else if (d && d->line_count == 0)
        message = d->ignore_eol ? L"Only line endings changed." : L"No content changes.";
    if (message) {
        RECT r = {scale(10), scale(8), c.right - scale(10), c.bottom};
        SelectObject(dc, app.ui_font);
        SetTextColor(dc, t->dim);
        DrawTextW(dc, message, -1, &r, DT_WORDBREAK | DT_NOPREFIX);
    }
    EndPaint(hwnd, &ps);
}

// The row of the other view (unified <-> side by side) that shows the same line as `row` here.
static int other_view_row(int row)
{
    const DiffResult *d = app.diff;
    if (app.side_by_side) {
        const SideRow *s = &d->side[row];
        return s->left >= 0 ? s->left : s->right;
    }
    int i = 0;
    for (; i < d->side_count - 1; i++) {
        const SideRow *s = &d->side[i];
        if ((s->left > s->right ? s->left : s->right) >= row)
            break;
    }
    return i;
}

// Switches unified <-> side by side, keeping the same line at the top.
static void toggle_view(void)
{
    int top = 0;
    if (diff_rows() > 0) {
        top = other_view_row(app.diff_top);
        if (app.change_row >= 0)
            app.change_row = other_view_row(app.change_row);
    }
    app.side_by_side = !app.side_by_side;
    diff_scroll_to(top, app.diff_left);
    InvalidateRect(app.main, &app.toggle, FALSE);
}

// Whether row `i` of the current view is an added or removed line (side by side: a context row
// shows the same line on both sides).
static int row_changed(int i)
{
    const DiffResult *d = app.diff;
    if (app.side_by_side)
        return d->side[i].left != d->side[i].right;
    return d->lines[i].kind == LINE_REMOVED || d->lines[i].kind == LINE_ADDED;
}

// First row of the next (dir +1) or previous (-1) change after `from`, or -1 when there is none.
static int find_change(int from, int dir)
{
    int rows = diff_rows();
    for (int i = from + dir; i >= 0 && i < rows; i += dir) {
        if (row_changed(i) && (i == 0 || !row_changed(i - 1)))
            return i;
    }
    return -1;
}

static void show_change(int row)
{
    app.change_row = row;
    diff_scroll_to(row - CHANGE_CONTEXT, app.diff_left);
}

static void list_select(int row, int dir);

// Prev/Next: the previous/next change in this file, then the previous/next file's last/first change.
static void goto_change(int dir)
{
    // While another file's diff is loading, app.diff is still the old file: go straight on.
    if (app.diff && app.diff->generation == app.diff_generation && diff_rows() > 0) {
        int from = app.change_row;
        if (from < app.diff_top || from >= app.diff_top + diff_visible_rows())
            from = dir > 0 ? app.diff_top - 1 : app.diff_top;  // scrolled away: start from the screen
        int row = find_change(from, dir);
        if (row >= 0) {
            show_change(row);
            return;
        }
    }
    if (!app.status || app.sel < 0)
        return;
    int old_sel = app.sel;
    app.change_generation = app.diff_generation + 1;  // the request list_select makes
    app.change_dir = dir;
    list_select(app.sel + dir, dir);
    if (app.sel == old_sel)
        app.change_generation = 0;  // already at the first/last file
}

static void move_focus(HWND from, int backwards);

static LRESULT CALLBACK diff_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_PAINT:
        diff_paint(hwnd);
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_SIZE:
        if (app.diff_wnd)  // WM_SIZE also arrives inside CreateWindow, before the handle is stored
            diff_scroll_to(app.diff_top, app.diff_left);
        return 0;
    case WM_VSCROLL:
        diff_scroll_to(scroll_target(hwnd, SB_VERT, wp, app.diff_top, diff_visible_rows(), 1), app.diff_left);
        return 0;
    case WM_HSCROLL: {
        RECT c;
        GetClientRect(hwnd, &c);
        diff_scroll_to(app.diff_top, scroll_target(hwnd, SB_HORZ, wp, app.diff_left, c.right / 2, 4 * app.char_w));
        return 0;
    }
    case WM_MOUSEWHEEL:
    case WM_MOUSEHWHEEL: {
        // 3 rows per 120-unit notch; the remainder is kept for smooth touchpads.
        app.diff_wheel_rest += GET_WHEEL_DELTA_WPARAM(wp);
        int steps = app.diff_wheel_rest / 40;
        app.diff_wheel_rest -= steps * 40;
        if (msg == WM_MOUSEHWHEEL)
            diff_scroll_to(app.diff_top, app.diff_left + steps * 2 * app.char_w);
        else if (GET_KEYSTATE_WPARAM(wp) & MK_SHIFT)
            diff_scroll_to(app.diff_top, app.diff_left - steps * 2 * app.char_w);
        else
            diff_scroll_to(app.diff_top - steps, app.diff_left);
        return 0;
    }
    case WM_LBUTTONDOWN:
        SetFocus(hwnd);
        return 0;
    case WM_KEYDOWN: {
        int visible = diff_visible_rows();
        switch (wp) {
        case VK_UP: diff_scroll_to(app.diff_top - 1, app.diff_left); break;
        case VK_DOWN: diff_scroll_to(app.diff_top + 1, app.diff_left); break;
        case VK_PRIOR: diff_scroll_to(app.diff_top - visible, app.diff_left); break;
        case VK_NEXT: diff_scroll_to(app.diff_top + visible, app.diff_left); break;
        case VK_HOME: diff_scroll_to(0, 0); break;
        case VK_END: diff_scroll_to(SCROLL_END, app.diff_left); break;
        case VK_LEFT: diff_scroll_to(app.diff_top, app.diff_left - 4 * app.char_w); break;
        case VK_RIGHT: diff_scroll_to(app.diff_top, app.diff_left + 4 * app.char_w); break;
        case VK_TAB: move_focus(hwnd, GetKeyState(VK_SHIFT) < 0); break;
        case 'U': toggle_view(); break;
        case 'N': goto_change(1); break;
        case 'P': goto_change(-1); break;
        case VK_F7: goto_change(GetKeyState(VK_SHIFT) < 0 ? -1 : 1); break;
        }
        return 0;
    }
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

// ---- file list -------------------------------------------------------------------------------

static int is_header(int row)
{
    int type = app.status->rows[row].type;
    return type == ROW_STAGED_HEADER || type == ROW_UNSTAGED_HEADER;
}

static char row_badge(const ListRow *row)
{
    const FileEntry *f = &app.status->files[row->file];
    char c = f->x;
    if (row->type == ROW_UNSTAGED) {
        if (f->kind == '?')
            return 'U';
        if (f->kind == 'u')
            return '!';
        c = f->y;
    }
    if (c == 'C')
        return 'A';  // a copy is a new file
    if (c == 'T')
        return 'M';  // type change (file <-> symlink)
    return c;
}

static COLORREF badge_color(char b)
{
    switch (b) {
    case 'A': return app.theme->added;
    case 'D': return app.theme->deleted;
    case 'U': return app.theme->untracked;
    case 'R': return app.theme->renamed;
    case '!': return app.theme->conflict;
    default: return app.theme->modified;
    }
}

static int list_visible_rows(void)
{
    RECT c;
    GetClientRect(app.list, &c);
    int rows = c.bottom / app.row_h;
    return rows > 0 ? rows : 1;
}

static void list_scroll_to(int top)
{
    int max_top = row_count() - list_visible_rows();
    if (top > max_top)
        top = max_top;
    if (top < 0)
        top = 0;
    app.top = top;
    SCROLLINFO si = {sizeof si, SIF_RANGE | SIF_PAGE | SIF_POS, 0, row_count() - 1, (UINT)list_visible_rows(), top};
    SetScrollInfo(app.list, SB_VERT, &si, TRUE);
    InvalidateRect(app.list, NULL, FALSE);
}

// Selects `row`, or the nearest file row in direction `dir` (+1/-1) when it is a group header.
static void list_select(int row, int dir)
{
    int count = row_count();
    if (count == 0)
        return;
    row = row < 0 ? 0 : row >= count ? count - 1 : row;
    int r = row;
    while (r >= 0 && r < count && is_header(r))
        r += dir;
    if (r < 0 || r >= count) {
        for (r = row; r >= 0 && r < count && is_header(r); r -= dir) {
        }
    }
    if (r < 0 || r >= count)
        return;

    int visible = list_visible_rows();
    int show = r > 0 && is_header(r - 1) ? r - 1 : r;  // keep a group's header in view with its first file
    if (show < app.top)
        list_scroll_to(show);
    else if (r >= app.top + visible)
        list_scroll_to(r - visible + 1);
    InvalidateRect(app.list, NULL, FALSE);
    if (r == app.sel)
        return;
    if (app.sel >= 0)
        app.language_pick = LANG_AUTO;  // a pick is for one file; -1 is a refresh of the same file
    app.sel = r;
    // Holding a key down queues many requests; the worker only runs the latest one.
    git_request_diff(app.status, &app.status->rows[r], ++app.diff_generation, app.language_pick, app.ignore_eol);
}

static void list_paint(HWND hwnd)
{
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(hwnd, &ps);
    RECT c;
    GetClientRect(hwnd, &c);
    const Theme *t = app.theme;
    int count = row_count();
    int first = app.top + ps.rcPaint.top / app.row_h;
    int last = app.top + (ps.rcPaint.bottom + app.row_h - 1) / app.row_h;
    int text_dy = (app.row_h - app.text_h) / 2;
    SetBkMode(dc, TRANSPARENT);

    for (int row = first; row < last; row++) {
        RECT r = {0, (row - app.top) * app.row_h, c.right, (row - app.top + 1) * app.row_h};
        int y = r.top + text_dy;
        if (row >= count) {
            fill(dc, &r, t->panel);
            continue;
        }
        const ListRow *list_row = &app.status->rows[row];
        fill(dc, &r, row == app.sel ? t->sel : t->panel);
        if (is_header(row)) {
            const wchar_t *label = list_row->type == ROW_STAGED_HEADER ? L"STAGED" : L"UNSTAGED";
            SelectObject(dc, app.bold_font);
            SetTextColor(dc, t->dim);
            ExtTextOutW(dc, scale(10), y, ETO_CLIPPED, &r, label, (UINT)lstrlenW(label), NULL);
            continue;
        }
        const FileEntry *f = &app.status->files[list_row->file];

        char b = row_badge(list_row);
        wchar_t badge_text = (wchar_t)b;
        SelectObject(dc, app.bold_font);
        SetTextColor(dc, badge_color(b));
        ExtTextOutW(dc, scale(10), y, ETO_CLIPPED, &r, &badge_text, 1, NULL);

        // File name first, then its folder dimmed, so the name is visible however deep it is.
        const char *slash = NULL;
        for (const char *p = f->path; *p; p++) {
            if (*p == '/')
                slash = p;
        }
        const char *name = slash ? slash + 1 : f->path;
        wchar_t text[1024];
        int n = utf8_to_wide(name, lstrlenA(name), text, 1024);
        int x = scale(30);
        SelectObject(dc, app.ui_font);
        SetTextColor(dc, t->fg);
        ExtTextOutW(dc, x, y, ETO_CLIPPED, &r, text, (UINT)n, NULL);
        if (slash) {
            SIZE size;
            GetTextExtentPoint32W(dc, text, n, &size);
            x += size.cx + scale(8);
            n = utf8_to_wide(f->path, (int)(slash - f->path), text, 1024);
            SetTextColor(dc, t->dim);
            ExtTextOutW(dc, x, y, ETO_CLIPPED, &r, text, (UINT)n, NULL);
        }
    }

    const wchar_t *message = NULL;
    if (count == 0 && !app.loading)
        message = app.status && app.status->error[0] ? app.status->error : L"No changes.";
    if (message) {
        RECT r = {scale(10), scale(8), c.right - scale(10), c.bottom};
        SelectObject(dc, app.ui_font);
        SetTextColor(dc, t->dim);
        DrawTextW(dc, message, -1, &r, DT_WORDBREAK | DT_NOPREFIX);
    }
    EndPaint(hwnd, &ps);

    if (!app.startup_ms)
        app.startup_ms = ms_since_process_start();
}

static LRESULT CALLBACK list_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_PAINT:
        list_paint(hwnd);
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_SIZE:
        if (app.list)  // WM_SIZE also arrives inside CreateWindow, before the handle is stored
            list_scroll_to(app.top);
        return 0;
    case WM_VSCROLL:
        list_scroll_to(scroll_target(hwnd, SB_VERT, wp, app.top, list_visible_rows(), 1));
        return 0;
    case WM_MOUSEWHEEL: {
        // 3 rows per 120-unit notch; the remainder is kept for smooth touchpads.
        app.wheel_rest += GET_WHEEL_DELTA_WPARAM(wp);
        int rows = app.wheel_rest / 40;
        app.wheel_rest -= rows * 40;
        list_scroll_to(app.top - rows);
        return 0;
    }
    case WM_LBUTTONDOWN: {
        SetFocus(hwnd);
        int row = app.top + (short)HIWORD(lp) / app.row_h;
        if (row < row_count())
            list_select(row, 1);
        return 0;
    }
    case WM_KEYDOWN: {
        int visible = list_visible_rows();
        switch (wp) {
        case VK_UP: list_select(app.sel - 1, -1); break;
        case VK_DOWN: list_select(app.sel + 1, 1); break;
        case VK_PRIOR: list_select(app.sel - visible, -1); break;
        case VK_NEXT: list_select(app.sel + visible, 1); break;
        case VK_HOME: list_select(0, 1); break;
        case VK_END: list_select(row_count() - 1, -1); break;
        case VK_TAB: move_focus(hwnd, GetKeyState(VK_SHIFT) < 0); break;
        case 'U': toggle_view(); break;
        case 'N': goto_change(1); break;
        case 'P': goto_change(-1); break;
        case VK_F7: goto_change(GetKeyState(VK_SHIFT) < 0 ? -1 : 1); break;
        }
        return 0;
    }
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

// ---- main window -----------------------------------------------------------------------------

// Tab order: path, base ref, target ref, file list, diff.
static void move_focus(HWND from, int backwards)
{
    HWND order[] = {app.edits[EDIT_PATH], app.edits[EDIT_BASE], app.edits[EDIT_TARGET], app.list, app.diff_wnd};
    int n = sizeof order / sizeof *order, i = 0;
    while (i < n && order[i] != from)
        i++;
    SetFocus(order[(i + (backwards ? n - 1 : 1)) % n]);
}

static void read_edit(int which, wchar_t *out, int cap)
{
    GetWindowTextW(app.edits[which], out, cap);
    int start = 0, len = lstrlenW(out);
    while (out[start] == ' ')
        start++;
    while (len > start && out[len - 1] == ' ')
        len--;
    for (int i = 0; i < len - start; i++)
        out[i] = out[start + i];
    out[len - start] = 0;
}

static void reload(void)
{
    wchar_t typed[1024], base[256], target[256];
    read_edit(EDIT_PATH, typed, 1024);
    read_edit(EDIT_BASE, base, 256);
    read_edit(EDIT_TARGET, target, 256);
    if (!GetFullPathNameW(typed, 1024, app.folder, NULL))
        return;
    app.loading = 1;
    git_request_status(app.folder, base, target);
    InvalidateRect(app.main, NULL, FALSE);
}

// ---- ref dropdown ----------------------------------------------------------------------------

static const char ref_groups[] = "Hbrtc";  // display order; HEAD has no header
static const wchar_t *const ref_group_labels[] = {NULL, L"BRANCHES", L"REMOTE BRANCHES", L"TAGS", L"RECENT COMMITS"};

static int refs_count(void)
{
    return app.refs ? app.refs->shown_count : 0;
}

static int refs_visible_rows(void)
{
    RECT c;
    GetClientRect(app.refs_wnd, &c);
    int rows = c.bottom / app.row_h;
    return rows > 0 ? rows : 1;
}

// ASCII case-insensitive substring test; `needle` is lower case.
static int contains(const char *s, const char *needle, int n)
{
    for (; s && *s; s++) {
        int i = 0;
        while (i < n && s[i] && (s[i] | (s[i] >= 'A' && s[i] <= 'Z' ? 0x20 : 0)) == needle[i])
            i++;
        if (i == n)
            return 1;
    }
    return 0;
}

// Fills refs->shown from the box text and highlights the ref named by it, else (when typed) the first match.
static void refs_filter(void)
{
    RefList *r = app.refs;
    app.refs_sel = -1;
    if (!r || !r->shown)
        return;
    wchar_t typed[256];
    char text[512], needle[512];
    read_edit(app.refs_edit, typed, 256);
    int len = WideCharToMultiByte(CP_UTF8, 0, typed, -1, text, sizeof text, NULL, NULL) - 1;
    len = len > 0 ? len : 0;
    text[len] = 0;
    int n = app.refs_typed ? len : 0;
    for (int i = 0; i < n; i++)
        needle[i] = text[i] >= 'A' && text[i] <= 'Z' ? (char)(text[i] | 0x20) : text[i];

    int exact = -1, first = -1;
    r->shown_count = 0;
    for (int g = 0; g < 5; g++) {
        int header = 0;
        for (int i = 0; i < r->count; i++) {
            const RefEntry *e = &r->refs[i];
            if (e->kind != ref_groups[g] || !(n == 0 || contains(e->name, needle, n) || contains(e->detail, needle, n)))
                continue;
            if (!header && g > 0)
                r->shown[r->shown_count++] = -1 - g;
            header = 1;
            if (first < 0)
                first = r->shown_count;
            if (exact < 0 && len && lstrcmpA(e->name, text) == 0)
                exact = r->shown_count;
            r->shown[r->shown_count++] = i;
        }
    }
    app.refs_sel = exact >= 0 ? exact : n ? first : -1;
}

static void refs_scroll_to(int top)
{
    int max_top = refs_count() - refs_visible_rows();
    top = top > max_top ? max_top : top;
    app.refs_top = top > 0 ? top : 0;
    SCROLLINFO si = {sizeof si, SIF_RANGE | SIF_PAGE | SIF_POS, 0, refs_count() - 1, (UINT)refs_visible_rows(),
                     app.refs_top};
    SetScrollInfo(app.refs_wnd, SB_VERT, &si, TRUE);
    InvalidateRect(app.refs_wnd, NULL, FALSE);
}

// Scrolls row `i` into view, with its group header when it is the group's first ref.
static void refs_reveal(int i)
{
    if (i < 0)
        return;
    int show = i > 0 && app.refs->shown[i - 1] < 0 ? i - 1 : i;
    if (show < app.refs_top)
        refs_scroll_to(show);
    else if (i >= app.refs_top + refs_visible_rows())
        refs_scroll_to(i - refs_visible_rows() + 1);
    InvalidateRect(app.refs_wnd, NULL, FALSE);
}

// Sizes the dropdown to its rows, under the box, inside the window, and shows it without taking focus.
static void refs_place(void)
{
    const RECT *f = &app.frames[app.refs_edit];
    RECT c;
    GetClientRect(app.main, &c);
    int rows = refs_count() ? refs_count() : 1;
    rows = rows < REFS_ROWS ? rows : REFS_ROWS;
    int w = scale(REFS_W), x = f->left;
    if (x + w > c.right - scale(PAD))
        x = c.right - scale(PAD) - w;
    POINT p = {x > 0 ? x : 0, f->bottom + scale(2)};
    ClientToScreen(app.main, &p);
    RECT outer = {0, 0, w, rows * app.row_h};  // border width depends on the DPI
    AdjustWindowRectExForDpi(&outer, WS_POPUP | WS_BORDER, FALSE, WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, app.dpi);
    SetWindowPos(app.refs_wnd, HWND_TOP, p.x, p.y, w, outer.bottom - outer.top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    refs_scroll_to(app.refs_top);
    refs_reveal(app.refs_sel);
}

static void refs_hide(void)
{
    if (app.refs_edit < 0)
        return;
    app.refs_edit = -1;
    ShowWindow(app.refs_wnd, SW_HIDE);
}

static void refs_open(int which)
{
    if (!app.refs_wnd) {
        app.refs_wnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, L"DiffViewerRefs", NULL,
                                       WS_POPUP | WS_BORDER | WS_VSCROLL, 0, 0, 0, 0, app.main, NULL,
                                       GetModuleHandleW(NULL), NULL);
        SetWindowTheme(app.refs_wnd, app.theme == &dark_theme ? L"DarkMode_Explorer" : L"Explorer", NULL);
    }
    // Read every time it opens, so new branches show up; the last list is shown meanwhile.
    const wchar_t *root = app.status ? app.status->root : L"";
    if (app.refs && lstrcmpW(app.refs->root, root) != 0) {
        refs_free(app.refs);
        app.refs = NULL;
    }
    if (root[0])
        git_request_refs(root, ++app.refs_generation);
    app.refs_edit = which;
    app.refs_typed = 0;
    app.refs_top = 0;
    refs_filter();
    refs_place();
}

static void refs_move(int delta)
{
    int count = refs_count(), dir = delta > 0 ? 1 : -1;
    if (count == 0)
        return;
    int i = (app.refs_sel >= 0 ? app.refs_sel : dir > 0 ? -1 : count) + delta;
    i = i < 0 ? 0 : i >= count ? count - 1 : i;
    int from = i;
    while (i >= 0 && i < count && app.refs->shown[i] < 0)
        i += dir;
    if (i < 0 || i >= count) {  // only headers that way: go back to the nearest ref
        for (i = from; i >= 0 && i < count && app.refs->shown[i] < 0; i -= dir) {
        }
    }
    if (i < 0 || i >= count)
        return;
    app.refs_sel = i;
    refs_reveal(i);
}

// Puts the highlighted ref into the box and reloads; returns 0 when nothing is highlighted.
static int refs_pick(void)
{
    if (app.refs_sel < 0 || app.refs_edit < 0)
        return 0;
    const RefEntry *e = &app.refs->refs[app.refs->shown[app.refs_sel]];
    wchar_t name[256];
    int n = utf8_to_wide(e->name, lstrlenA(e->name), name, 255);
    name[n] = 0;
    HWND edit = app.edits[app.refs_edit];
    refs_hide();
    app.refs_setting_text = 1;
    SetWindowTextW(edit, name);
    app.refs_setting_text = 0;
    SendMessageW(edit, EM_SETSEL, n, n);
    reload();
    return 1;
}

static void on_refs(RefList *r)
{
    if (r->generation != app.refs_generation) {
        refs_free(r);
        return;
    }
    if (app.refs)
        refs_free(app.refs);
    app.refs = r;
    if (app.refs_edit >= 0) {
        refs_filter();
        refs_place();
    }
}

static void refs_paint(HWND hwnd)
{
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(hwnd, &ps);
    RECT c;
    GetClientRect(hwnd, &c);
    const Theme *t = app.theme;
    const RefList *r = app.refs;
    int count = refs_count(), dy = (app.row_h - app.text_h) / 2;
    SetBkMode(dc, TRANSPARENT);
    for (int row = app.refs_top, y = 0; y < c.bottom; row++, y += app.row_h) {
        RECT box = {0, y, c.right, y + app.row_h};
        fill(dc, &box, row == app.refs_sel ? t->sel : t->panel);
        if (row >= count)
            continue;
        int s = r->shown[row];
        if (s < 0) {
            const wchar_t *label = ref_group_labels[-1 - s];
            SelectObject(dc, app.bold_font);
            SetTextColor(dc, t->dim);
            ExtTextOutW(dc, scale(10), y + dy, ETO_CLIPPED, &box, label, (UINT)lstrlenW(label), NULL);
            continue;
        }
        const RefEntry *e = &r->refs[s];
        wchar_t text[512];
        int n = utf8_to_wide(e->name, lstrlenA(e->name), text, 512);
        SelectObject(dc, app.ui_font);
        SetTextColor(dc, t->fg);
        ExtTextOutW(dc, scale(18), y + dy, ETO_CLIPPED, &box, text, (UINT)n, NULL);
        if (e->detail) {
            SIZE size;
            GetTextExtentPoint32W(dc, text, n, &size);
            n = utf8_to_wide(e->detail, lstrlenA(e->detail), text, 512);
            SetTextColor(dc, t->dim);
            ExtTextOutW(dc, scale(18) + size.cx + scale(10), y + dy, ETO_CLIPPED, &box, text, (UINT)n, NULL);
        }
    }
    const wchar_t *message = NULL;
    if (count == 0) {
        message = !app.status || !app.status->root[0] ? L"Open a git repo first."
                  : !r                                ? L"Loading\x2026"
                  : r->error[0]                       ? r->error
                                                      : L"No matching refs.";
    }
    if (message) {
        RECT box = {scale(10), dy, c.right - scale(10), c.bottom};
        SelectObject(dc, app.ui_font);
        SetTextColor(dc, t->dim);
        DrawTextW(dc, message, -1, &box, DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    }
    EndPaint(hwnd, &ps);
}

static LRESULT CALLBACK refs_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_PAINT:
        refs_paint(hwnd);
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_MOUSEACTIVATE:
        return MA_NOACTIVATE;  // the ref box keeps focus, so typing still filters
    case WM_VSCROLL:
        refs_scroll_to(scroll_target(hwnd, SB_VERT, wp, app.refs_top, refs_visible_rows(), 1));
        return 0;
    case WM_MOUSEWHEEL:
        refs_scroll_to(app.refs_top - GET_WHEEL_DELTA_WPARAM(wp) / 40);
        return 0;
    case WM_MOUSEMOVE:
    case WM_LBUTTONDOWN: {
        int row = app.refs_top + (short)HIWORD(lp) / app.row_h;
        if (row < refs_count() && app.refs->shown[row] >= 0 && row != app.refs_sel) {
            app.refs_sel = row;
            InvalidateRect(hwnd, NULL, FALSE);
        }
        if (msg == WM_LBUTTONDOWN && row == app.refs_sel)
            refs_pick();
        return 0;
    }
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

static LRESULT CALLBACK edit_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    int ref_box = hwnd == app.edits[EDIT_BASE] ? EDIT_BASE : hwnd == app.edits[EDIT_TARGET] ? EDIT_TARGET : -1;
    int open = ref_box >= 0 && app.refs_edit == ref_box;
    switch (msg) {
    case WM_KEYDOWN:
        if (open) {
            switch (wp) {
            case VK_DOWN: refs_move(1); return 0;
            case VK_UP: refs_move(-1); return 0;
            case VK_NEXT: refs_move(refs_visible_rows()); return 0;
            case VK_PRIOR: refs_move(-refs_visible_rows()); return 0;
            case VK_ESCAPE: refs_hide(); return 0;
            case VK_RETURN:
                if (refs_pick())
                    return 0;
                refs_hide();
                break;
            }
        } else if (ref_box >= 0 && wp == VK_DOWN) {
            refs_open(ref_box);
            return 0;
        }
        if (wp == VK_RETURN) {
            reload();
            return 0;
        }
        if (wp == VK_TAB) {
            move_focus(hwnd, GetKeyState(VK_SHIFT) < 0);
            return 0;
        }
        break;
    case WM_SYSKEYDOWN:
        if (ref_box >= 0 && wp == VK_DOWN) {  // Alt+Down, as in a combo box
            if (open)
                refs_hide();
            else
                refs_open(ref_box);
            return 0;
        }
        break;
    case WM_CHAR:
        if (wp == '\r' || wp == '\t' || wp == 27)
            return 0;  // the edit control would beep
        break;
    case WM_LBUTTONDOWN:
        if (ref_box >= 0 && !open) {
            LRESULT result = CallWindowProcW(edit_proc_original, hwnd, msg, wp, lp);  // focus and caret first
            refs_open(ref_box);
            return result;
        }
        break;
    case WM_MOUSEWHEEL:
        if (open)
            return SendMessageW(app.refs_wnd, msg, wp, lp);
        break;
    case WM_KILLFOCUS:
        refs_hide();
        InvalidateRect(hwnd, NULL, TRUE);  // shows the hint
        break;
    case WM_SETFOCUS:
        InvalidateRect(hwnd, NULL, TRUE);  // hides the hint
        break;
    case WM_PAINT: {
        // The plain EDIT control has no placeholder text (that needs comctl32 v6), so draw it here.
        LRESULT result = CallWindowProcW(edit_proc_original, hwnd, msg, wp, lp);
        const wchar_t *hint = hwnd == app.edits[EDIT_BASE] ? L"base ref" : hwnd == app.edits[EDIT_TARGET] ? L"target ref" : NULL;
        if (hint && GetWindowTextLengthW(hwnd) == 0 && GetFocus() != hwnd) {
            HDC dc = GetDC(hwnd);
            SelectObject(dc, app.ui_font);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, app.theme->dim);
            TextOutW(dc, LOWORD(SendMessageW(hwnd, EM_GETMARGINS, 0, 0)), 0, hint, lstrlenW(hint));
            ReleaseDC(hwnd, dc);
        }
        return result;
    }
    }
    return CallWindowProcW(edit_proc_original, hwnd, msg, wp, lp);
}

// Two buttons side by side in `box`; `selected` (0, 1, or -1 for none) is drawn highlighted.
static void paint_pair(HDC dc, const RECT *box, const wchar_t *left, const wchar_t *right, int selected)
{
    const Theme *t = app.theme;
    const wchar_t *labels[2] = {left, right};
    int mid = (box->left + box->right) / 2;
    SelectObject(dc, app.ui_font);
    for (int i = 0; i < 2; i++) {
        RECT seg = {i ? mid : box->left, box->top, i ? box->right : mid, box->bottom};
        fill(dc, &seg, selected == i ? t->sel : t->edit_bg);
        SetTextColor(dc, selected == i || selected < 0 ? t->fg : t->dim);
        DrawTextW(dc, labels[i], -1, &seg, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    }
    RECT divider = {mid, box->top, mid + 1, box->bottom};
    fill(dc, &divider, t->border);
    FrameRect(dc, box, app.border_brush);
}

// Requests the open file's diff again (after a setting changed), keeping the scroll position.
static void reload_diff(void)
{
    app.keep_scroll_generation = app.diff_generation + 1;
    git_request_diff(app.status, &app.status->rows[app.sel], ++app.diff_generation, app.language_pick, app.ignore_eol);
}

static int language_field_shown(void)
{
    return !app.loading && app.diff && !app.diff->error[0] && !app.diff->binary;
}

// The highlighter menu above the status bar field. A pick reloads the open diff in that language.
static void language_menu(void)
{
    HMENU menu = CreatePopupMenu();
    for (int lang = LANG_AUTO; lang < LANG_COUNT; lang++) {
        AppendMenuW(menu, MF_STRING | (lang == app.language_pick ? MF_CHECKED : 0), lang + 1, language_names[lang]);
        if (lang == LANG_PLAIN)
            AppendMenuW(menu, MF_SEPARATOR, 0, NULL);
    }
    POINT p = {app.language_field.right, app.language_field.top};
    ClientToScreen(app.main, &p);
    // Ids are LANG_* + 1: TrackPopupMenu returns 0 when the menu is dismissed.
    int id = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTALIGN | TPM_BOTTOMALIGN, p.x, p.y, 0,
                            app.main, NULL);
    DestroyMenu(menu);
    if (id == 0 || id - 1 == app.language_pick)
        return;
    app.language_pick = id - 1;
    reload_diff();
}

static void main_paint(HWND hwnd)
{
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(hwnd, &ps);
    RECT c;
    GetClientRect(hwnd, &c);
    const Theme *t = app.theme;
    int bar = scale(BAR_H), status_h = scale(STATUS_H);
    RECT top = {0, 0, c.right, bar};
    fill(dc, &top, t->bg);
    for (int i = 0; i < EDIT_COUNT; i++) {
        FillRect(dc, &app.frames[i], app.edit_brush);
        FrameRect(dc, &app.frames[i], app.border_brush);
    }
    SetBkMode(dc, TRANSPARENT);
    SelectObject(dc, app.ui_font);
    SetTextColor(dc, t->dim);
    RECT arrow = {app.frames[EDIT_BASE].right, app.frames[EDIT_BASE].top, app.frames[EDIT_TARGET].left,
                  app.frames[EDIT_BASE].bottom};
    DrawTextW(dc, L"\x2192", 1, &arrow, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    paint_pair(dc, &app.nav, L"\x25B2  Prev", L"\x25BC  Next", -1);
    paint_pair(dc, &app.toggle, L"Unified", L"Side by side", app.side_by_side);

    RECT split = split_rect();
    fill(dc, &split, t->bg);
    split.right = split.left + 1;  // the visible line, against the list
    fill(dc, &split, t->border);

    RECT status = {0, c.bottom - status_h, c.right, c.bottom};
    fill(dc, &status, t->panel);
    wchar_t text[1200];
    text[0] = 0;
    const StatusResult *s = app.status;
    if (app.loading)
        lstrcpyW(text, L"Loading\x2026");
    else if (s && s->error[0])
        lstrcpynW(text, s->error, 600);
    else if (s && s->base[0])
        wsprintfW(text, L"%s \x2192 %s   \x00b7   %d file%s differ%s   \x00b7   %lu ms", s->base,
                  s->target[0] ? s->target : L"working tree", s->file_count, s->file_count == 1 ? L"" : L"s",
                  s->file_count == 1 ? L"s" : L"", s->elapsed_ms);
    else if (s)
        wsprintfW(text, L"%d changed file%s   \x00b7   git status %lu ms", s->file_count,
                  s->file_count == 1 ? L"" : L"s", s->elapsed_ms);
    if (!app.loading && app.diff && !app.diff->error[0]) {
        int len = lstrlenW(text);
        wsprintfW(text + len, L"   \x00b7   +%d \x2212%d   \x00b7   diff %lu ms", app.diff->added, app.diff->removed,
                  app.diff->elapsed_ms);
    }
    DWORD kb = (DWORD)(memory_counters().PrivateWorkingSetSize / 1024);
    int len = lstrlenW(text);
    wsprintfW(text + len, L"%s%lu.%lu MB", len ? L"   \x00b7   " : L"", kb / 1024, kb % 1024 * 10 / 1024);
    RECT text_clip = status;
    if (s && s->root[0]) {
        text_clip.right = app.eol_field.left;
        int box = app.text_h * 3 / 4;
        RECT check = {app.eol_field.left, status.top + (status_h - box) / 2, app.eol_field.left + box,
                      status.top + (status_h + box) / 2};
        FrameRect(dc, &check, app.border_brush);
        if (app.ignore_eol) {  // a filled square, not a check glyph: that glyph maps in a fallback font
            RECT mark = {check.left + scale(3), check.top + scale(3), check.right - scale(3), check.bottom - scale(3)};
            fill(dc, &mark, t->fg);
        }
        SetTextColor(dc, t->fg);
        RECT label = {check.right + scale(6), status.top, app.eol_field.right, status.bottom};
        DrawTextW(dc, L"Ignore line endings", -1, &label, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }
    if (language_field_shown()) {
        wchar_t field[32];
        wsprintfW(field, L"%s%s  \x25BE", app.diff->language_auto ? L"Auto: " : L"",
                  language_names[app.diff->language]);
        SetTextColor(dc, t->fg);
        DrawTextW(dc, field, -1, &app.language_field, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    }
    SetTextColor(dc, t->dim);
    ExtTextOutW(dc, scale(PAD), status.top + (status_h - app.text_h) / 2, ETO_CLIPPED, &text_clip, text,
                (UINT)lstrlenW(text), NULL);
    EndPaint(hwnd, &ps);
}

static void clear_diff(void)
{
    if (app.diff)
        diff_free(app.diff);
    app.diff = NULL;
    app.diff_generation++;  // a diff still loading for the old list is dropped when it arrives
    diff_scroll_to(0, 0);
}

static void on_status(StatusResult *r)
{
    // After an auto-refresh, select the same file in the same group if it is still there.
    int sel = 0, keep_top = 0, same_file = 0;
    if (r->auto_refresh && app.status && app.sel >= 0) {
        const ListRow *old = &app.status->rows[app.sel];
        const char *old_path = app.status->files[old->file].path;
        sel = app.sel < r->row_count ? app.sel : r->row_count - 1;
        for (int i = 0; i < r->row_count && !same_file; i++) {
            const ListRow *row = &r->rows[i];
            if (row->type == old->type && lstrcmpA(r->files[row->file].path, old_path) == 0) {
                sel = i;
                same_file = 1;
            }
        }
        keep_top = app.top;
    }
    if (app.status)
        status_free(app.status);
    app.status = r;
    app.loading = 0;
    app.sel = -1;
    if (!same_file)
        app.language_pick = LANG_AUTO;
    if (!r->auto_refresh && !r->error[0])
        SetWindowTextW(app.edits[EDIT_PATH], r->root);
    list_scroll_to(keep_top);
    InvalidateRect(app.main, NULL, FALSE);
    if (r->row_count == 0) {
        clear_diff();
        if (app.perf)
            perf_finish(r->error);
        return;
    }
    if (same_file && app.diff && app.diff->generation == app.diff_generation &&
        diff_is_current(app.diff, r, &r->rows[sel]))
        app.sel = sel;  // the file did not change: list_select keeps the open diff
    else if (same_file)
        app.keep_scroll_generation = app.diff_generation + 1;  // the request list_select makes below
    if (!r->auto_refresh)
        clear_diff();
    list_select(sel, 1);
}

static void on_diff(DiffResult *r)
{
    if (r->generation != app.diff_generation) {
        diff_free(r);  // the selection moved on while this was loading
        return;
    }
    if (app.diff)
        diff_free(app.diff);
    app.diff = r;
    app.change_row = -1;
    if (r->generation == app.change_generation) {
        int row = find_change(app.change_dir > 0 ? -1 : diff_rows(), app.change_dir);
        if (row >= 0)
            show_change(row);
        else
            diff_scroll_to(0, 0);
    } else if (r->generation == app.keep_scroll_generation)
        diff_scroll_to(app.diff_top, app.diff_left);
    else {
        diff_scroll_to(0, 0);
        int first = find_change(-1, 1);
        if (first >= 0 && first < diff_visible_rows())
            app.change_row = first;  // already on screen, so the first Next goes past it
    }
    InvalidateRect(app.main, NULL, FALSE);
    if (app.perf)
        perf_finish(r->error);
}

static LRESULT CALLBACK main_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_CREATE: {
        app.main = hwnd;
        app.dpi = GetDpiForWindow(hwnd);
        make_fonts();
        HINSTANCE inst = GetModuleHandleW(NULL);
        for (int i = 0; i < EDIT_COUNT; i++) {
            app.edits[i] = CreateWindowExW(0, L"EDIT", i == EDIT_PATH ? app.folder : L"",
                                           WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL, 0, 0, 0, 0, hwnd, NULL, inst, NULL);
            edit_proc_original = (WNDPROC)SetWindowLongPtrW(app.edits[i], GWLP_WNDPROC, (LONG_PTR)edit_proc);
            SendMessageW(app.edits[i], WM_SETFONT, (WPARAM)app.ui_font, FALSE);
        }
        app.list = CreateWindowExW(0, L"DiffViewerList", NULL, WS_CHILD | WS_VISIBLE | WS_VSCROLL,
                                   0, 0, 0, 0, hwnd, NULL, inst, NULL);
        app.diff_wnd = CreateWindowExW(0, L"DiffViewerDiff", NULL, WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_HSCROLL,
                                       0, 0, 0, 0, hwnd, NULL, inst, NULL);
        apply_theme();
        return 0;
    }
    case WM_SIZE:
        refs_hide();
        layout();
        InvalidateRect(hwnd, NULL, FALSE);
        return 0;
    case WM_MOVE:
        refs_hide();  // it would stay where the box was
        return 0;
    case WM_COMMAND: {
        // Typing in a ref box opens the dropdown and filters it.
        int which = (HWND)lp == app.edits[EDIT_BASE] ? EDIT_BASE : (HWND)lp == app.edits[EDIT_TARGET] ? EDIT_TARGET : -1;
        if (HIWORD(wp) == EN_CHANGE && which >= 0 && !app.refs_setting_text && GetFocus() == (HWND)lp) {
            if (app.refs_edit != which)
                refs_open(which);
            app.refs_typed = 1;
            app.refs_top = 0;
            refs_filter();
            refs_place();
        }
        return 0;
    }
    case WM_PAINT:
        main_paint(hwnd);
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_LBUTTONDOWN: {
        refs_hide();
        POINT p = {(short)LOWORD(lp), (short)HIWORD(lp)};
        if (PtInRect(&app.toggle, p) && (p.x >= (app.toggle.left + app.toggle.right) / 2) != app.side_by_side)
            toggle_view();
        if (PtInRect(&app.nav, p))
            goto_change(p.x >= (app.nav.left + app.nav.right) / 2 ? 1 : -1);
        if (PtInRect(&app.language_field, p) && language_field_shown())
            language_menu();
        if (PtInRect(&app.eol_field, p) && app.status && app.status->root[0]) {
            app.ignore_eol = !app.ignore_eol;
            InvalidateRect(hwnd, &app.eol_field, FALSE);
            if (language_field_shown())
                reload_diff();
        }
        RECT split = split_rect();
        if (PtInRect(&split, p)) {
            app.drag_dx = p.x - split.left;
            SetCapture(hwnd);
        }
        return 0;
    }
    case WM_MOUSEMOVE:
        if (app.drag_dx >= 0) {
            app.list_w96 = MulDiv((short)LOWORD(lp) - app.drag_dx, 96, (int)app.dpi);
            app.list_w96 = MulDiv(list_width(), 96, (int)app.dpi);  // keep it inside the limits
            layout();
            InvalidateRect(hwnd, NULL, FALSE);
        }
        return 0;
    case WM_LBUTTONUP:
        ReleaseCapture();
        return 0;
    case WM_CAPTURECHANGED:
        app.drag_dx = -1;
        return 0;
    case WM_SETCURSOR: {
        POINT p;
        GetCursorPos(&p);
        ScreenToClient(hwnd, &p);
        RECT split = split_rect();
        if ((HWND)wp == hwnd && PtInRect(&split, p)) {
            SetCursor(LoadCursorW(NULL, (LPCWSTR)IDC_SIZEWE));
            return TRUE;
        }
        break;
    }
    case WM_SETFOCUS:
        SetFocus(app.list);
        return 0;
    case WM_CTLCOLOREDIT:
        SetTextColor((HDC)wp, app.theme->fg);
        SetBkColor((HDC)wp, app.theme->edit_bg);
        return (LRESULT)app.edit_brush;
    case WM_DPICHANGED: {
        app.dpi = LOWORD(wp);
        make_fonts();
        const RECT *r = (const RECT *)lp;
        SetWindowPos(hwnd, NULL, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
        layout();
        RedrawWindow(hwnd, NULL, NULL, RDW_INVALIDATE | RDW_ALLCHILDREN);
        return 0;
    }
    case WM_SETTINGCHANGE:
        if (lp && lstrcmpW((const wchar_t *)lp, L"ImmersiveColorSet") == 0) {
            apply_theme();
            RedrawWindow(hwnd, NULL, NULL, RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME);
        }
        return 0;
    case WM_APP_STATUS:
        on_status((StatusResult *)lp);
        return 0;
    case WM_APP_DIFF:
        on_diff((DiffResult *)lp);
        return 0;
    case WM_APP_REFS:
        on_refs((RefList *)lp);
        return 0;
    case WM_TIMER:
        perf_print("error: timed out waiting for git\r\n");
        app.exit_code = 2;
        DestroyWindow(hwnd);
        return 0;
    case WM_DESTROY:
        PostQuitMessage(app.exit_code);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

// Copies the next command-line argument, quotes removed, into `out`; returns 0 when there are none.
static int next_arg(const wchar_t **cursor, wchar_t *out, int cap)
{
    const wchar_t *p = *cursor;
    while (*p == ' ' || *p == '\t')
        p++;
    if (!*p)
        return 0;
    int n = 0, quoted = 0;
    for (; *p; p++) {
        if (*p == '"') {
            quoted = !quoted;
            continue;
        }
        if (!quoted && (*p == ' ' || *p == '\t'))
            break;
        if (n < cap - 1)
            out[n++] = *p;
    }
    out[n] = 0;
    *cursor = p;
    return 1;
}

static int run(void)
{
    // DiffViewer.exe [--perf] [folder]; the folder defaults to the current directory.
    const wchar_t *cursor = GetCommandLineW();
    wchar_t arg[1024];
    next_arg(&cursor, arg, 1024);
    GetCurrentDirectoryW(1024, app.folder);
    while (next_arg(&cursor, arg, 1024)) {
        if (lstrcmpW(arg, L"--perf") == 0)
            app.perf = 1;
        else
            GetFullPathNameW(arg, 1024, app.folder, NULL);
    }

    HINSTANCE inst = GetModuleHandleW(NULL);
    HCURSOR arrow = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    WNDCLASSW main_class = {0, main_proc, 0, 0, inst, NULL, arrow, NULL, NULL, L"DiffViewer"};
    WNDCLASSW list_class = {0, list_proc, 0, 0, inst, NULL, arrow, NULL, NULL, L"DiffViewerList"};
    WNDCLASSW diff_class = {0, diff_proc, 0, 0, inst, NULL, LoadCursorW(NULL, (LPCWSTR)IDC_IBEAM), NULL, NULL,
                            L"DiffViewerDiff"};
    WNDCLASSW refs_class = {CS_DROPSHADOW, refs_proc, 0, 0, inst, NULL, arrow, NULL, NULL, L"DiffViewerRefs"};
    if (!RegisterClassW(&main_class) || !RegisterClassW(&list_class) || !RegisterClassW(&diff_class) ||
        !RegisterClassW(&refs_class))
        fatal(L"RegisterClass");

    app.loading = 1;
    app.sel = -1;
    app.side_by_side = 1;
    app.change_row = -1;
    app.list_w96 = LIST_W;
    app.ignore_eol = 1;
    app.drag_dx = -1;
    app.refs_edit = -1;
    HWND hwnd = CreateWindowExW(0, L"DiffViewer", L"DiffViewer", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                                CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, NULL, NULL, inst, NULL);
    if (!hwnd)
        fatal(L"CreateWindow");
    git_start(hwnd);
    git_request_status(app.folder, L"", L"");
    ShowWindow(hwnd, app.perf ? SW_SHOWNOACTIVATE : SW_SHOWDEFAULT);
    UpdateWindow(hwnd);
    if (app.perf)
        SetTimer(hwnd, 1, 30000, NULL);

    MSG msg;
    while (GetMessageW(&msg, NULL, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    return (int)msg.wParam;
}

void entry(void)
{
    ExitProcess((UINT)run());
}
