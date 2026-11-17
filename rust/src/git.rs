use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::*;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::syntax::{syntax_language_for_path, syntax_scan_line};
use crate::types::*;

extern "system" {
    fn GetTickCount() -> u32;
}

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const DIFF_OPTIONS: &str = "--no-color --no-ext-diff --no-textconv -U999999 --histogram -M";
const REFRESH_DELAY_MS: u32 = 300;

pub const DIFF_UNSTAGED: i32 = 0;
pub const DIFF_STAGED: i32 = 1;
pub const DIFF_UNTRACKED: i32 = 2;
pub const DIFF_UNMERGED: i32 = 3;
pub const DIFF_REFS: i32 = 4;

#[derive(Clone, Default)]
struct StatusRequest {
    folder: String,
    base: String,
    target: String,
}

#[derive(Clone)]
struct DiffRequest {
    root: String,
    path: String,
    orig_path: String,
    base: String,
    target: String,
    mode: i32,
    has_head: bool,
    generation: i32,
    language: usize,
    language_auto: bool,
    ignore_eol: bool,
    x: u8,
    y: u8,
    hashes: [u8; 130],
}

impl Default for DiffRequest {
    fn default() -> Self {
        Self {
            root: String::new(),
            path: String::new(),
            orig_path: String::new(),
            base: String::new(),
            target: String::new(),
            mode: 0,
            has_head: false,
            generation: 0,
            language: 0,
            language_auto: false,
            ignore_eol: false,
            x: 0,
            y: 0,
            hashes: [0u8; 130],
        }
    }
}

#[derive(Clone, Default)]
struct RefsRequest {
    root: String,
    generation: i32,
}

struct WorkerShared {
    status_req: Option<StatusRequest>,
    diff_req: Option<DiffRequest>,
    refs_req: Option<RefsRequest>,
}

static NOTIFY_HWND: AtomicIsize = AtomicIsize::new(0);

struct WorkerContext {
    shared: Arc<Mutex<WorkerShared>>,
    event: HANDLE,
}

static mut WORKER_CTX: Option<WorkerContext> = None;

pub fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub fn from_wide(w: &[u16]) -> String {
    let len = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..len])
}

fn append_quoted(cmd: &mut String, text: &str) {
    cmd.push_str(" \"");
    cmd.push_str(text);
    cmd.push('"');
}

fn git_run(
    folder: &str,
    args: &str,
    ok_codes: u32,
    out: &mut Vec<u8>,
    err: &mut Vec<u16>,
) -> i32 {
    unsafe {
        let mut inherit: SECURITY_ATTRIBUTES = std::mem::zeroed();
        inherit.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
        inherit.bInheritHandle = TRUE;

        let mut out_read: HANDLE = null_mut();
        let mut out_write: HANDLE = null_mut();
        let mut err_read: HANDLE = null_mut();
        let mut err_write: HANDLE = null_mut();

        if CreatePipe(&mut out_read, &mut out_write, &inherit, 0) == 0
            || CreatePipe(&mut err_read, &mut err_write, &inherit, 65536) == 0
        {
            *err = to_wide("CreatePipe failed.");
            return -1;
        }

        SetHandleInformation(out_read, 1, 0); // HANDLE_FLAG_INHERIT = 1
        SetHandleInformation(err_read, 1, 0);

        let nul_name = to_wide("NUL");
        let null_in = CreateFileW(
            nul_name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &inherit,
            OPEN_EXISTING,
            0,
            null_mut(),
        );

        let command_line = format!("git --no-optional-locks {}", args);
        let mut cmd_wide = to_wide(&command_line);

        let folder_wide = if !folder.is_empty() {
            Some(to_wide(folder))
        } else {
            None
        };

        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdInput = null_in;
        si.hStdOutput = out_write;
        si.hStdError = err_write;

        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        let started = CreateProcessW(
            null(),
            cmd_wide.as_mut_ptr(),
            null(),
            null(),
            TRUE,
            CREATE_NO_WINDOW,
            null(),
            if let Some(ref f) = folder_wide { f.as_ptr() } else { null() },
            &si,
            &mut pi,
        );
        let start_error = GetLastError();

        CloseHandle(out_write);
        CloseHandle(err_write);
        if null_in != INVALID_HANDLE_VALUE && !null_in.is_null() {
            CloseHandle(null_in);
        }

        if started == 0 {
            CloseHandle(out_read);
            CloseHandle(err_read);
            if start_error == ERROR_FILE_NOT_FOUND {
                *err = to_wide("git.exe was not found on PATH.");
            } else if start_error == ERROR_DIRECTORY {
                *err = to_wide(&format!("Folder not found: {}", folder));
            } else {
                *err = to_wide(&format!("Could not start git (error {}).", start_error));
            }
            return -1;
        }

        let mut buffer = [0u8; 65536];
        loop {
            let mut got: u32 = 0;
            let success = ReadFile(out_read, buffer.as_mut_ptr(), buffer.len() as u32, &mut got, null_mut());
            if success == 0 || got == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..got as usize]);
        }

        WaitForSingleObject(pi.hProcess, INFINITE);
        let mut code: u32 = 0;
        GetExitCodeProcess(pi.hProcess, &mut code);
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);

        let result = if code < 32 && ((ok_codes & (1u32 << code)) != 0) {
            0
        } else {
            let mut msg = [0u8; 1024];
            let mut got: u32 = 0;
            ReadFile(err_read, msg.as_mut_ptr(), msg.len() as u32 - 1, &mut got, null_mut());
            let mut got_usize = got as usize;
            while got_usize > 0 && (msg[got_usize - 1] == b'\n' || msg[got_usize - 1] == b'\r') {
                got_usize -= 1;
            }
            if got_usize > 0 {
                let s = String::from_utf8_lossy(&msg[..got_usize]);
                *err = to_wide(&s);
            } else {
                *err = to_wide(&format!("git exited with code {}.", code));
            }
            code as i32
        };

        CloseHandle(out_read);
        CloseHandle(err_read);
        result
    }
}

fn skip_fields(mut s: &str, mut count: usize) -> &str {
    while count > 0 && !s.is_empty() {
        if let Some(pos) = s.find(' ') {
            s = &s[pos + 1..];
            count -= 1;
        } else {
            return "";
        }
    }
    s
}

fn load_working_tree(r: &mut StatusResult, out: &[u8]) {
    let records: Vec<&[u8]> = out.split(|&b| b == 0).filter(|rec| !rec.is_empty()).collect();
    let mut i = 0;
    while i < records.len() {
        let rec = records[i];
        let kind = rec[0];
        match kind {
            b'1' | b'2' | b'u' => {
                let rec_str = String::from_utf8_lossy(rec);
                let x = rec[2];
                let y = rec[3];
                let field_count = if kind == b'1' { 8 } else if kind == b'2' { 9 } else { 10 };
                let path = skip_fields(&rec_str, field_count).to_string();
                let orig_path = if kind == b'2' {
                    i += 1;
                    if i < records.len() {
                        Some(String::from_utf8_lossy(records[i]).to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };
                let hashes = if kind == b'u' {
                    None
                } else {
                    let h_start = skip_fields(&rec_str, 6);
                    let h_end = skip_fields(&rec_str, 8);
                    let len = if !h_start.is_empty() && !h_end.is_empty() && h_end.as_ptr() > h_start.as_ptr() {
                        h_end.as_ptr() as usize - h_start.as_ptr() as usize - 1
                    } else {
                        h_start.len()
                    };
                    Some(h_start[..len.min(h_start.len())].to_string())
                };

                r.files.push(FileEntry {
                    kind,
                    x,
                    y,
                    path,
                    orig_path,
                    hashes,
                });
            }
            b'?' => {
                let rec_str = String::from_utf8_lossy(rec);
                let path = rec_str[2..].to_string();
                r.files.push(FileEntry {
                    kind: b'?',
                    x: b'?',
                    y: b'?',
                    path,
                    orig_path: None,
                    hashes: None,
                });
            }
            b'#' => {
                let rec_str = String::from_utf8_lossy(rec);
                if rec_str.starts_with("# branch.oid (initial)") {
                    r.has_head = false;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut staged = 0;
    let mut unstaged = 0;
    for f in &r.files {
        if f.kind != b'?' && f.kind != b'u' && f.x != b'.' {
            staged += 1;
        }
        if f.kind == b'?' || f.kind == b'u' || f.y != b'.' {
            unstaged += 1;
        }
    }

    if staged > 0 {
        r.rows.push(ListRow {
            row_type: RowType::StagedHeader,
            file_index: 0,
        });
        for (idx, f) in r.files.iter().enumerate() {
            if f.kind != b'?' && f.kind != b'u' && f.x != b'.' {
                r.rows.push(ListRow {
                    row_type: RowType::Staged,
                    file_index: idx,
                });
            }
        }
        if unstaged > 0 {
            r.rows.push(ListRow {
                row_type: RowType::UnstagedHeader,
                file_index: 0,
            });
        }
    }
    for (idx, f) in r.files.iter().enumerate() {
        if f.kind == b'?' || f.kind == b'u' || f.y != b'.' {
            r.rows.push(ListRow {
                row_type: RowType::Unstaged,
                file_index: idx,
            });
        }
    }
}

fn load_ref_compare(r: &mut StatusResult, out: &[u8]) {
    let records: Vec<&[u8]> = out.split(|&b| b == 0).filter(|rec| !rec.is_empty()).collect();
    let mut i = 0;
    while i < records.len() {
        let status = String::from_utf8_lossy(records[i]);
        let x = status.as_bytes().first().copied().unwrap_or(b'M');
        let orig_path = if x == b'R' || x == b'C' {
            i += 1;
            if i < records.len() {
                Some(String::from_utf8_lossy(records[i]).to_string())
            } else {
                None
            }
        } else {
            None
        };
        i += 1;
        let path = if i < records.len() {
            String::from_utf8_lossy(records[i]).to_string()
        } else {
            String::new()
        };

        r.files.push(FileEntry {
            kind: b'r',
            x,
            y: b'.',
            path,
            orig_path,
            hashes: None,
        });
        i += 1;
    }

    for idx in 0..r.files.len() {
        r.rows.push(ListRow {
            row_type: RowType::Ref,
            file_index: idx,
        });
    }
}

fn load_status(req: &StatusRequest, auto_refresh: bool) -> StatusResult {
    let mut r = StatusResult {
        root: Vec::new(),
        base: to_wide(&req.base),
        target: to_wide(&req.target),
        has_head: true,
        auto_refresh,
        files: Vec::new(),
        rows: Vec::new(),
        error: Vec::new(),
        elapsed_ms: 0,
    };

    let start_tick = unsafe { GetTickCount() };
    let mut out = Vec::new();

    if git_run(&req.folder, "rev-parse --show-toplevel", 1, &mut out, &mut r.error) != 0 {
        r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
        return r;
    }

    while !out.is_empty() && (out[out.len() - 1] == b'\n' || out[out.len() - 1] == b'\r') {
        out.pop();
    }
    let mut root_str = String::from_utf8_lossy(&out).to_string();
    root_str = root_str.replace('/', "\\");
    r.root = to_wide(&root_str);

    if req.base.is_empty() && !req.target.is_empty() {
        r.error = to_wide("Fill in the base ref too, or clear the target ref.");
        r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
        return r;
    }

    if req.base.is_empty() {
        out.clear();
        if git_run(&root_str, "status --porcelain=v2 -z --branch --untracked-files=all", 1, &mut out, &mut r.error) == 0 {
            load_working_tree(&mut r, &out);
        }
        r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
        return r;
    }

    if req.base.starts_with('-') || req.target.starts_with('-') {
        r.error = to_wide("A ref can't start with '-'.");
        r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
        return r;
    }
    if req.base.contains('"') || req.base.contains('\\') || req.target.contains('"') || req.target.contains('\\') {
        r.error = to_wide("A ref can't contain '\"' or '\\'.");
        r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
        return r;
    }

    let mut args = "diff --name-status -z -M --no-color".to_string();
    append_quoted(&mut args, &req.base);
    if !req.target.is_empty() {
        append_quoted(&mut args, &req.target);
    }
    args.push_str(" --");

    out.clear();
    if git_run(&root_str, &args, 1, &mut out, &mut r.error) == 0 {
        load_ref_compare(&mut r, &out);
    }

    r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
    r
}

fn load_refs(req: &RefsRequest) -> RefList {
    let mut r = RefList {
        generation: req.generation,
        root: to_wide(&req.root),
        refs: Vec::new(),
        shown: Vec::new(),
        error: Vec::new(),
    };

    let mut out_branches = Vec::new();
    let mut out_tags = Vec::new();
    let mut out_log = Vec::new();

    if git_run(&req.root, "for-each-ref --sort=-committerdate --format=%(refname) refs/heads refs/remotes", 1, &mut out_branches, &mut r.error) != 0
        || git_run(&req.root, "for-each-ref --sort=-v:refname --format=%(refname) refs/tags", 1, &mut out_tags, &mut r.error) != 0
    {
        return r;
    }

    let _ = git_run(&req.root, "log -n 30 --format=%h%x09%s", 1, &mut out_log, &mut Vec::new());

    if !out_log.is_empty() {
        r.refs.push(RefEntry {
            kind: b'H',
            name: "HEAD".to_string(),
            detail: None,
        });
    }

    let prefixes = ["refs/heads/", "refs/remotes/", "refs/tags/"];
    let kinds = [b'b', b'r', b't'];

    for (out, _is_tag) in [(&out_branches, false), (&out_tags, true)] {
        let text = String::from_utf8_lossy(out);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            for (p_idx, prefix) in prefixes.iter().enumerate() {
                if let Some(name) = line.strip_prefix(prefix) {
                    if kinds[p_idx] == b'r' && name.ends_with("/HEAD") {
                        continue;
                    }
                    r.refs.push(RefEntry {
                        kind: kinds[p_idx],
                        name: name.to_string(),
                        detail: None,
                    });
                }
            }
        }
    }

    let log_text = String::from_utf8_lossy(&out_log);
    for line in log_text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Some((hash, subj)) = line.split_once('\t') {
            r.refs.push(RefEntry {
                kind: b'c',
                name: hash.to_string(),
                detail: Some(subj.to_string()),
            });
        }
    }

    r
}

fn build_side_rows(lines: &[DiffLine]) -> Vec<SideRow> {
    let mut side = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let kind = lines[i].kind;
        if kind == LINE_REMOVED || kind == LINE_ADDED {
            let mut removed_end = i;
            while removed_end < lines.len() && lines[removed_end].kind == LINE_REMOVED {
                removed_end += 1;
            }
            let mut added_end = removed_end;
            while added_end < lines.len() && lines[added_end].kind == LINE_ADDED {
                added_end += 1;
            }
            let removed = removed_end - i;
            let added = added_end - removed_end;
            let max_k = removed.max(added);
            for k in 0..max_k {
                side.push(SideRow {
                    left: if k < removed { (i + k) as i32 } else { -1 },
                    right: if k < added { (removed_end + k) as i32 } else { -1 },
                });
            }
            i = added_end;
        } else {
            side.push(SideRow {
                left: i as i32,
                right: i as i32,
            });
            i += 1;
        }
    }
    side
}

const WORD_LINE_MAX: usize = 8192;
const WORD_TOKENS_MAX: usize = 1024;
const WORD_LCS_MAX: usize = 64;

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn split_tokens(s: &[u8], starts: &mut [usize]) -> Option<usize> {
    let mut n = 0;
    let mut i = 0;
    while i < s.len() {
        if n >= WORD_TOKENS_MAX {
            return None;
        }
        starts[n] = i;
        n += 1;
        let c = s[i];
        if is_word_byte(c) {
            while i < s.len() && is_word_byte(s[i]) {
                i += 1;
            }
        } else if c == b' ' || c == b'\t' {
            while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    starts[n] = s.len();
    Some(n)
}

fn same_token(a: &[u8], ta: &[usize], i: usize, b: &[u8], tb: &[usize], j: usize) -> bool {
    let sa = &a[ta[i]..ta[i + 1]];
    let sb = &b[tb[j]..tb[j + 1]];
    sa == sb
}

fn count_solid(s: &[u8], from: usize, to: usize) -> usize {
    s[from..to].iter().filter(|&&c| c != b' ' && c != b'\t').count()
}

fn push_span(word_spans: &mut Vec<WordSpan>, s: &[u8], line: usize, start: usize, end: usize) {
    if let Some(last) = word_spans.last_mut() {
        if last.line == line && count_solid(s, last.end as usize, start) == 0 {
            last.end = end as u16;
            return;
        }
    }
    word_spans.push(WordSpan {
        line,
        start: start as u16,
        end: end as u16,
    });
}

fn mark_pair(
    word_spans: &mut Vec<WordSpan>,
    lines: &[DiffLine],
    text: &[u8],
    old_line: usize,
    new_line: usize,
    new_side: bool,
) {
    let la = &lines[old_line];
    let lb = &lines[new_line];
    let a_full = &text[la.offset as usize..(la.offset + la.length) as usize];
    let b_full = &text[lb.offset as usize..(lb.offset + lb.length) as usize];
    let a = &a_full[..a_full.len().min(WORD_LINE_MAX)];
    let b = &b_full[..b_full.len().min(WORD_LINE_MAX)];

    let mut ta = [0usize; WORD_TOKENS_MAX + 1];
    let mut tb = [0usize; WORD_TOKENS_MAX + 1];

    let na = match split_tokens(a, &mut ta) {
        Some(n) => n,
        None => return,
    };
    let nb = match split_tokens(b, &mut tb) {
        Some(n) => n,
        None => return,
    };

    let mut pre = 0;
    while pre < na && pre < nb && same_token(a, &ta, pre, b, &tb, pre) {
        pre += 1;
    }
    let mut suf = 0;
    while suf < na - pre && suf < nb - pre && same_token(a, &ta, na - 1 - suf, b, &tb, nb - 1 - suf) {
        suf += 1;
    }

    let mut common = count_solid(a, 0, ta[pre]) + count_solid(a, ta[na - suf], a.len());
    let solid_a = count_solid(a, 0, a.len());
    let solid_b = count_solid(b, 0, b.len());
    let longer = solid_a.max(solid_b);
    let ma = na - suf - pre;
    let mb = nb - suf - pre;

    if ma > WORD_LCS_MAX || mb > WORD_LCS_MAX {
        if common * 2 < longer {
            return;
        }
        if new_side && mb > 0 {
            push_span(word_spans, b, new_line, tb[pre], tb[pre + mb]);
        } else if !new_side && ma > 0 {
            push_span(word_spans, a, old_line, ta[pre], ta[pre + ma]);
        }
        return;
    }

    let mut lcs = [[0u8; WORD_LCS_MAX + 1]; WORD_LCS_MAX + 1];
    for i in (0..=ma).rev() {
        for j in (0..=mb).rev() {
            if i == ma || j == mb {
                lcs[i][j] = 0;
            } else if same_token(a, &ta, pre + i, b, &tb, pre + j) {
                lcs[i][j] = lcs[i + 1][j + 1] + 1;
            } else if lcs[i + 1][j] > lcs[i][j + 1] {
                lcs[i][j] = lcs[i + 1][j];
            } else {
                lcs[i][j] = lcs[i][j + 1];
            }
        }
    }

    for pass in 0..2 {
        let mut i = 0;
        let mut j = 0;
        while i < ma || j < mb {
            if i < ma && j < mb && same_token(a, &ta, pre + i, b, &tb, pre + j) {
                if pass == 0 {
                    common += count_solid(a, ta[pre + i], ta[pre + i + 1]);
                }
                i += 1;
                j += 1;
            } else if j == mb || (i < ma && lcs[i + 1][j] >= lcs[i][j + 1]) {
                if pass == 1 && !new_side {
                    push_span(word_spans, a, old_line, ta[pre + i], ta[pre + i + 1]);
                }
                i += 1;
            } else {
                if pass == 1 && new_side {
                    push_span(word_spans, b, new_line, tb[pre + j], tb[pre + j + 1]);
                }
                j += 1;
            }
        }
        if pass == 0 && common * 2 < longer {
            return;
        }
    }
}

fn compute_word_spans(lines: &[DiffLine], side: &[SideRow], text: &[u8]) -> Vec<WordSpan> {
    let mut word_spans = Vec::new();
    let mut i = 0;
    while i < side.len() {
        let row = &side[i];
        if row.left < 0 || row.right < 0 || row.left == row.right {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < side.len()
            && side[end].left == side[end - 1].left + 1
            && side[end].right == side[end - 1].right + 1
        {
            end += 1;
        }
        for new_side in [false, true] {
            for k in i..end {
                mark_pair(&mut word_spans, lines, text, side[k].left as usize, side[k].right as usize, new_side);
            }
        }
        i = end;
    }
    word_spans
}

fn compute_syntax_states(lines: &[DiffLine], text: &[u8], language: usize) -> Vec<u8> {
    let mut states = vec![0u8; lines.len() * 2];
    let mut old_state = 0u8;
    let mut new_state = 0u8;

    for (i, line) in lines.iter().enumerate() {
        states[i * 2] = old_state;
        states[i * 2 + 1] = new_state;
        let s = &text[line.offset as usize..(line.offset + line.length) as usize];
        match line.kind {
            LINE_CONTEXT => {
                old_state = syntax_scan_line(language, s, old_state, &mut None);
                new_state = if states[i * 2] == states[i * 2 + 1] {
                    old_state
                } else {
                    syntax_scan_line(language, s, new_state, &mut None)
                };
            }
            LINE_REMOVED => {
                old_state = syntax_scan_line(language, s, old_state, &mut None);
            }
            LINE_ADDED => {
                new_state = syntax_scan_line(language, s, new_state, &mut None);
            }
            LINE_GAP => {
                old_state = 0;
                new_state = 0;
            }
            _ => {}
        }
    }
    states
}

pub fn file_stamp(root: &str, path: &str) -> (u64, u64) {
    let full_path = format!("{}\\{}", root, path);
    let full_wide = to_wide(&full_path);
    unsafe {
        let mut data: WIN32_FILE_ATTRIBUTE_DATA = std::mem::zeroed();
        if GetFileAttributesExW(full_wide.as_ptr(), 0, &mut data as *mut _ as *mut _) == 0 {
            return (0, 0);
        }
        let time = ((data.ftLastWriteTime.dwHighDateTime as u64) << 32) | (data.ftLastWriteTime.dwLowDateTime as u64);
        let size = ((data.nFileSizeHigh as u64) << 32) | (data.nFileSizeLow as u64);
        (time, size)
    }
}

fn parse_number(s: &[u8], idx: &mut usize) -> u32 {
    let mut n = 0u32;
    while *idx < s.len() && s[*idx].is_ascii_digit() {
        n = n * 10 + (s[*idx] - b'0') as u32;
        *idx += 1;
    }
    n
}

pub fn diff_advance_numbers(line: &DiffLine, old_no: &mut u32, new_no: &mut u32) {
    match line.kind {
        LINE_CONTEXT => {
            *old_no += 1;
            *new_no += 1;
        }
        LINE_REMOVED => *old_no += 1,
        LINE_ADDED => *new_no += 1,
        LINE_GAP => {
            *old_no = line.offset;
            *new_no = line.length;
        }
        _ => {}
    }
}

pub fn diff_line_numbers(d: &DiffResult, row: usize, old_no: &mut u32, new_no: &mut u32) {
    let mut i = row / 1024 * 1024;
    let cp = &d.checkpoints[i / 1024];
    *old_no = cp.old_no;
    *new_no = cp.new_no;
    while i < row {
        diff_advance_numbers(&d.lines[i], old_no, new_no);
        i += 1;
    }
}

fn load_diff(req: &DiffRequest) -> DiffResult {
    let mut r = DiffResult {
        generation: req.generation,
        text: Vec::new(),
        lines: Vec::new(),
        checkpoints: Vec::new(),
        side: Vec::new(),
        word_spans: Vec::new(),
        states: None,
        language: req.language,
        language_auto: req.language_auto,
        ignore_eol: req.ignore_eol,
        max_line_no: 0,
        max_line_len: 0,
        added: 0,
        removed: 0,
        binary: false,
        error: Vec::new(),
        elapsed_ms: 0,
        mode: req.mode,
        x: req.x,
        y: req.y,
        hashes: req.hashes,
        file_time: 0,
        file_size: 0,
    };

    let start_tick = unsafe { GetTickCount() };

    if req.mode == DIFF_UNSTAGED || req.mode == DIFF_UNTRACKED {
        let (time, size) = file_stamp(&req.root, &req.path);
        r.file_time = time;
        r.file_size = size;
    }

    let mut args = if req.ignore_eol {
        "diff --ignore-cr-at-eol ".to_string()
    } else {
        "diff ".to_string()
    };

    match req.mode {
        DIFF_UNTRACKED => {
            args.push_str("--no-index ");
            args.push_str(DIFF_OPTIONS);
            args.push_str(" -- NUL");
        }
        DIFF_UNSTAGED => {
            args.push_str(DIFF_OPTIONS);
            args.push_str(" --");
        }
        DIFF_UNMERGED => {
            args.push_str(DIFF_OPTIONS);
            args.push_str(" HEAD --");
        }
        DIFF_STAGED => {
            args.push_str(DIFF_OPTIONS);
            args.push_str(" --cached ");
            args.push_str(if req.has_head { "HEAD" } else { EMPTY_TREE });
            args.push_str(" --");
        }
        DIFF_REFS => {
            args.push_str(DIFF_OPTIONS);
            append_quoted(&mut args, &req.base);
            if !req.target.is_empty() {
                append_quoted(&mut args, &req.target);
            }
            args.push_str(" --");
        }
        _ => {}
    }

    if !req.orig_path.is_empty() {
        append_quoted(&mut args, &req.orig_path);
    }
    append_quoted(&mut args, &req.path);

    let ok_codes = if req.mode == DIFF_UNTRACKED { 3 } else { 1 };
    if git_run(&req.root, &args, ok_codes, &mut r.text, &mut r.error) != 0 {
        r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
        return r;
    }

    let mut old_no = 1u32;
    let mut new_no = 1u32;
    let mut in_hunk = false;

    let len = r.text.len();
    let mut p = 0;
    while p < len {
        let line_start = p;
        while p < len && r.text[p] != b'\n' {
            p += 1;
        }
        let mut line_end = p;
        p += 1;
        if line_end > line_start && r.text[line_end - 1] == b'\r' {
            line_end -= 1;
        }
        let line = &r.text[line_start..line_end];
        let line_len = line.len();

        let mut row = DiffLine {
            offset: 0,
            length: 0,
            kind: LINE_CONTEXT,
        };

        if line.starts_with(b"@@ -") {
            let mut q = 4;
            let hunk_old = parse_number(line, &mut q);
            if q < line.len() && line[q] == b',' {
                q += 1;
                parse_number(line, &mut q);
            }
            while q < line.len() && (line[q] == b' ' || line[q] == b'+') {
                q += 1;
            }
            let hunk_new = parse_number(line, &mut q);
            let hunk_old = if hunk_old == 0 { 1 } else { hunk_old };
            let hunk_new = if hunk_new == 0 { 1 } else { hunk_new };
            in_hunk = true;
            if r.lines.is_empty() {
                old_no = hunk_old;
                new_no = hunk_new;
                continue;
            }
            row.kind = LINE_GAP;
            row.offset = hunk_old;
            row.length = hunk_new;
        } else if !in_hunk {
            if line.starts_with(b"Binary files ") {
                r.binary = true;
            }
            continue;
        } else if line_len == 0 || line[0] == b'\\' {
            continue;
        } else {
            row.kind = if line[0] == b'-' {
                LINE_REMOVED
            } else if line[0] == b'+' {
                LINE_ADDED
            } else {
                LINE_CONTEXT
            };
            row.offset = (line_start + 1) as u32;
            row.length = (line_len - 1) as u32;
            if (line_len - 1) as u32 > r.max_line_len {
                r.max_line_len = (line_len - 1) as u32;
            }
            if row.kind == LINE_ADDED {
                r.added += 1;
            } else if row.kind == LINE_REMOVED {
                r.removed += 1;
            }
        }

        if r.lines.len() % 1024 == 0 {
            r.checkpoints.push(LineNumbers { old_no, new_no });
        }
        r.lines.push(row);
        diff_advance_numbers(&row, &mut old_no, &mut new_no);
    }

    r.max_line_no = old_no.max(new_no);
    r.side = build_side_rows(&r.lines);
    r.word_spans = compute_word_spans(&r.lines, &r.side, &r.text);
    if r.language != LANG_PLAIN {
        r.states = Some(compute_syntax_states(&r.lines, &r.text, r.language));
    }

    r.elapsed_ms = unsafe { GetTickCount() } - start_tick;
    r
}

fn watch_changes_matter(buffer: &[u8], bytes: u32) -> bool {
    if bytes == 0 {
        return true;
    }
    let mut offset = 0usize;
    while offset < bytes as usize {
        let info = unsafe { &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION) };
        let name_len = (info.FileNameLength / 2) as usize;
        let name_slice = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
        let name = String::from_utf16_lossy(name_slice);
        let in_git = name == ".git" || name.starts_with(".git\\") || name.starts_with(".git/");
        if !in_git || name.starts_with(".git\\index") || name.starts_with(".git/index")
            || name.starts_with(".git\\HEAD") || name.starts_with(".git/HEAD")
            || name.starts_with(".git\\refs\\") || name.starts_with(".git/refs/")
        {
            return true;
        }
        if info.NextEntryOffset == 0 {
            break;
        }
        offset += info.NextEntryOffset as usize;
    }
    false
}

pub fn diff_mode(f: &FileEntry, row: &ListRow) -> i32 {
    match row.row_type {
        RowType::Ref => DIFF_REFS,
        RowType::Staged => DIFF_STAGED,
        _ => {
            if f.kind == b'?' {
                DIFF_UNTRACKED
            } else if f.kind == b'u' {
                DIFF_UNMERGED
            } else {
                DIFF_UNSTAGED
            }
        }
    }
}

pub fn diff_is_current(d: &DiffResult, status: &StatusResult, row: &ListRow) -> bool {
    if row.file_index >= status.files.len() {
        return false;
    }
    let f = &status.files[row.file_index];
    let mode = diff_mode(f, row);
    if !d.error.is_empty() || mode != d.mode || f.x != d.x || f.y != d.y {
        return false;
    }
    if mode != DIFF_UNSTAGED && mode != DIFF_STAGED && mode != DIFF_UNTRACKED {
        return false;
    }
    let hashes_bytes = f.hashes.as_deref().map(|h| h.as_bytes()).unwrap_or(&[]);
    let d_hashes_len = d.hashes.iter().position(|&b| b == 0).unwrap_or(d.hashes.len());
    if &d.hashes[..d_hashes_len] != hashes_bytes {
        return false;
    }
    if mode == DIFF_STAGED {
        return true;
    }
    let (time, size) = file_stamp(&from_wide(&status.root), &f.path);
    time == d.file_time && size == d.file_size
}

pub fn git_start(notify: HWND) {
    NOTIFY_HWND.store(notify as isize, Ordering::SeqCst);
    let event = unsafe { CreateEventW(null(), FALSE, FALSE, null()) };

    let shared = Arc::new(Mutex::new(WorkerShared {
        status_req: None,
        diff_req: None,
        refs_req: None,
    }));

    let shared_clone = Arc::clone(&shared);
    let event_usize = event as usize;

    unsafe {
        WORKER_CTX = Some(WorkerContext {
            shared,
            event,
        });
    }

    thread::spawn(move || {
        let event = event_usize as HANDLE;
        let mut watch_dir: HANDLE = INVALID_HANDLE_VALUE;
        let mut watch_overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        watch_overlapped.hEvent = unsafe { CreateEventW(null(), TRUE, FALSE, null()) };
        let mut watch_root = String::new();
        let mut watch_buffer = vec![0u8; 32768];

        let mut worker_status = StatusRequest::default();
        let mut refresh_due = false;
        let mut refresh_at = 0u32;

        let watch_issue = |dir: HANDLE, buf: &mut [u8], ov: &mut OVERLAPPED| -> bool {
            let res = unsafe {
                ReadDirectoryChangesW(
                    dir,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32,
                    TRUE,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_DIR_NAME
                        | FILE_NOTIFY_CHANGE_SIZE
                        | FILE_NOTIFY_CHANGE_LAST_WRITE,
                    null_mut(),
                    ov,
                    None,
                )
            };
            res != 0
        };

        loop {
            let waits = [event, watch_overlapped.hEvent];
            let wait_count = if watch_dir != INVALID_HANDLE_VALUE { 2 } else { 1 };
            let mut timeout = INFINITE;

            if refresh_due {
                let now = unsafe { GetTickCount() };
                let left = (refresh_at as i32) - (now as i32);
                timeout = if left > 0 { left as u32 } else { 0 };
            }

            let woke = unsafe { WaitForMultipleObjects(wait_count, waits.as_ptr(), FALSE, timeout) };

            if woke == WAIT_OBJECT_0 + 1 {
                let mut bytes: u32 = 0;
                unsafe {
                    GetOverlappedResult(watch_dir, &mut watch_overlapped, &mut bytes, FALSE);
                }
                if watch_changes_matter(&watch_buffer, bytes) {
                    refresh_due = true;
                    refresh_at = unsafe { GetTickCount() } + REFRESH_DELAY_MS;
                }
                if !watch_issue(watch_dir, &mut watch_buffer, &mut watch_overlapped) {
                    unsafe { CloseHandle(watch_dir); }
                    watch_dir = INVALID_HANDLE_VALUE;
                }
                continue;
            }

            if woke == WAIT_TIMEOUT {
                refresh_due = false;
                if !worker_status.folder.is_empty() && worker_status.target.is_empty() {
                    let res = Box::new(load_status(&worker_status, true));
                    let hwnd = NOTIFY_HWND.load(Ordering::SeqCst) as HWND;
                    unsafe {
                        PostMessageW(hwnd, WM_APP_STATUS, 0, Box::into_raw(res) as isize);
                    }
                }
                continue;
            }

            let (do_status, do_diff, do_refs) = {
                let mut guard = shared_clone.lock().unwrap();
                (guard.status_req.take(), guard.diff_req.take(), guard.refs_req.take())
            };

            if let Some(req) = do_status {
                refresh_due = false;
                worker_status = req.clone();
                let res = Box::new(load_status(&req, false));

                let root_str = from_wide(&res.root);
                if !root_str.is_empty() && root_str != watch_root {
                    if watch_dir != INVALID_HANDLE_VALUE {
                        unsafe {
                            let mut b = 0;
                            CancelIoEx(watch_dir, &watch_overlapped);
                            GetOverlappedResult(watch_dir, &mut watch_overlapped, &mut b, TRUE);
                            CloseHandle(watch_dir);
                        }
                    }
                    watch_root = root_str.clone();
                    let root_w = to_wide(&watch_root);
                    watch_dir = unsafe {
                        CreateFileW(
                            root_w.as_ptr(),
                            FILE_LIST_DIRECTORY,
                            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                            null(),
                            OPEN_EXISTING,
                            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                            null_mut(),
                        )
                    };
                    if watch_dir != INVALID_HANDLE_VALUE {
                        if !watch_issue(watch_dir, &mut watch_buffer, &mut watch_overlapped) {
                            unsafe { CloseHandle(watch_dir); }
                            watch_dir = INVALID_HANDLE_VALUE;
                        }
                    }
                }

                let hwnd = NOTIFY_HWND.load(Ordering::SeqCst) as HWND;
                unsafe {
                    PostMessageW(hwnd, WM_APP_STATUS, 0, Box::into_raw(res) as isize);
                }
            }

            if let Some(req) = do_refs {
                let res = Box::new(load_refs(&req));
                let hwnd = NOTIFY_HWND.load(Ordering::SeqCst) as HWND;
                unsafe {
                    PostMessageW(hwnd, WM_APP_REFS, 0, Box::into_raw(res) as isize);
                }
            }

            if let Some(req) = do_diff {
                let res = Box::new(load_diff(&req));
                let hwnd = NOTIFY_HWND.load(Ordering::SeqCst) as HWND;
                unsafe {
                    PostMessageW(hwnd, WM_APP_DIFF, 0, Box::into_raw(res) as isize);
                }
            }
        }
    });
}

pub fn git_request_status(folder: &str, base: &str, target: &str) {
    unsafe {
        if let Some(ref ctx) = WORKER_CTX {
            {
                let mut guard = ctx.shared.lock().unwrap();
                guard.status_req = Some(StatusRequest {
                    folder: folder.to_string(),
                    base: base.to_string(),
                    target: target.to_string(),
                });
            }
            SetEvent(ctx.event);
        }
    }
}

pub fn git_request_refs(root: &str, generation: i32) {
    unsafe {
        if let Some(ref ctx) = WORKER_CTX {
            {
                let mut guard = ctx.shared.lock().unwrap();
                guard.refs_req = Some(RefsRequest {
                    root: root.to_string(),
                    generation,
                });
            }
            SetEvent(ctx.event);
        }
    }
}

pub fn git_request_diff(
    status: &StatusResult,
    row: &ListRow,
    generation: i32,
    language: usize,
    ignore_eol: bool,
) {
    if row.file_index >= status.files.len() {
        return;
    }
    let f = &status.files[row.file_index];
    let mode = diff_mode(f, row);
    let with_orig = f.orig_path.is_some() && (mode == DIFF_STAGED || mode == DIFF_REFS);

    let mut hashes = [0u8; 130];
    if let Some(ref h) = f.hashes {
        let b = h.as_bytes();
        let n = b.len().min(129);
        hashes[..n].copy_from_slice(&b[..n]);
    }

    unsafe {
        if let Some(ref ctx) = WORKER_CTX {
            {
                let mut guard = ctx.shared.lock().unwrap();
                guard.diff_req = Some(DiffRequest {
                    root: from_wide(&status.root),
                    path: f.path.clone(),
                    orig_path: if with_orig { f.orig_path.clone().unwrap_or_default() } else { String::new() },
                    base: from_wide(&status.base),
                    target: from_wide(&status.target),
                    mode,
                    has_head: status.has_head,
                    generation,
                    language: if language == LANG_AUTO { syntax_language_for_path(&f.path) } else { language },
                    language_auto: language == LANG_AUTO,
                    ignore_eol,
                    x: f.x,
                    y: f.y,
                    hashes,
                });
            }
            SetEvent(ctx.event);
        }
    }
}
