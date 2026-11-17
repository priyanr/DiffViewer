#![windows_subsystem = "windows"]
#![allow(static_mut_refs, dead_code)]

mod git;
mod syntax;
mod types;

use std::ptr::{null, null_mut};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::ProcessStatus::GetProcessMemoryInfo;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Environment::GetCurrentDirectoryW;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows_sys::Win32::UI::Controls::{SetScrollInfo, SetWindowTheme, EM_GETMARGINS, EM_SETSEL};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use git::*;
use syntax::*;
use types::*;

extern "system" {
    fn GetTickCount() -> u32;
    fn GetSystemTimePreciseAsFileTime(lpSystemTimeAsFileTime: *mut FILETIME);
    fn GetCommandLineW() -> *const u16;
}

const PAD: i32 = 8;
const BAR_H: i32 = 40;
const LIST_W: i32 = 340;
const STATUS_H: i32 = 24;
const REF_W: i32 = 150;
const ARROW_W: i32 = 24;
const TOGGLE_W: i32 = 180;
const NAV_W: i32 = 150;
const SPLIT_W: i32 = 5;
const MIN_LIST_W: i32 = 120;
const MIN_DIFF_W: i32 = 200;
const CHANGE_CONTEXT: i32 = 3;
const REFS_W: i32 = 440;
const REFS_ROWS: i32 = 14;
const LANGUAGE_W: i32 = 150;
const EOL_W: i32 = 170;

const EDIT_PATH: usize = 0;
const EDIT_BASE: usize = 1;
const EDIT_TARGET: usize = 2;
const EDIT_COUNT: usize = 3;

const SCROLL_END: i32 = 0x3FFFFFFF;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const MK_SHIFT: usize = 0x0004;

#[repr(C)]
struct MemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_usage: usize,
    private_working_set_size: usize,
    shared_commit_usage: u64,
}

struct AppState {
    main: HWND,
    list: HWND,
    diff_wnd: HWND,
    edits: [HWND; EDIT_COUNT],
    frames: [RECT; EDIT_COUNT],
    toggle: RECT,
    nav: RECT,
    language_field: RECT,
    eol_field: RECT,
    dpi: u32,
    theme: &'static Theme,
    ui_font: HFONT,
    bold_font: HFONT,
    code_font: HFONT,
    edit_brush: HBRUSH,
    border_brush: HBRUSH,
    text_h: i32,
    row_h: i32,
    code_h: i32,
    code_row_h: i32,
    char_w: i32,
    folder: String,
    status: Option<Box<StatusResult>>,
    loading: bool,
    sel: i32,
    top: i32,
    wheel_rest: i32,
    list_w96: i32,
    drag_dx: i32,
    diff: Option<Box<DiffResult>>,
    diff_generation: i32,
    keep_scroll_generation: i32,
    language_pick: usize,
    ignore_eol: bool,
    side_by_side: bool,
    diff_top: i32,
    diff_left: i32,
    diff_wheel_rest: i32,
    change_row: i32,
    change_generation: i32,
    change_dir: i32,
    refs_wnd: HWND,
    refs: Option<Box<RefList>>,
    refs_edit: i32,
    refs_generation: i32,
    refs_top: i32,
    refs_sel: i32,
    refs_typed: bool,
    refs_setting_text: bool,
    perf: bool,
    exit_code: i32,
    startup_ms: u32,
}

static mut APP: AppState = AppState {
    main: null_mut(),
    list: null_mut(),
    diff_wnd: null_mut(),
    edits: [null_mut(); EDIT_COUNT],
    frames: [RECT { left: 0, top: 0, right: 0, bottom: 0 }; EDIT_COUNT],
    toggle: RECT { left: 0, top: 0, right: 0, bottom: 0 },
    nav: RECT { left: 0, top: 0, right: 0, bottom: 0 },
    language_field: RECT { left: 0, top: 0, right: 0, bottom: 0 },
    eol_field: RECT { left: 0, top: 0, right: 0, bottom: 0 },
    dpi: 96,
    theme: &DARK_THEME,
    ui_font: null_mut(),
    bold_font: null_mut(),
    code_font: null_mut(),
    edit_brush: null_mut(),
    border_brush: null_mut(),
    text_h: 16,
    row_h: 24,
    code_h: 16,
    code_row_h: 18,
    char_w: 8,
    folder: String::new(),
    status: None,
    loading: false,
    sel: -1,
    top: 0,
    wheel_rest: 0,
    list_w96: LIST_W,
    drag_dx: -1,
    diff: None,
    diff_generation: 0,
    keep_scroll_generation: 0,
    language_pick: LANG_AUTO,
    ignore_eol: true,
    side_by_side: true,
    diff_top: 0,
    diff_left: 0,
    diff_wheel_rest: 0,
    change_row: -1,
    change_generation: 0,
    change_dir: 0,
    refs_wnd: null_mut(),
    refs: None,
    refs_edit: -1,
    refs_generation: 0,
    refs_top: 0,
    refs_sel: -1,
    refs_typed: false,
    refs_setting_text: false,
    perf: false,
    exit_code: 0,
    startup_ms: 0,
};

static mut EDIT_PROC_ORIGINAL: WNDPROC = None;

fn scale(v: i32) -> i32 {
    unsafe { ((v as i64 * APP.dpi as i64) / 96) as i32 }
}

fn fill(dc: HDC, r: &RECT, color: COLORREF) {
    unsafe {
        SetBkColor(dc, color);
        ExtTextOutW(dc, 0, 0, ETO_OPAQUE as u32, r, null(), 0, null());
    }
}

fn scroll_target(hwnd: HWND, bar: i32, wp: WPARAM, pos: i32, page: i32, step: i32) -> i32 {
    let code = (wp & 0xFFFF) as i32;
    match code {
        SB_LINEUP => pos - step,
        SB_LINEDOWN => pos + step,
        SB_PAGEUP => pos - page,
        SB_PAGEDOWN => pos + page,
        SB_TOP => 0,
        SB_BOTTOM => SCROLL_END,
        SB_THUMBTRACK | SB_THUMBPOSITION => unsafe {
            let mut si: SCROLLINFO = std::mem::zeroed();
            si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
            si.fMask = SIF_TRACKPOS;
            GetScrollInfo(hwnd, bar, &mut si);
            si.nTrackPos
        },
        _ => pos,
    }
}

fn ms_since_process_start() -> u32 {
    unsafe {
        let proc = GetCurrentProcess();
        let mut created: FILETIME = std::mem::zeroed();
        let mut exited: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let mut now: FILETIME = std::mem::zeroed();
        GetProcessTimes(proc, &mut created, &mut exited, &mut kernel, &mut user);
        GetSystemTimePreciseAsFileTime(&mut now);
        let start = ((created.dwHighDateTime as u64) << 32) | (created.dwLowDateTime as u64);
        let end = ((now.dwHighDateTime as u64) << 32) | (now.dwLowDateTime as u64);
        ((end - start) / 10000) as u32
    }
}

fn apply_theme() {
    unsafe {
        let mut light: u32 = 1;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let subkey = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let valname = to_wide("AppsUseLightTheme");
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            valname.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut light as *mut _ as *mut _,
            &mut size,
        );
        let dark = (light == 0) as BOOL;
        APP.theme = if dark != 0 { &DARK_THEME } else { &LIGHT_THEME };
        DwmSetWindowAttribute(APP.main, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const _, std::mem::size_of::<BOOL>() as u32);

        let theme_name = if dark != 0 { to_wide("DarkMode_Explorer") } else { to_wide("Explorer") };
        SetWindowTheme(APP.list, theme_name.as_ptr(), null());
        SetWindowTheme(APP.diff_wnd, theme_name.as_ptr(), null());
        if !APP.refs_wnd.is_null() {
            SetWindowTheme(APP.refs_wnd, theme_name.as_ptr(), null());
        }

        if !APP.edit_brush.is_null() {
            DeleteObject(APP.edit_brush);
        }
        if !APP.border_brush.is_null() {
            DeleteObject(APP.border_brush);
        }
        APP.edit_brush = CreateSolidBrush(APP.theme.edit_bg);
        APP.border_brush = CreateSolidBrush(APP.theme.border);
    }
}

fn make_fonts() {
    unsafe {
        if !APP.ui_font.is_null() { DeleteObject(APP.ui_font); }
        if !APP.bold_font.is_null() { DeleteObject(APP.bold_font); }
        if !APP.code_font.is_null() { DeleteObject(APP.code_font); }

        let h_ui = -((9 * APP.dpi as i32) / 72);
        let font_name = to_wide("Segoe UI");
        let code_font_name = to_wide("Consolas");

        APP.ui_font = CreateFontW(h_ui, 0, 0, 0, FW_NORMAL as i32, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, font_name.as_ptr());
        APP.bold_font = CreateFontW(h_ui, 0, 0, 0, FW_BOLD as i32, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, font_name.as_ptr());
        APP.code_font = CreateFontW(-((10 * APP.dpi as i32) / 72), 0, 0, 0, FW_NORMAL as i32, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, FIXED_PITCH as u32, code_font_name.as_ptr());

        let dc = GetDC(APP.main);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        SelectObject(dc, APP.ui_font);
        GetTextMetricsW(dc, &mut tm);
        APP.text_h = tm.tmHeight;
        APP.row_h = APP.text_h + scale(8);

        SelectObject(dc, APP.code_font);
        GetTextMetricsW(dc, &mut tm);
        APP.code_h = tm.tmHeight;
        APP.code_row_h = APP.code_h + scale(2);
        APP.char_w = tm.tmAveCharWidth;
        ReleaseDC(APP.main, dc);

        for &edit in &APP.edits {
            if !edit.is_null() {
                SendMessageW(edit, WM_SETFONT, APP.ui_font as usize, 1);
            }
        }
    }
}

fn list_width() -> i32 {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.main, &mut c);
        let w = scale(APP.list_w96);
        let max = c.right - scale(SPLIT_W) - scale(MIN_DIFF_W);
        let mut res = if w > max { max } else { w };
        if res < scale(MIN_LIST_W) {
            res = scale(MIN_LIST_W);
        }
        if res < c.right { res } else { c.right }
    }
}

fn split_rect() -> RECT {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.main, &mut c);
        let x = list_width();
        RECT {
            left: x,
            top: scale(BAR_H),
            right: x + scale(SPLIT_W),
            bottom: c.bottom - scale(STATUS_H),
        }
    }
}

fn layout() {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.main, &mut c);
        let bar = scale(BAR_H);
        let status = scale(STATUS_H);
        let pad = scale(PAD);

        let mut right = c.right - pad;
        APP.toggle = RECT { left: right - scale(TOGGLE_W), top: pad, right, bottom: bar - pad };
        right = APP.toggle.left - pad;
        APP.nav = RECT { left: right - scale(NAV_W), top: pad, right, bottom: bar - pad };
        right = APP.nav.left - pad;
        APP.frames[EDIT_TARGET] = RECT { left: right - scale(REF_W), top: pad, right, bottom: bar - pad };
        right = APP.frames[EDIT_TARGET].left - scale(ARROW_W);
        APP.frames[EDIT_BASE] = RECT { left: right - scale(REF_W), top: pad, right, bottom: bar - pad };
        right = APP.frames[EDIT_BASE].left - pad;
        APP.frames[EDIT_PATH] = RECT { left: pad, top: pad, right: if right > pad { right } else { pad }, bottom: bar - pad };

        for i in 0..EDIT_COUNT {
            let f = &APP.frames[i];
            let w = f.right - f.left - scale(12);
            let h = APP.text_h;
            let y = f.top + (f.bottom - f.top - h) / 2;
            MoveWindow(APP.edits[i], f.left + scale(6), y, if w > 0 { w } else { 0 }, h, 1);
        }

        APP.language_field = RECT { left: c.right - scale(LANGUAGE_W), top: c.bottom - status, right: c.right - pad, bottom: c.bottom };
        APP.eol_field = RECT { left: APP.language_field.left - scale(EOL_W), top: c.bottom - status, right: APP.language_field.left - pad, bottom: c.bottom };

        let list_w = list_width();
        let pane_h = (c.bottom - bar - status).max(0);
        MoveWindow(APP.list, 0, bar, list_w, pane_h, 1);
        let split = scale(SPLIT_W);
        let diff_w = (c.right - list_w - split).max(0);
        MoveWindow(APP.diff_wnd, list_w + split, bar, diff_w, pane_h, 1);
    }
}

fn row_count() -> usize {
    unsafe { APP.status.as_ref().map(|s| s.rows.len()).unwrap_or(0) }
}

fn perf_print(text: &str) {
    unsafe {
        let mut out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out == INVALID_HANDLE_VALUE {
            if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
                let con_name = to_wide("CONOUT$");
                out = CreateFileW(con_name.as_ptr(), GENERIC_WRITE, FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
            }
        }
        if !out.is_null() && out != INVALID_HANDLE_VALUE {
            let bytes = text.as_bytes();
            let mut written: u32 = 0;
            windows_sys::Win32::Storage::FileSystem::WriteFile(out, bytes.as_ptr(), bytes.len() as u32, &mut written, null_mut());
        }
    }
}

fn memory_counters() -> MemoryCounters {
    unsafe {
        let mut mem: MemoryCounters = std::mem::zeroed();
        mem.cb = std::mem::size_of::<MemoryCounters>() as u32;
        GetProcessMemoryInfo(GetCurrentProcess(), &mut mem as *mut _ as *mut _, std::mem::size_of::<MemoryCounters>() as u32);
        mem
    }
}

fn perf_finish(error: &[u16]) {
    unsafe {
        UpdateWindow(APP.list);
        UpdateWindow(APP.diff_wnd);
        if !error.is_empty() {
            let err_str = from_wide(error);
            perf_print(&format!("error: {}\r\n", err_str));
            APP.exit_code = 1;
        } else {
            let mem = memory_counters();
            let priv_kb = if mem.private_working_set_size > 0 { mem.private_working_set_size / 1024 } else { mem.private_usage / 1024 };
            let peak_ws_kb = mem.peak_working_set_size / 1024;
            let status_ms = APP.status.as_ref().map(|s| s.elapsed_ms).unwrap_or(0);
            let diff_ms = APP.diff.as_ref().map(|d| d.elapsed_ms).unwrap_or(0);
            let files = APP.status.as_ref().map(|s| s.files.len()).unwrap_or(0);
            let diff_lines = APP.diff.as_ref().map(|d| d.lines.len()).unwrap_or(0);
            perf_print(&format!(
                "private_kb={} peak_ws_kb={} startup_ms={} status_ms={} diff_ms={} files={} diff_lines={}\r\n",
                priv_kb, peak_ws_kb, APP.startup_ms, status_ms, diff_ms, files, diff_lines
            ));
        }
        DestroyWindow(APP.main);
    }
}

fn diff_rows() -> usize {
    unsafe {
        match APP.diff.as_ref() {
            Some(d) => {
                if !d.error.is_empty() { 0 }
                else if APP.side_by_side { d.side.len() }
                else { d.lines.len() }
            }
            None => 0,
        }
    }
}

fn line_number_width() -> i32 {
    unsafe {
        let mut n = APP.diff.as_ref().map(|d| d.max_line_no).unwrap_or(0);
        let mut digits = 1;
        while n >= 10 {
            n /= 10;
            digits += 1;
        }
        digits.max(3) * APP.char_w
    }
}

fn unified_gutter() -> (i32, i32, i32, i32) {
    let num_w = line_number_width();
    let old_right = scale(8) + num_w;
    let new_right = old_right + scale(10) + num_w;
    let marker_x = new_right + scale(10);
    unsafe {
        let text_x = marker_x + 2 * APP.char_w;
        (old_right, new_right, marker_x, text_x)
    }
}

fn side_text_offset() -> i32 {
    scale(8) + line_number_width() + scale(10)
}

fn diff_visible_rows() -> i32 {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.diff_wnd, &mut c);
        (c.bottom / APP.code_row_h).max(1)
    }
}

fn diff_scroll_to(mut top: i32, mut left: i32) {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.diff_wnd, &mut c);
        let rows = diff_rows() as i32;
        let visible = diff_visible_rows();
        if top > rows - visible { top = rows - visible; }
        if top < 0 { top = 0; }
        let page = if APP.side_by_side { (c.right - 1) / 2 } else { c.right };
        let mut width = 0;
        if rows > 0 {
            let text_x = if APP.side_by_side { side_text_offset() } else { unified_gutter().3 };
            let max_line_len = APP.diff.as_ref().map(|d| d.max_line_len as i32).unwrap_or(0);
            width = text_x + (max_line_len + 1) * APP.char_w;
        }
        if left > width - page { left = width - page; }
        if left < 0 { left = 0; }

        APP.diff_top = top;
        APP.diff_left = left;

        let mut si: SCROLLINFO = std::mem::zeroed();
        si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
        si.fMask = SIF_RANGE | SIF_PAGE | SIF_POS;
        si.nMin = 0;
        si.nMax = (rows - 1).max(0);
        si.nPage = visible as u32;
        si.nPos = top;
        SetScrollInfo(APP.diff_wnd, SB_VERT as i32, &si, 1);

        si.nMin = 0;
        si.nMax = (width - 1).max(0);
        si.nPage = page as u32;
        si.nPos = left;
        SetScrollInfo(APP.diff_wnd, SB_HORZ as i32, &si, 1);

        InvalidateRect(APP.diff_wnd, null(), 0);
    }
}

fn wide_index_after(s: &[u8], from: usize, to: usize, mut wide: usize) -> usize {
    for &b in &s[from..to] {
        if (b & 0xC0) != 0x80 {
            wide += if b >= 0xF0 { 2 } else { 1 };
        }
    }
    wide
}

fn draw_code(dc: HDC, d: &DiffResult, index: usize, new_side: bool, x: i32, y: i32, clip: &RECT) {
    unsafe {
        let line = &d.lines[index];
        let s_full = &d.text[line.offset as usize..(line.offset + line.length) as usize];
        let s = &s_full[..s_full.len().min(8192)];
        let line_text_str = String::from_utf8_lossy(s);
        let line_text_w = to_wide(&line_text_str);
        let n = line_text_w.len() - 1;

        let mut tokens_vec = Vec::new();
        if let Some(ref states) = d.states {
            let state = states[index * 2 + if new_side { 1 } else { 0 }];
            let mut opt = Some(&mut tokens_vec);
            syntax_scan_line(d.language, s, state, &mut opt);
        }

        let saved = SaveDC(dc);
        IntersectClipRect(dc, clip.left, clip.top, clip.right, clip.bottom);
        let tab_w = 4 * APP.char_w;

        let mut lo = 0;
        let mut hi = d.word_spans.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if d.word_spans[mid].line < index {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        let word_bg = if line.kind == LINE_ADDED { APP.theme.added_word_bg } else { APP.theme.removed_word_bg };
        let mut cur = lo;
        while cur < d.word_spans.len() && d.word_spans[cur].line == index {
            let w = &d.word_spans[cur];
            if (w.start as usize) < s.len() {
                let end = (w.end as usize).min(s.len());
                let from = wide_index_after(s, 0, w.start as usize, 0).min(n);
                let to = wide_index_after(s, w.start as usize, end, from).min(n);

                let from_ext = (GetTabbedTextExtentW(dc, line_text_w.as_ptr(), from as i32, 1, &tab_w) & 0xFFFF) as i32;
                let to_ext = (GetTabbedTextExtentW(dc, line_text_w.as_ptr(), to as i32, 1, &tab_w) & 0xFFFF) as i32;
                let mark = RECT {
                    left: x + from_ext,
                    top: clip.top,
                    right: x + to_ext,
                    bottom: clip.bottom,
                };
                fill(dc, &mark, word_bg);
            }
            cur += 1;
        }

        let mut byte = 0;
        let mut wide = 0;
        let mut cx = x;
        let count = tokens_vec.len();

        for t in 0..=count {
            if cx >= clip.right { break; }
            let starts = [
                if t < count { tokens_vec[t].start } else { s.len() },
                if t < count { tokens_vec[t].end } else { s.len() },
            ];

            for part in 0..2 {
                let to = wide_index_after(s, byte, starts[part], wide).min(n);
                if to > wide {
                    let color = if part == 0 {
                        APP.theme.fg
                    } else {
                        APP.theme.syntax[tokens_vec[t].kind as usize]
                    };
                    SetTextColor(dc, color);
                    let printed = TabbedTextOutW(
                        dc,
                        cx,
                        y,
                        line_text_w.as_ptr().add(wide),
                        (to - wide) as i32,
                        1,
                        &tab_w,
                        x,
                    );
                    cx += (printed & 0xFFFF) as i32;
                }
                byte = starts[part];
                wide = to;
            }
        }

        RestoreDC(dc, saved);
    }
}

fn draw_number(dc: HDC, no: u32, right: i32, y: i32) {
    unsafe {
        let num_str = format!("{}", no);
        let num_w = to_wide(&num_str);
        let n = num_w.len() - 1;
        SetTextColor(dc, APP.theme.dim);
        ExtTextOutW(dc, right - (n as i32) * APP.char_w, y, 0, null(), num_w.as_ptr(), n as u32, null());
    }
}

fn paint_unified_row(dc: HDC, d: &DiffResult, index: usize, r: &RECT, y: i32, old_no: u32, new_no: u32) {
    unsafe {
        let t = APP.theme;
        let line = &d.lines[index];
        let (old_right, new_right, marker_x, text_x) = unified_gutter();
        let bg_color = match line.kind {
            LINE_ADDED => t.added_bg,
            LINE_REMOVED => t.removed_bg,
            LINE_GAP => t.panel,
            _ => t.bg,
        };
        fill(dc, r, bg_color);

        if line.kind == LINE_GAP {
            SetTextColor(dc, t.dim);
            let dots = [0x22EFu16, 0];
            ExtTextOutW(dc, marker_x, y, 0, null(), dots.as_ptr(), 1, null());
            return;
        }

        if line.kind != LINE_ADDED {
            draw_number(dc, old_no, old_right, y);
        }
        if line.kind != LINE_REMOVED {
            draw_number(dc, new_no, new_right, y);
        }
        if line.kind != LINE_CONTEXT {
            SetTextColor(dc, if line.kind == LINE_ADDED { t.added } else { t.deleted });
            let mark = if line.kind == LINE_ADDED { [b'+' as u16, 0] } else { [b'-' as u16, 0] };
            ExtTextOutW(dc, marker_x, y, 0, null(), mark.as_ptr(), 1, null());
        }

        let clip = RECT { left: text_x, top: r.top, right: r.right, bottom: r.bottom };
        draw_code(dc, d, index, line.kind != LINE_REMOVED, text_x - APP.diff_left, y, &clip);
    }
}

struct NumberCursor {
    index: i32,
    old_no: u32,
    new_no: u32,
}

fn number_at(d: &DiffResult, c: &mut NumberCursor, index: usize, new_side: bool) -> u32 {
    if c.index < 0 || (index as i32) < c.index {
        diff_line_numbers(d, index, &mut c.old_no, &mut c.new_no);
        c.index = index as i32;
    }
    while (c.index as usize) < index {
        diff_advance_numbers(&d.lines[c.index as usize], &mut c.old_no, &mut c.new_no);
        c.index += 1;
    }
    if new_side { c.new_no } else { c.old_no }
}

fn paint_side_cell(dc: HDC, d: &DiffResult, index: i32, new_side: bool, x0: i32, x1: i32, top: i32, y: i32, cursor: &mut NumberCursor) {
    unsafe {
        let t = APP.theme;
        let cell = RECT { left: x0, top, right: x1, bottom: top + APP.code_row_h };
        if index < 0 {
            fill(dc, &cell, t.panel);
            return;
        }
        let line = &d.lines[index as usize];
        let bg_color = match line.kind {
            LINE_ADDED => t.added_bg,
            LINE_REMOVED => t.removed_bg,
            LINE_GAP => t.panel,
            _ => t.bg,
        };
        fill(dc, &cell, bg_color);
        let num_right = x0 + scale(8) + line_number_width();

        if line.kind == LINE_GAP {
            SetTextColor(dc, t.dim);
            let dots = [0x22EFu16, 0];
            ExtTextOutW(dc, num_right + scale(10), y, 0, null(), dots.as_ptr(), 1, null());
            return;
        }

        draw_number(dc, number_at(d, cursor, index as usize, new_side), num_right, y);
        let text_x = x0 + side_text_offset();
        let clip = RECT { left: text_x, top: cell.top, right: x1, bottom: cell.bottom };
        draw_code(dc, d, index as usize, new_side, text_x - APP.diff_left, y, &clip);
    }
}

fn diff_paint(hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut c);
        let t = APP.theme;
        let rows = diff_rows();
        let first = (APP.diff_top + ps.rcPaint.top / APP.code_row_h).max(0) as usize;
        let last = (APP.diff_top + (ps.rcPaint.bottom + APP.code_row_h - 1) / APP.code_row_h).max(0) as usize;
        let text_dy = (APP.code_row_h - APP.code_h) / 2;
        let half = (c.right - 1) / 2;

        let mut old_no = 0u32;
        let mut new_no = 0u32;
        if let Some(ref d) = APP.diff {
            if !APP.side_by_side && first < rows {
                diff_line_numbers(d, first, &mut old_no, &mut new_no);
            }
        }

        let mut left_numbers = NumberCursor { index: -1, old_no: 0, new_no: 0 };
        let mut right_numbers = NumberCursor { index: -1, old_no: 0, new_no: 0 };
        SelectObject(dc, APP.code_font);
        SetBkMode(dc, TRANSPARENT as i32);

        for row in first..last {
            let r = RECT {
                left: 0,
                top: (row as i32 - APP.diff_top) * APP.code_row_h,
                right: c.right,
                bottom: (row as i32 - APP.diff_top + 1) * APP.code_row_h,
            };
            let y = r.top + text_dy;
            if row >= rows || APP.diff.is_none() {
                fill(dc, &r, t.bg);
            } else if let Some(ref d) = APP.diff {
                if APP.side_by_side {
                    let side = &d.side[row];
                    paint_side_cell(dc, d, side.left, false, 0, half, r.top, y, &mut left_numbers);
                    let separator = RECT { left: half, top: r.top, right: half + 1, bottom: r.bottom };
                    fill(dc, &separator, t.border);
                    paint_side_cell(dc, d, side.right, true, half + 1, c.right, r.top, y, &mut right_numbers);
                } else {
                    paint_unified_row(dc, d, row, &r, y, old_no, new_no);
                    diff_advance_numbers(&d.lines[row], &mut old_no, &mut new_no);
                }
            }
        }

        let mut message: Option<String> = None;
        if let Some(ref d) = APP.diff {
            if !d.error.is_empty() {
                message = Some(from_wide(&d.error));
            } else if d.binary {
                message = Some("Binary file: no text diff.".to_string());
            } else if d.lines.is_empty() {
                message = Some(if d.ignore_eol { "Only line endings changed.".to_string() } else { "No content changes.".to_string() });
            }
        }

        if let Some(msg_str) = message {
            let mut r = RECT { left: scale(10), top: scale(8), right: c.right - scale(10), bottom: c.bottom };
            SelectObject(dc, APP.ui_font);
            SetTextColor(dc, t.dim);
            let msg_w = to_wide(&msg_str);
            DrawTextW(dc, msg_w.as_ptr(), -1, &mut r, (DT_WORDBREAK | DT_NOPREFIX) as u32);
        }

        EndPaint(hwnd, &ps);
    }
}

fn other_view_row(row: i32) -> i32 {
    unsafe {
        if let Some(ref d) = APP.diff {
            if APP.side_by_side {
                if (row as usize) < d.side.len() {
                    let s = &d.side[row as usize];
                    return if s.left >= 0 { s.left } else { s.right };
                }
            } else {
                for i in 0..d.side.len().saturating_sub(1) {
                    let s = &d.side[i];
                    if s.left.max(s.right) >= row {
                        return i as i32;
                    }
                }
                return d.side.len().saturating_sub(1) as i32;
            }
        }
        0
    }
}

fn toggle_view() {
    unsafe {
        let mut top = 0;
        if diff_rows() > 0 {
            top = other_view_row(APP.diff_top);
            if APP.change_row >= 0 {
                APP.change_row = other_view_row(APP.change_row);
            }
        }
        APP.side_by_side = !APP.side_by_side;
        diff_scroll_to(top, APP.diff_left);
        InvalidateRect(APP.main, &APP.toggle, 0);
    }
}

fn row_changed(i: usize) -> bool {
    unsafe {
        if let Some(ref d) = APP.diff {
            if APP.side_by_side {
                if i < d.side.len() {
                    return d.side[i].left != d.side[i].right;
                }
            } else if i < d.lines.len() {
                return d.lines[i].kind == LINE_REMOVED || d.lines[i].kind == LINE_ADDED;
            }
        }
        false
    }
}

fn find_change(from: i32, dir: i32) -> i32 {
    let rows = diff_rows() as i32;
    let mut i = from + dir;
    while i >= 0 && i < rows {
        if row_changed(i as usize) && (i == 0 || !row_changed((i - 1) as usize)) {
            return i;
        }
        i += dir;
    }
    -1
}

fn show_change(row: i32) {
    unsafe {
        APP.change_row = row;
        diff_scroll_to(row - CHANGE_CONTEXT, APP.diff_left);
    }
}

fn list_select(row: i32, dir: i32) {
    unsafe {
        let count = row_count() as i32;
        if count == 0 { return; }
        let mut r = row.clamp(0, count - 1);

        let initial_r = r;
        while r >= 0 && r < count && is_header(r as usize) {
            r += dir;
        }
        if r < 0 || r >= count {
            r = initial_r;
            while r >= 0 && r < count && is_header(r as usize) {
                r -= dir;
            }
        }
        if r < 0 || r >= count { return; }

        let visible = list_visible_rows();
        let show = if r > 0 && is_header((r - 1) as usize) { r - 1 } else { r };
        if show < APP.top {
            list_scroll_to(show);
        } else if r >= APP.top + visible {
            list_scroll_to(r - visible + 1);
        }
        InvalidateRect(APP.list, null(), 0);

        if r == APP.sel { return; }
        if APP.sel >= 0 {
            APP.language_pick = LANG_AUTO;
        }
        APP.sel = r;
        APP.diff_generation += 1;
        if let Some(ref s) = APP.status {
            git_request_diff(s, &s.rows[r as usize], APP.diff_generation, APP.language_pick, APP.ignore_eol);
        }
    }
}

fn goto_change(dir: i32) {
    unsafe {
        if let Some(ref d) = APP.diff {
            if d.generation == APP.diff_generation && diff_rows() > 0 {
                let mut from = APP.change_row;
                if from < APP.diff_top || from >= APP.diff_top + diff_visible_rows() {
                    from = if dir > 0 { APP.diff_top - 1 } else { APP.diff_top };
                }
                let row = find_change(from, dir);
                if row >= 0 {
                    show_change(row);
                    return;
                }
            }
        }
        if APP.status.is_none() || APP.sel < 0 {
            return;
        }
        let old_sel = APP.sel;
        APP.change_generation = APP.diff_generation + 1;
        APP.change_dir = dir;
        list_select(APP.sel + dir, dir);
        if APP.sel == old_sel {
            APP.change_generation = 0;
        }
    }
}

fn move_focus(from: HWND, backwards: bool) {
    unsafe {
        let order = [APP.edits[EDIT_PATH], APP.edits[EDIT_BASE], APP.edits[EDIT_TARGET], APP.list, APP.diff_wnd];
        let n = order.len();
        let mut i = 0;
        while i < n && order[i] != from { i += 1; }
        let next_idx = if backwards { (i + n - 1) % n } else { (i + 1) % n };
        SetFocus(order[next_idx]);
    }
}

unsafe extern "system" fn diff_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            diff_paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            if !APP.diff_wnd.is_null() {
                diff_scroll_to(APP.diff_top, APP.diff_left);
            }
            0
        }
        WM_VSCROLL => {
            diff_scroll_to(scroll_target(hwnd, SB_VERT as i32, wp, APP.diff_top, diff_visible_rows(), 1), APP.diff_left);
            0
        }
        WM_HSCROLL => {
            let mut c: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut c);
            diff_scroll_to(APP.diff_top, scroll_target(hwnd, SB_HORZ as i32, wp, APP.diff_left, c.right / 2, 4 * APP.char_w));
            0
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let delta = ((wp >> 16) as i16) as i32;
            APP.diff_wheel_rest += delta;
            let steps = APP.diff_wheel_rest / 40;
            APP.diff_wheel_rest -= steps * 40;
            if msg == WM_MOUSEHWHEEL {
                diff_scroll_to(APP.diff_top, APP.diff_left + steps * 2 * APP.char_w);
            } else if (wp & (MK_SHIFT as usize)) != 0 {
                diff_scroll_to(APP.diff_top, APP.diff_left - steps * 2 * APP.char_w);
            } else {
                diff_scroll_to(APP.diff_top - steps, APP.diff_left);
            }
            0
        }
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            0
        }
        WM_KEYDOWN => {
            let visible = diff_visible_rows();
            match (wp & 0xFFFF) as u16 {
                VK_UP => diff_scroll_to(APP.diff_top - 1, APP.diff_left),
                VK_DOWN => diff_scroll_to(APP.diff_top + 1, APP.diff_left),
                VK_PRIOR => diff_scroll_to(APP.diff_top - visible, APP.diff_left),
                VK_NEXT => diff_scroll_to(APP.diff_top + visible, APP.diff_left),
                VK_HOME => diff_scroll_to(0, 0),
                VK_END => diff_scroll_to(SCROLL_END, APP.diff_left),
                VK_LEFT => diff_scroll_to(APP.diff_top, APP.diff_left - 4 * APP.char_w),
                VK_RIGHT => diff_scroll_to(APP.diff_top, APP.diff_left + 4 * APP.char_w),
                VK_TAB => move_focus(hwnd, (GetKeyState(VK_SHIFT as i32) as i16) < 0),
                0x55 => toggle_view(),
                0x4E => goto_change(1),
                0x50 => goto_change(-1),
                VK_F7 => goto_change(if (GetKeyState(VK_SHIFT as i32) as i16) < 0 { -1 } else { 1 }),
                _ => {}
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn is_header(row: usize) -> bool {
    unsafe {
        if let Some(ref s) = APP.status {
            if row < s.rows.len() {
                let t = s.rows[row].row_type;
                return t == RowType::StagedHeader || t == RowType::UnstagedHeader;
            }
        }
        false
    }
}

fn row_badge(row: &ListRow) -> char {
    unsafe {
        if let Some(ref s) = APP.status {
            let f = &s.files[row.file_index];
            let mut c = f.x as char;
            if row.row_type == RowType::Unstaged {
                if f.kind == b'?' { return 'U'; }
                if f.kind == b'u' { return '!'; }
                c = f.y as char;
            }
            if c == 'C' { return 'A'; }
            if c == 'T' { return 'M'; }
            return c;
        }
        'M'
    }
}

fn badge_color(b: char) -> COLORREF {
    unsafe {
        match b {
            'A' => APP.theme.added,
            'D' => APP.theme.deleted,
            'U' => APP.theme.untracked,
            'R' => APP.theme.renamed,
            '!' => APP.theme.conflict,
            _ => APP.theme.modified,
        }
    }
}

fn list_visible_rows() -> i32 {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.list, &mut c);
        (c.bottom / APP.row_h).max(1)
    }
}

fn list_scroll_to(mut top: i32) {
    unsafe {
        let count = row_count() as i32;
        let visible = list_visible_rows();
        let max_top = count - visible;
        if top > max_top { top = max_top; }
        if top < 0 { top = 0; }
        APP.top = top;

        let mut si: SCROLLINFO = std::mem::zeroed();
        si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
        si.fMask = SIF_RANGE | SIF_PAGE | SIF_POS;
        si.nMin = 0;
        si.nMax = (count - 1).max(0);
        si.nPage = visible as u32;
        si.nPos = top;
        SetScrollInfo(APP.list, SB_VERT as i32, &si, 1);
        InvalidateRect(APP.list, null(), 0);
    }
}

fn list_paint(hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut c);
        let t = APP.theme;
        let count = row_count();
        let first = (APP.top + ps.rcPaint.top / APP.row_h).max(0) as usize;
        let last = (APP.top + (ps.rcPaint.bottom + APP.row_h - 1) / APP.row_h).max(0) as usize;
        let text_dy = (APP.row_h - APP.text_h) / 2;
        SetBkMode(dc, TRANSPARENT as i32);

        for row in first..last {
            let r = RECT {
                left: 0,
                top: (row as i32 - APP.top) * APP.row_h,
                right: c.right,
                bottom: (row as i32 - APP.top + 1) * APP.row_h,
            };
            let y = r.top + text_dy;
            if row >= count || APP.status.is_none() {
                fill(dc, &r, t.panel);
                continue;
            }
            let s = APP.status.as_ref().unwrap();
            let list_row = &s.rows[row];
            fill(dc, &r, if row as i32 == APP.sel { t.sel } else { t.panel });

            if is_header(row) {
                let label_str = if list_row.row_type == RowType::StagedHeader { "STAGED" } else { "UNSTAGED" };
                let label_w = to_wide(label_str);
                SelectObject(dc, APP.bold_font);
                SetTextColor(dc, t.dim);
                ExtTextOutW(dc, scale(10), y, ETO_CLIPPED as u32, &r, label_w.as_ptr(), (label_w.len() - 1) as u32, null());
                continue;
            }

            let f = &s.files[list_row.file_index];
            let b = row_badge(list_row);
            let badge_w = [b as u16, 0];
            SelectObject(dc, APP.bold_font);
            SetTextColor(dc, badge_color(b));
            ExtTextOutW(dc, scale(10), y, ETO_CLIPPED as u32, &r, badge_w.as_ptr(), 1, null());

            let slash = f.path.rfind('/');
            let name = match slash {
                Some(idx) => &f.path[idx + 1..],
                None => &f.path,
            };
            let name_w = to_wide(name);
            let mut x = scale(30);
            SelectObject(dc, APP.ui_font);
            SetTextColor(dc, t.fg);
            ExtTextOutW(dc, x, y, ETO_CLIPPED as u32, &r, name_w.as_ptr(), (name_w.len() - 1) as u32, null());

            if let Some(slash_idx) = slash {
                let mut size: SIZE = std::mem::zeroed();
                GetTextExtentPoint32W(dc, name_w.as_ptr(), (name_w.len() - 1) as i32, &mut size);
                x += size.cx + scale(8);
                let dir_str = &f.path[..slash_idx];
                let dir_w = to_wide(dir_str);
                SetTextColor(dc, t.dim);
                ExtTextOutW(dc, x, y, ETO_CLIPPED as u32, &r, dir_w.as_ptr(), (dir_w.len() - 1) as u32, null());
            }
        }

        let mut message: Option<String> = None;
        if count == 0 && !APP.loading {
            if let Some(ref s) = APP.status {
                if !s.error.is_empty() {
                    message = Some(from_wide(&s.error));
                }
            }
            if message.is_none() {
                message = Some("No changes.".to_string());
            }
        }

        if let Some(msg_str) = message {
            let mut r = RECT { left: scale(10), top: scale(8), right: c.right - scale(10), bottom: c.bottom };
            SelectObject(dc, APP.ui_font);
            SetTextColor(dc, t.dim);
            let msg_w = to_wide(&msg_str);
            DrawTextW(dc, msg_w.as_ptr(), -1, &mut r, (DT_WORDBREAK | DT_NOPREFIX) as u32);
        }

        EndPaint(hwnd, &ps);

        if APP.startup_ms == 0 {
            APP.startup_ms = ms_since_process_start();
        }
    }
}

unsafe extern "system" fn list_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            list_paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            if !APP.list.is_null() {
                list_scroll_to(APP.top);
            }
            0
        }
        WM_VSCROLL => {
            list_scroll_to(scroll_target(hwnd, SB_VERT as i32, wp, APP.top, list_visible_rows(), 1));
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) as i16) as i32;
            APP.wheel_rest += delta;
            let rows = APP.wheel_rest / 40;
            APP.wheel_rest -= rows * 40;
            list_scroll_to(APP.top - rows);
            0
        }
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            let y = ((lp >> 16) as i16) as i32;
            let row = APP.top + y / APP.row_h;
            if row < row_count() as i32 {
                list_select(row, 1);
            }
            0
        }
        WM_KEYDOWN => {
            let visible = list_visible_rows();
            match (wp & 0xFFFF) as u16 {
                VK_UP => list_select(APP.sel - 1, -1),
                VK_DOWN => list_select(APP.sel + 1, 1),
                VK_PRIOR => list_select(APP.sel - visible, -1),
                VK_NEXT => list_select(APP.sel + visible, 1),
                VK_HOME => list_select(0, 1),
                VK_END => list_select(row_count() as i32 - 1, -1),
                VK_TAB => move_focus(hwnd, (GetKeyState(VK_SHIFT as i32) as i16) < 0),
                0x55 => toggle_view(),
                0x4E => goto_change(1),
                0x50 => goto_change(-1),
                VK_F7 => goto_change(if (GetKeyState(VK_SHIFT as i32) as i16) < 0 { -1 } else { 1 }),
                _ => {}
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn read_edit(which: usize) -> String {
    unsafe {
        let mut buf = [0u16; 1024];
        let len = GetWindowTextW(APP.edits[which], buf.as_mut_ptr(), buf.len() as i32);
        let s = from_wide(&buf[..len as usize]);
        s.trim().to_string()
    }
}

fn reload() {
    unsafe {
        let typed = read_edit(EDIT_PATH);
        let base = read_edit(EDIT_BASE);
        let target = read_edit(EDIT_TARGET);

        let mut buf = [0u16; 1024];
        let typed_w = to_wide(&typed);
        let n = windows_sys::Win32::Storage::FileSystem::GetFullPathNameW(typed_w.as_ptr(), buf.len() as u32, buf.as_mut_ptr(), null_mut());
        if n == 0 { return; }
        APP.folder = from_wide(&buf[..n as usize]);
        APP.loading = true;
        git_request_status(&APP.folder, &base, &target);
        InvalidateRect(APP.main, null(), 0);
    }
}

const REF_GROUPS: [u8; 5] = [b'H', b'b', b'r', b't', b'c'];
const REF_GROUP_LABELS: [Option<&str>; 5] = [None, Some("BRANCHES"), Some("REMOTE BRANCHES"), Some("TAGS"), Some("RECENT COMMITS")];

fn refs_count() -> usize {
    unsafe { APP.refs.as_ref().map(|r| r.shown.len()).unwrap_or(0) }
}

fn refs_visible_rows() -> i32 {
    unsafe {
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.refs_wnd, &mut c);
        (c.bottom / APP.row_h).max(1)
    }
}

fn refs_filter() {
    unsafe {
        APP.refs_sel = -1;
        if APP.refs.is_none() { return; }
        let r = APP.refs.as_mut().unwrap();

        let typed = if APP.refs_edit >= 0 { read_edit(APP.refs_edit as usize) } else { String::new() };
        let needle = if APP.refs_typed { typed.to_ascii_lowercase() } else { String::new() };

        let mut exact: i32 = -1;
        let mut first: i32 = -1;
        r.shown.clear();

        for (g_idx, &g_kind) in REF_GROUPS.iter().enumerate() {
            let mut header = false;
            for (idx, e) in r.refs.iter().enumerate() {
                if e.kind != g_kind { continue; }
                let matches = needle.is_empty()
                    || e.name.to_ascii_lowercase().contains(&needle)
                    || e.detail.as_deref().map(|d| d.to_ascii_lowercase().contains(&needle)).unwrap_or(false);
                if !matches { continue; }

                if !header && g_idx > 0 {
                    r.shown.push(-1 - (g_idx as i32));
                }
                header = true;

                if first < 0 {
                    first = r.shown.len() as i32;
                }
                if exact < 0 && !typed.is_empty() && e.name == typed {
                    exact = r.shown.len() as i32;
                }
                r.shown.push(idx as i32);
            }
        }

        APP.refs_sel = if exact >= 0 { exact } else if !needle.is_empty() { first } else { -1 };
    }
}

fn refs_scroll_to(mut top: i32) {
    unsafe {
        let max_top = refs_count() as i32 - refs_visible_rows();
        if top > max_top { top = max_top; }
        if top < 0 { top = 0; }
        APP.refs_top = top;

        let mut si: SCROLLINFO = std::mem::zeroed();
        si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
        si.fMask = SIF_RANGE | SIF_PAGE | SIF_POS;
        si.nMin = 0;
        si.nMax = (refs_count() as i32 - 1).max(0);
        si.nPage = refs_visible_rows() as u32;
        si.nPos = top;
        SetScrollInfo(APP.refs_wnd, SB_VERT as i32, &si, 1);
        InvalidateRect(APP.refs_wnd, null(), 0);
    }
}

fn refs_reveal(i: i32) {
    unsafe {
        if i < 0 || APP.refs.is_none() { return; }
        let r = APP.refs.as_ref().unwrap();
        let show = if i > 0 && (i as usize - 1) < r.shown.len() && r.shown[i as usize - 1] < 0 {
            i - 1
        } else {
            i
        };
        if show < APP.refs_top {
            refs_scroll_to(show);
        } else if i >= APP.refs_top + refs_visible_rows() {
            refs_scroll_to(i - refs_visible_rows() + 1);
        }
        InvalidateRect(APP.refs_wnd, null(), 0);
    }
}

fn refs_place() {
    unsafe {
        if APP.refs_edit < 0 { return; }
        let f = APP.frames[APP.refs_edit as usize];
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(APP.main, &mut c);
        let rows = (refs_count() as i32).max(1).min(REFS_ROWS);
        let w = scale(REFS_W);
        let mut x = f.left;
        if x + w > c.right - scale(PAD) {
            x = c.right - scale(PAD) - w;
        }
        let mut p = POINT { x: x.max(0), y: f.bottom + scale(2) };
        ClientToScreen(APP.main, &mut p);
        let mut outer = RECT { left: 0, top: 0, right: w, bottom: rows * APP.row_h };
        AdjustWindowRectExForDpi(&mut outer, (WS_POPUP | WS_BORDER) as u32, 0, (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) as u32, APP.dpi);
        SetWindowPos(APP.refs_wnd, null_mut(), p.x, p.y, w, outer.bottom - outer.top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        refs_scroll_to(APP.refs_top);
        refs_reveal(APP.refs_sel);
    }
}

fn refs_hide() {
    unsafe {
        if APP.refs_edit < 0 { return; }
        APP.refs_edit = -1;
        ShowWindow(APP.refs_wnd, SW_HIDE);
    }
}

fn refs_open(which: usize) {
    unsafe {
        if APP.refs_wnd.is_null() {
            let class_name = to_wide("DiffViewerRefs");
            APP.refs_wnd = CreateWindowExW(
                (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) as u32,
                class_name.as_ptr(),
                null(),
                (WS_POPUP | WS_BORDER | WS_VSCROLL) as u32,
                0, 0, 0, 0,
                APP.main,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            SetWindowTheme(APP.refs_wnd, if APP.theme as *const _ == &DARK_THEME as *const _ { to_wide("DarkMode_Explorer").as_ptr() } else { to_wide("Explorer").as_ptr() }, null());
        }

        let root = APP.status.as_ref().map(|s| from_wide(&s.root)).unwrap_or_default();
        if let Some(ref r) = APP.refs {
            if from_wide(&r.root) != root {
                APP.refs = None;
            }
        }
        if !root.is_empty() {
            APP.refs_generation += 1;
            git_request_refs(&root, APP.refs_generation);
        }
        APP.refs_edit = which as i32;
        APP.refs_typed = false;
        APP.refs_top = 0;
        refs_filter();
        refs_place();
    }
}

fn refs_move(delta: i32) {
    unsafe {
        let count = refs_count() as i32;
        if count == 0 || APP.refs.is_none() { return; }
        let dir = if delta > 0 { 1 } else { -1 };
        let mut i = (if APP.refs_sel >= 0 { APP.refs_sel } else if dir > 0 { -1 } else { count }) + delta;
        i = i.clamp(0, count - 1);
        let from = i;
        let r = APP.refs.as_ref().unwrap();
        while i >= 0 && i < count && r.shown[i as usize] < 0 {
            i += dir;
        }
        if i < 0 || i >= count {
            i = from;
            while i >= 0 && i < count && r.shown[i as usize] < 0 {
                i -= dir;
            }
        }
        if i < 0 || i >= count { return; }
        APP.refs_sel = i;
        refs_reveal(i);
    }
}

fn refs_pick() -> bool {
    unsafe {
        if APP.refs_sel < 0 || APP.refs_edit < 0 || APP.refs.is_none() { return false; }
        let r = APP.refs.as_ref().unwrap();
        let idx = r.shown[APP.refs_sel as usize];
        if idx < 0 { return false; }
        let name = r.refs[idx as usize].name.clone();
        let edit = APP.edits[APP.refs_edit as usize];
        refs_hide();
        APP.refs_setting_text = true;
        let name_w = to_wide(&name);
        SetWindowTextW(edit, name_w.as_ptr());
        APP.refs_setting_text = false;
        SendMessageW(edit, EM_SETSEL, (name_w.len() - 1) as usize, (name_w.len() - 1) as isize);
        reload();
        true
    }
}

fn refs_paint(hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut c);
        let t = APP.theme;
        let count = refs_count();
        let dy = (APP.row_h - APP.text_h) / 2;
        SetBkMode(dc, TRANSPARENT as i32);

        let mut row = APP.refs_top as usize;
        let mut y = 0;
        while y < c.bottom {
            let box_rect = RECT { left: 0, top: y, right: c.right, bottom: y + APP.row_h };
            fill(dc, &box_rect, if row as i32 == APP.refs_sel { t.sel } else { t.panel });

            if row < count && APP.refs.is_some() {
                let r = APP.refs.as_ref().unwrap();
                let s = r.shown[row];
                if s < 0 {
                    let g_idx = (-1 - s) as usize;
                    if let Some(label) = REF_GROUP_LABELS[g_idx] {
                        let label_w = to_wide(label);
                        SelectObject(dc, APP.bold_font);
                        SetTextColor(dc, t.dim);
                        ExtTextOutW(dc, scale(10), y + dy, ETO_CLIPPED as u32, &box_rect, label_w.as_ptr(), (label_w.len() - 1) as u32, null());
                    }
                } else {
                    let e = &r.refs[s as usize];
                    let name_w = to_wide(&e.name);
                    SelectObject(dc, APP.ui_font);
                    SetTextColor(dc, t.fg);
                    ExtTextOutW(dc, scale(18), y + dy, ETO_CLIPPED as u32, &box_rect, name_w.as_ptr(), (name_w.len() - 1) as u32, null());
                    if let Some(ref detail) = e.detail {
                        let mut size: SIZE = std::mem::zeroed();
                        GetTextExtentPoint32W(dc, name_w.as_ptr(), (name_w.len() - 1) as i32, &mut size);
                        let detail_w = to_wide(detail);
                        SetTextColor(dc, t.dim);
                        ExtTextOutW(dc, scale(18) + size.cx + scale(10), y + dy, ETO_CLIPPED as u32, &box_rect, detail_w.as_ptr(), (detail_w.len() - 1) as u32, null());
                    }
                }
            }

            row += 1;
            y += APP.row_h;
        }

        let mut message: Option<String> = None;
        if count == 0 {
            if APP.status.is_none() || APP.status.as_ref().unwrap().root.is_empty() {
                message = Some("Open a git repo first.".to_string());
            } else if APP.refs.is_none() {
                message = Some("Loading\u{2026}".to_string());
            } else if let Some(ref r) = APP.refs {
                if !r.error.is_empty() {
                    message = Some(from_wide(&r.error));
                } else {
                    message = Some("No matching refs.".to_string());
                }
            }
        }

        if let Some(msg_str) = message {
            let mut box_rect = RECT { left: scale(10), top: dy, right: c.right - scale(10), bottom: c.bottom };
            SelectObject(dc, APP.ui_font);
            SetTextColor(dc, t.dim);
            let msg_w = to_wide(&msg_str);
            DrawTextW(dc, msg_w.as_ptr(), -1, &mut box_rect, (DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX) as u32);
        }

        EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn refs_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            refs_paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_VSCROLL => {
            refs_scroll_to(scroll_target(hwnd, SB_VERT as i32, wp, APP.refs_top, refs_visible_rows(), 1));
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) as i16) as i32;
            refs_scroll_to(APP.refs_top - delta / 40);
            0
        }
        WM_MOUSEMOVE | WM_LBUTTONDOWN => {
            let y = ((lp >> 16) as i16) as i32;
            let row = APP.refs_top + y / APP.row_h;
            if let Some(ref r) = APP.refs {
                if (row as usize) < r.shown.len() && r.shown[row as usize] >= 0 && row != APP.refs_sel {
                    APP.refs_sel = row;
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            if msg == WM_LBUTTONDOWN && row == APP.refs_sel {
                refs_pick();
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ref_box = if hwnd == APP.edits[EDIT_BASE] {
        EDIT_BASE as i32
    } else if hwnd == APP.edits[EDIT_TARGET] {
        EDIT_TARGET as i32
    } else {
        -1
    };
    let open = ref_box >= 0 && APP.refs_edit == ref_box;

    match msg {
        WM_KEYDOWN => {
            if open {
                match (wp & 0xFFFF) as u16 {
                    VK_DOWN => { refs_move(1); return 0; }
                    VK_UP => { refs_move(-1); return 0; }
                    VK_NEXT => { refs_move(refs_visible_rows()); return 0; }
                    VK_PRIOR => { refs_move(-refs_visible_rows()); return 0; }
                    VK_ESCAPE => { refs_hide(); return 0; }
                    VK_RETURN => {
                        if refs_pick() { return 0; }
                        refs_hide();
                    }
                    _ => {}
                }
            } else if ref_box >= 0 && (wp & 0xFFFF) as u16 == VK_DOWN {
                refs_open(ref_box as usize);
                return 0;
            }
            if (wp & 0xFFFF) as u16 == VK_RETURN {
                reload();
                return 0;
            }
            if (wp & 0xFFFF) as u16 == VK_TAB {
                move_focus(hwnd, (GetKeyState(VK_SHIFT as i32) as i16) < 0);
                return 0;
            }
        }
        WM_SYSKEYDOWN => {
            if ref_box >= 0 && (wp & 0xFFFF) as u16 == VK_DOWN {
                if open { refs_hide(); } else { refs_open(ref_box as usize); }
                return 0;
            }
        }
        WM_CHAR => {
            if wp == 13 || wp == 9 || wp == 27 {
                return 0;
            }
        }
        WM_LBUTTONDOWN => {
            if ref_box >= 0 && !open {
                let orig = EDIT_PROC_ORIGINAL.unwrap();
                let res = orig(hwnd, msg, wp, lp);
                refs_open(ref_box as usize);
                return res;
            }
        }
        WM_MOUSEWHEEL => {
            if open {
                return SendMessageW(APP.refs_wnd, msg, wp, lp);
            }
        }
        WM_KILLFOCUS => {
            refs_hide();
            InvalidateRect(hwnd, null(), 1);
        }
        WM_SETFOCUS => {
            InvalidateRect(hwnd, null(), 1);
        }
        WM_PAINT => {
            let orig = EDIT_PROC_ORIGINAL.unwrap();
            let res = orig(hwnd, msg, wp, lp);
            let hint = if hwnd == APP.edits[EDIT_BASE] {
                Some("base ref")
            } else if hwnd == APP.edits[EDIT_TARGET] {
                Some("target ref")
            } else {
                None
            };
            if let Some(hint_str) = hint {
                if GetWindowTextLengthW(hwnd) == 0 && GetFocus() != hwnd {
                    let dc = GetDC(hwnd);
                    SelectObject(dc, APP.ui_font);
                    SetBkMode(dc, TRANSPARENT as i32);
                    SetTextColor(dc, APP.theme.dim);
                    let margins = SendMessageW(hwnd, EM_GETMARGINS, 0, 0) as i32;
                    let hint_w = to_wide(hint_str);
                    TextOutW(dc, margins & 0xFFFF, 0, hint_w.as_ptr(), (hint_w.len() - 1) as i32);
                    ReleaseDC(hwnd, dc);
                }
            }
            return res;
        }
        _ => {}
    }

    let orig = EDIT_PROC_ORIGINAL.unwrap();
    orig(hwnd, msg, wp, lp)
}

fn paint_pair(dc: HDC, box_rect: &RECT, left: &str, right: &str, selected: i32) {
    unsafe {
        let t = APP.theme;
        let labels = [left, right];
        let mid = (box_rect.left + box_rect.right) / 2;
        SelectObject(dc, APP.ui_font);

        for i in 0..2 {
            let mut seg = RECT {
                left: if i == 1 { mid } else { box_rect.left },
                top: box_rect.top,
                right: if i == 1 { box_rect.right } else { mid },
                bottom: box_rect.bottom,
            };
            fill(dc, &seg, if selected == i { t.sel } else { t.edit_bg });
            SetTextColor(dc, if selected == i || selected < 0 { t.fg } else { t.dim });
            let label_w = to_wide(labels[i as usize]);
            DrawTextW(dc, label_w.as_ptr(), -1, &mut seg, (DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX) as u32);
        }

        let divider = RECT { left: mid, top: box_rect.top, right: mid + 1, bottom: box_rect.bottom };
        fill(dc, &divider, t.border);
        FrameRect(dc, box_rect, APP.border_brush);
    }
}

fn reload_diff() {
    unsafe {
        APP.keep_scroll_generation = APP.diff_generation + 1;
        APP.diff_generation += 1;
        if let Some(ref s) = APP.status {
            if (APP.sel as usize) < s.rows.len() {
                git_request_diff(s, &s.rows[APP.sel as usize], APP.diff_generation, APP.language_pick, APP.ignore_eol);
            }
        }
    }
}

fn language_field_shown() -> bool {
    unsafe {
        !APP.loading && APP.diff.as_ref().map(|d| d.error.is_empty() && !d.binary).unwrap_or(false)
    }
}

fn language_menu() {
    unsafe {
        let menu = CreatePopupMenu();
        for lang in LANG_AUTO..LANG_COUNT {
            let name_w = to_wide(LANGUAGE_NAMES[lang]);
            let flags = MF_STRING | (if lang == APP.language_pick { MF_CHECKED } else { 0 });
            AppendMenuW(menu, flags as u32, (lang + 1) as usize, name_w.as_ptr());
            if lang == LANG_PLAIN {
                AppendMenuW(menu, MF_SEPARATOR as u32, 0, null());
            }
        }
        let mut p = POINT { x: APP.language_field.right, y: APP.language_field.top };
        ClientToScreen(APP.main, &mut p);
        let id = TrackPopupMenu(menu, (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTALIGN | TPM_BOTTOMALIGN) as u32, p.x, p.y, 0, APP.main, null());
        DestroyMenu(menu);
        if id == 0 || (id as usize - 1) == APP.language_pick { return; }
        APP.language_pick = id as usize - 1;
        reload_diff();
    }
}

fn main_paint(hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut c: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut c);
        let t = APP.theme;
        let bar = scale(BAR_H);
        let status_h = scale(STATUS_H);

        let top = RECT { left: 0, top: 0, right: c.right, bottom: bar };
        fill(dc, &top, t.bg);

        for i in 0..EDIT_COUNT {
            FillRect(dc, &APP.frames[i], APP.edit_brush);
            FrameRect(dc, &APP.frames[i], APP.border_brush);
        }

        SetBkMode(dc, TRANSPARENT as i32);
        SelectObject(dc, APP.ui_font);
        SetTextColor(dc, t.dim);
        let mut arrow = RECT {
            left: APP.frames[EDIT_BASE].right,
            top: APP.frames[EDIT_BASE].top,
            right: APP.frames[EDIT_TARGET].left,
            bottom: APP.frames[EDIT_BASE].bottom,
        };
        let arrow_w = [0x2192u16, 0];
        DrawTextW(dc, arrow_w.as_ptr(), 1, &mut arrow, (DT_CENTER | DT_VCENTER | DT_SINGLELINE) as u32);

        paint_pair(dc, &APP.nav, "\u{25B2}  Prev", "\u{25BC}  Next", -1);
        paint_pair(dc, &APP.toggle, "Unified", "Side by side", if APP.side_by_side { 1 } else { 0 });

        let mut split = split_rect();
        fill(dc, &split, t.bg);
        split.right = split.left + 1;
        fill(dc, &split, t.border);

        let status = RECT { left: 0, top: c.bottom - status_h, right: c.right, bottom: c.bottom };
        fill(dc, &status, t.panel);

        let mut text = String::new();
        if APP.loading {
            text.push_str("Loading\u{2026}");
        } else if let Some(ref s) = APP.status {
            if !s.error.is_empty() {
                text.push_str(&from_wide(&s.error));
            } else if !s.base.is_empty() && s.base[0] != 0 {
                let base_str = from_wide(&s.base);
                let target_str = if !s.target.is_empty() && s.target[0] != 0 { from_wide(&s.target) } else { "working tree".to_string() };
                text.push_str(&format!(
                    "{} \u{2192} {}   \u{00b7}   {} file{} differ{}   \u{00b7}   {} ms",
                    base_str,
                    target_str,
                    s.files.len(),
                    if s.files.len() == 1 { "" } else { "s" },
                    if s.files.len() == 1 { "s" } else { "" },
                    s.elapsed_ms
                ));
            } else {
                text.push_str(&format!(
                    "{} changed file{}   \u{00b7}   git status {} ms",
                    s.files.len(),
                    if s.files.len() == 1 { "" } else { "s" },
                    s.elapsed_ms
                ));
            }
        }

        if !APP.loading {
            if let Some(ref d) = APP.diff {
                if d.error.is_empty() {
                    text.push_str(&format!("   \u{00b7}   +{} \u{2212}{}   \u{00b7}   diff {} ms", d.added, d.removed, d.elapsed_ms));
                }
            }
        }

        let mem = memory_counters();
        let priv_bytes = if mem.private_working_set_size > 0 { mem.private_working_set_size } else { mem.private_usage };
        let kb = (priv_bytes / 1024) as u32;
        if !text.is_empty() {
            text.push_str("   \u{00b7}   ");
        }
        text.push_str(&format!("{}.{} MB", kb / 1024, (kb % 1024) * 10 / 1024));

        let mut text_clip = status;
        if let Some(ref s) = APP.status {
            if !s.root.is_empty() && s.root[0] != 0 {
                text_clip.right = APP.eol_field.left;
                let box_sz = APP.text_h * 3 / 4;
                let check = RECT {
                    left: APP.eol_field.left,
                    top: status.top + (status_h - box_sz) / 2,
                    right: APP.eol_field.left + box_sz,
                    bottom: status.top + (status_h + box_sz) / 2,
                };
                FrameRect(dc, &check, APP.border_brush);
                if APP.ignore_eol {
                    let mark = RECT {
                        left: check.left + scale(3),
                        top: check.top + scale(3),
                        right: check.right - scale(3),
                        bottom: check.bottom - scale(3),
                    };
                    fill(dc, &mark, t.fg);
                }
                SetTextColor(dc, t.fg);
                let mut label = RECT { left: check.right + scale(6), top: status.top, right: APP.eol_field.right, bottom: status.bottom };
                let label_w = to_wide("Ignore line endings");
                DrawTextW(dc, label_w.as_ptr(), -1, &mut label, (DT_LEFT | DT_VCENTER | DT_SINGLELINE) as u32);
            }
        }

        if language_field_shown() {
            if let Some(ref d) = APP.diff {
                let field_str = format!(
                    "{}{}  \u{25BE}",
                    if d.language_auto { "Auto: " } else { "" },
                    LANGUAGE_NAMES[d.language]
                );
                let field_w = to_wide(&field_str);
                SetTextColor(dc, t.fg);
                let mut lf = APP.language_field;
                DrawTextW(dc, field_w.as_ptr(), -1, &mut lf, (DT_RIGHT | DT_VCENTER | DT_SINGLELINE) as u32);
            }
        }

        SetTextColor(dc, t.dim);
        let text_w = to_wide(&text);
        ExtTextOutW(dc, scale(PAD), status.top + (status_h - APP.text_h) / 2, ETO_CLIPPED as u32, &text_clip, text_w.as_ptr(), (text_w.len() - 1) as u32, null());

        EndPaint(hwnd, &ps);
    }
}

fn clear_diff() {
    unsafe {
        APP.diff = None;
        APP.diff_generation += 1;
        diff_scroll_to(0, 0);
    }
}

fn on_status(r: Box<StatusResult>) {
    unsafe {
        let mut sel = 0i32;
        let mut keep_top = 0i32;
        let mut same_file = false;

        if r.auto_refresh && APP.status.is_some() && APP.sel >= 0 {
            let old_s = APP.status.as_ref().unwrap();
            let old_row = &old_s.rows[APP.sel as usize];
            let old_path = &old_s.files[old_row.file_index].path;
            sel = (APP.sel).min(r.rows.len() as i32 - 1);
            for (i, row) in r.rows.iter().enumerate() {
                if row.row_type == old_row.row_type && &r.files[row.file_index].path == old_path {
                    sel = i as i32;
                    same_file = true;
                    break;
                }
            }
            keep_top = APP.top;
        }

        APP.status = Some(r);
        APP.loading = false;
        APP.sel = -1;
        if !same_file {
            APP.language_pick = LANG_AUTO;
        }
        let s = APP.status.as_ref().unwrap();
        if !s.auto_refresh && s.error.is_empty() {
            SetWindowTextW(APP.edits[EDIT_PATH], s.root.as_ptr());
        }
        list_scroll_to(keep_top);
        InvalidateRect(APP.main, null(), 0);

        if s.rows.is_empty() {
            clear_diff();
            if APP.perf {
                let err = s.error.clone();
                perf_finish(&err);
            }
            return;
        }

        if same_file && APP.diff.is_some() {
            let d = APP.diff.as_ref().unwrap();
            if d.generation == APP.diff_generation && diff_is_current(d, s, &s.rows[sel as usize]) {
                APP.sel = sel;
            } else {
                APP.keep_scroll_generation = APP.diff_generation + 1;
            }
        }
        if !s.auto_refresh {
            clear_diff();
        }
        list_select(sel, 1);
    }
}

fn on_diff(r: Box<DiffResult>) {
    unsafe {
        if r.generation != APP.diff_generation {
            return;
        }
        let error = r.error.clone();
        APP.diff = Some(r);
        let d = APP.diff.as_ref().unwrap();
        APP.change_row = -1;

        if d.generation == APP.change_generation {
            let row = find_change(if APP.change_dir > 0 { -1 } else { diff_rows() as i32 }, APP.change_dir);
            if row >= 0 {
                show_change(row);
            } else {
                diff_scroll_to(0, 0);
            }
        } else if d.generation == APP.keep_scroll_generation {
            diff_scroll_to(APP.diff_top, APP.diff_left);
        } else {
            diff_scroll_to(0, 0);
            let first = find_change(-1, 1);
            if first >= 0 && first < diff_visible_rows() {
                APP.change_row = first;
            }
        }

        InvalidateRect(APP.main, null(), 0);
        if APP.perf {
            perf_finish(&error);
        }
    }
}

fn on_refs(r: Box<RefList>) {
    unsafe {
        if r.generation != APP.refs_generation {
            return;
        }
        APP.refs = Some(r);
        if APP.refs_edit >= 0 {
            refs_filter();
            refs_place();
        }
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            APP.main = hwnd;
            APP.dpi = GetDpiForWindow(hwnd);
            make_fonts();
            let inst = GetModuleHandleW(null());
            let edit_class = to_wide("EDIT");
            let initial_folder_w = to_wide(&APP.folder);

            for i in 0..EDIT_COUNT {
                let text_ptr = if i == EDIT_PATH { initial_folder_w.as_ptr() } else { null() };
                APP.edits[i] = CreateWindowExW(
                    0,
                    edit_class.as_ptr(),
                    text_ptr,
                    WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
                    0, 0, 0, 0,
                    hwnd,
                    null_mut(),
                    inst,
                    null(),
                );
                EDIT_PROC_ORIGINAL = std::mem::transmute(SetWindowLongPtrW(APP.edits[i], GWLP_WNDPROC, edit_proc as *const () as usize as isize));
                SendMessageW(APP.edits[i], WM_SETFONT, APP.ui_font as usize, 0);
            }

            let list_class = to_wide("DiffViewerList");
            APP.list = CreateWindowExW(
                0,
                list_class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL,
                0, 0, 0, 0,
                hwnd,
                null_mut(),
                inst,
                null(),
            );

            let diff_class = to_wide("DiffViewerDiff");
            APP.diff_wnd = CreateWindowExW(
                0,
                diff_class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_HSCROLL,
                0, 0, 0, 0,
                hwnd,
                null_mut(),
                inst,
                null(),
            );

            apply_theme();
            0
        }
        WM_SIZE => {
            refs_hide();
            layout();
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_MOVE => {
            refs_hide();
            0
        }
        WM_COMMAND => {
            let which = if lp == APP.edits[EDIT_BASE] as LPARAM {
                EDIT_BASE as i32
            } else if lp == APP.edits[EDIT_TARGET] as LPARAM {
                EDIT_TARGET as i32
            } else {
                -1
            };
            let code = (wp >> 16) as u32;
            if code == EN_CHANGE && which >= 0 && !APP.refs_setting_text && GetFocus() == lp as HWND {
                if APP.refs_edit != which {
                    refs_open(which as usize);
                }
                APP.refs_typed = true;
                APP.refs_top = 0;
                refs_filter();
                refs_place();
            }
            0
        }
        WM_PAINT => {
            main_paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN => {
            refs_hide();
            let p = POINT {
                x: ((lp & 0xFFFF) as i16) as i32,
                y: (((lp >> 16) & 0xFFFF) as i16) as i32,
            };
            if PtInRect(&APP.toggle, p) != 0 {
                let mid = (APP.toggle.left + APP.toggle.right) / 2;
                if (p.x >= mid) != APP.side_by_side {
                    toggle_view();
                }
            }
            if PtInRect(&APP.nav, p) != 0 {
                let mid = (APP.nav.left + APP.nav.right) / 2;
                goto_change(if p.x >= mid { 1 } else { -1 });
            }
            if PtInRect(&APP.language_field, p) != 0 && language_field_shown() {
                language_menu();
            }
            if PtInRect(&APP.eol_field, p) != 0 && APP.status.is_some() && !APP.status.as_ref().unwrap().root.is_empty() {
                APP.ignore_eol = !APP.ignore_eol;
                InvalidateRect(hwnd, &APP.eol_field, 0);
                if language_field_shown() {
                    reload_diff();
                }
            }
            let split = split_rect();
            if PtInRect(&split, p) != 0 {
                APP.drag_dx = p.x - split.left;
                SetCapture(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            if APP.drag_dx >= 0 {
                let x = ((lp & 0xFFFF) as i16) as i32;
                APP.list_w96 = ((x - APP.drag_dx) as i64 * 96 / APP.dpi as i64) as i32;
                APP.list_w96 = (list_width() as i64 * 96 / APP.dpi as i64) as i32;
                layout();
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            0
        }
        WM_CAPTURECHANGED => {
            APP.drag_dx = -1;
            0
        }
        WM_SETCURSOR => {
            let mut p: POINT = std::mem::zeroed();
            GetCursorPos(&mut p);
            ScreenToClient(hwnd, &mut p);
            let split = split_rect();
            if wp == hwnd as WPARAM && PtInRect(&split, p) != 0 {
                SetCursor(LoadCursorW(null_mut(), IDC_SIZEWE));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SETFOCUS => {
            SetFocus(APP.list);
            0
        }
        WM_CTLCOLOREDIT => {
            SetTextColor(wp as HDC, APP.theme.fg);
            SetBkColor(wp as HDC, APP.theme.edit_bg);
            APP.edit_brush as LRESULT
        }
        WM_DPICHANGED => {
            APP.dpi = (wp & 0xFFFF) as u32;
            make_fonts();
            let r = &*(lp as *const RECT);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            layout();
            RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
            0
        }
        WM_SETTINGCHANGE => {
            if lp != 0 {
                let s = from_wide(std::slice::from_raw_parts(lp as *const u16, 20));
                if s.starts_with("ImmersiveColorSet") {
                    apply_theme();
                    RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME);
                }
            }
            0
        }
        WM_APP_STATUS => {
            let res = Box::from_raw(lp as *mut StatusResult);
            on_status(res);
            0
        }
        WM_APP_DIFF => {
            let res = Box::from_raw(lp as *mut DiffResult);
            on_diff(res);
            0
        }
        WM_APP_REFS => {
            let res = Box::from_raw(lp as *mut RefList);
            on_refs(res);
            0
        }
        WM_TIMER => {
            perf_print("error: timed out waiting for git\r\n");
            APP.exit_code = 2;
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(APP.exit_code);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn next_arg<'a>(cursor: &mut &'a [u16]) -> Option<String> {
    while !cursor.is_empty() && (cursor[0] == b' ' as u16 || cursor[0] == b'\t' as u16) {
        *cursor = &cursor[1..];
    }
    if cursor.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    let mut quoted = false;
    let mut idx = 0;
    while idx < cursor.len() {
        let c = cursor[idx];
        if c == b'"' as u16 {
            quoted = !quoted;
            idx += 1;
            continue;
        }
        if !quoted && (c == b' ' as u16 || c == b'\t' as u16) {
            break;
        }
        out.push(c);
        idx += 1;
    }
    *cursor = &cursor[idx..];
    Some(String::from_utf16_lossy(&out))
}

fn main() {
    unsafe {
        let cmd_ptr = GetCommandLineW();
        let mut len = 0;
        while *cmd_ptr.add(len) != 0 { len += 1; }
        let mut cmd_slice = std::slice::from_raw_parts(cmd_ptr, len);

        next_arg(&mut cmd_slice);

        let mut cwd_buf = [0u16; 1024];
        let cwd_len = GetCurrentDirectoryW(cwd_buf.len() as u32, cwd_buf.as_mut_ptr());
        APP.folder = from_wide(&cwd_buf[..cwd_len as usize]);

        while let Some(arg) = next_arg(&mut cmd_slice) {
            if arg == "--perf" {
                APP.perf = true;
            } else {
                let mut buf = [0u16; 1024];
                let arg_w = to_wide(&arg);
                let n = windows_sys::Win32::Storage::FileSystem::GetFullPathNameW(arg_w.as_ptr(), buf.len() as u32, buf.as_mut_ptr(), null_mut());
                if n > 0 {
                    APP.folder = from_wide(&buf[..n as usize]);
                }
            }
        }

        let inst = GetModuleHandleW(null());
        let arrow = LoadCursorW(null_mut(), IDC_ARROW);
        let ibeam = LoadCursorW(null_mut(), IDC_IBEAM);

        let main_class_w = to_wide("DiffViewer");
        let list_class_w = to_wide("DiffViewerList");
        let diff_class_w = to_wide("DiffViewerDiff");
        let refs_class_w = to_wide("DiffViewerRefs");

        let main_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(main_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: null_mut(),
            hCursor: arrow,
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: main_class_w.as_ptr(),
        };

        let list_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(list_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: null_mut(),
            hCursor: arrow,
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: list_class_w.as_ptr(),
        };

        let diff_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(diff_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: null_mut(),
            hCursor: ibeam,
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: diff_class_w.as_ptr(),
        };

        let refs_class = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(refs_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: null_mut(),
            hCursor: arrow,
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: refs_class_w.as_ptr(),
        };

        if RegisterClassW(&main_class) == 0
            || RegisterClassW(&list_class) == 0
            || RegisterClassW(&diff_class) == 0
            || RegisterClassW(&refs_class) == 0
        {
            let msg = to_wide("RegisterClass failed.");
            let title = to_wide("DiffViewer");
            MessageBoxW(null_mut(), msg.as_ptr(), title.as_ptr(), MB_ICONERROR);
            std::process::exit(1);
        }

        APP.loading = true;
        APP.sel = -1;
        APP.side_by_side = true;
        APP.change_row = -1;
        APP.list_w96 = LIST_W;
        APP.ignore_eol = true;
        APP.drag_dx = -1;
        APP.refs_edit = -1;

        let title = to_wide("DiffViewer");
        let hwnd = CreateWindowExW(
            0,
            main_class_w.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            null_mut(),
            null_mut(),
            inst,
            null(),
        );

        if hwnd.is_null() {
            let msg = to_wide("CreateWindow failed.");
            MessageBoxW(null_mut(), msg.as_ptr(), title.as_ptr(), MB_ICONERROR);
            std::process::exit(1);
        }

        git_start(hwnd);
        git_request_status(&APP.folder, "", "");

        ShowWindow(hwnd, if APP.perf { SW_SHOWNOACTIVATE } else { SW_SHOWDEFAULT });
        UpdateWindow(hwnd);

        if APP.perf {
            SetTimer(hwnd, 1, 30000, None);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        std::process::exit(msg.wParam as i32);
    }
}
