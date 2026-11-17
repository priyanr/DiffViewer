use windows_sys::Win32::Foundation::COLORREF;

pub const WM_APP_STATUS: u32 = 0x8000 + 1;
pub const WM_APP_DIFF: u32 = 0x8000 + 2;
pub const WM_APP_REFS: u32 = 0x8000 + 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowType {
    StagedHeader,
    UnstagedHeader,
    Staged,
    Unstaged,
    Ref,
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub kind: u8,
    pub x: u8,
    pub y: u8,
    pub path: String,
    pub orig_path: Option<String>,
    pub hashes: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct ListRow {
    pub row_type: RowType,
    pub file_index: usize,
}

#[derive(Clone, Debug)]
pub struct StatusResult {
    pub root: Vec<u16>,
    pub base: Vec<u16>,
    pub target: Vec<u16>,
    pub has_head: bool,
    pub auto_refresh: bool,
    pub files: Vec<FileEntry>,
    pub rows: Vec<ListRow>,
    pub error: Vec<u16>,
    pub elapsed_ms: u32,
}

pub const LINE_CONTEXT: u8 = 0;
pub const LINE_REMOVED: u8 = 1;
pub const LINE_ADDED: u8 = 2;
pub const LINE_GAP: u8 = 3;

#[derive(Clone, Copy, Debug)]
pub struct DiffLine {
    pub offset: u32,
    pub length: u32,
    pub kind: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LineNumbers {
    pub old_no: u32,
    pub new_no: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct WordSpan {
    pub line: usize,
    pub start: u16,
    pub end: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct SideRow {
    pub left: i32,
    pub right: i32,
}

#[derive(Clone, Debug)]
pub struct DiffResult {
    pub generation: i32,
    pub text: Vec<u8>,
    pub lines: Vec<DiffLine>,
    pub checkpoints: Vec<LineNumbers>,
    pub side: Vec<SideRow>,
    pub word_spans: Vec<WordSpan>,
    pub states: Option<Vec<u8>>, // pairs of [old, new] lexer state
    pub language: usize,
    pub language_auto: bool,
    pub ignore_eol: bool,
    pub max_line_no: u32,
    pub max_line_len: u32,
    pub added: i32,
    pub removed: i32,
    pub binary: bool,
    pub error: Vec<u16>,
    pub elapsed_ms: u32,
    pub mode: i32,
    pub x: u8,
    pub y: u8,
    pub hashes: [u8; 130],
    pub file_time: u64,
    pub file_size: u64,
}

#[derive(Clone, Debug)]
pub struct RefEntry {
    pub kind: u8, // 'H', 'b', 'r', 't', 'c'
    pub name: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RefList {
    pub generation: i32,
    pub root: Vec<u16>,
    pub refs: Vec<RefEntry>,
    pub shown: Vec<i32>,
    pub error: Vec<u16>,
}

pub const TOK_KEYWORD: u8 = 0;
pub const TOK_CONTROL: u8 = 1;
pub const TOK_TYPE: u8 = 2;
pub const TOK_METHOD: u8 = 3;
pub const TOK_STRING: u8 = 4;
pub const TOK_NUMBER: u8 = 5;
pub const TOK_COMMENT: u8 = 6;
pub const TOK_PREPROCESSOR: u8 = 7;

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: u8,
}

pub const LANG_AUTO: usize = 0;
pub const LANG_PLAIN: usize = 1;
pub const LANG_C: usize = 2;
pub const LANG_CS: usize = 3;
pub const LANG_CSS: usize = 4;
pub const LANG_GO: usize = 5;
pub const LANG_HTML: usize = 6;
pub const LANG_JAVA: usize = 7;
pub const LANG_JS: usize = 8;
pub const LANG_JSON: usize = 9;
pub const LANG_KOTLIN: usize = 10;
pub const LANG_POWERSHELL: usize = 11;
pub const LANG_PYTHON: usize = 12;
pub const LANG_RUST: usize = 13;
pub const LANG_SHELL: usize = 14;
pub const LANG_SQL: usize = 15;
pub const LANG_TS: usize = 16;
pub const LANG_XML: usize = 17;
pub const LANG_YAML: usize = 18;
pub const LANG_ZIG: usize = 19;
pub const LANG_COUNT: usize = 20;

pub const LANGUAGE_NAMES: [&str; LANG_COUNT] = [
    "Auto", "Plain text", "C / C++", "C#", "CSS", "Go", "HTML", "Java", "JavaScript", "JSON",
    "Kotlin", "PowerShell", "Python", "Rust", "Shell", "SQL", "TypeScript", "XML", "YAML", "Zig",
];

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: COLORREF,
    pub panel: COLORREF,
    pub fg: COLORREF,
    pub dim: COLORREF,
    pub sel: COLORREF,
    pub border: COLORREF,
    pub edit_bg: COLORREF,
    pub added: COLORREF,
    pub modified: COLORREF,
    pub deleted: COLORREF,
    pub untracked: COLORREF,
    pub renamed: COLORREF,
    pub conflict: COLORREF,
    pub added_bg: COLORREF,
    pub removed_bg: COLORREF,
    pub added_word_bg: COLORREF,
    pub removed_word_bg: COLORREF,
    pub syntax: [COLORREF; 8],
}

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub static DARK_THEME: Theme = Theme {
    bg: rgb(30, 30, 30),
    panel: rgb(37, 37, 38),
    fg: rgb(212, 212, 212),
    dim: rgb(140, 140, 140),
    sel: rgb(9, 71, 113),
    border: rgb(69, 69, 69),
    edit_bg: rgb(49, 49, 49),
    added: rgb(115, 201, 145),
    modified: rgb(226, 192, 141),
    deleted: rgb(241, 76, 76),
    untracked: rgb(78, 201, 176),
    renamed: rgb(197, 134, 192),
    conflict: rgb(244, 135, 113),
    added_bg: rgb(35, 57, 38),
    removed_bg: rgb(70, 35, 37),
    added_word_bg: rgb(46, 98, 55),
    removed_word_bg: rgb(125, 45, 48),
    syntax: [
        rgb(86, 156, 214),  // TOK_KEYWORD
        rgb(197, 134, 192), // TOK_CONTROL
        rgb(78, 201, 176),  // TOK_TYPE
        rgb(220, 220, 170), // TOK_METHOD
        rgb(206, 145, 120), // TOK_STRING
        rgb(181, 206, 168), // TOK_NUMBER
        rgb(106, 153, 85),  // TOK_COMMENT
        rgb(155, 155, 155), // TOK_PREPROCESSOR
    ],
};

pub static LIGHT_THEME: Theme = Theme {
    bg: rgb(255, 255, 255),
    panel: rgb(243, 243, 243),
    fg: rgb(30, 30, 30),
    dim: rgb(110, 110, 110),
    sel: rgb(204, 228, 247),
    border: rgb(200, 200, 200),
    edit_bg: rgb(255, 255, 255),
    added: rgb(46, 125, 50),
    modified: rgb(154, 103, 0),
    deleted: rgb(198, 40, 40),
    untracked: rgb(0, 121, 107),
    renamed: rgb(142, 36, 170),
    conflict: rgb(216, 67, 21),
    added_bg: rgb(218, 251, 225),
    removed_bg: rgb(255, 235, 233),
    added_word_bg: rgb(172, 238, 187),
    removed_word_bg: rgb(255, 196, 192),
    syntax: [
        rgb(0, 0, 255),     // TOK_KEYWORD
        rgb(175, 0, 219),   // TOK_CONTROL
        rgb(38, 127, 153),  // TOK_TYPE
        rgb(121, 94, 38),   // TOK_METHOD
        rgb(163, 21, 21),   // TOK_STRING
        rgb(9, 134, 88),    // TOK_NUMBER
        rgb(0, 128, 0),     // TOK_COMMENT
        rgb(128, 128, 128), // TOK_PREPROCESSOR
    ],
};
