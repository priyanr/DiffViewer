use crate::types::*;

const CS_KEYWORDS: &[&str] = &[
    "abstract", "add", "alias", "and", "as", "async", "await", "base", "bool", "by", "byte",
    "char", "checked", "class", "const", "decimal", "delegate", "descending", "double", "dynamic",
    "enum", "equals", "event", "explicit", "extern", "false", "file", "fixed", "float", "from",
    "get", "global", "group", "implicit", "in", "init", "int", "interface", "internal", "into",
    "is", "join", "let", "lock", "long", "managed", "nameof", "namespace", "new", "nint", "not",
    "notnull", "nuint", "null", "object", "on", "operator", "or", "orderby", "out", "override",
    "params", "partial", "private", "protected", "public", "readonly", "record", "ref", "remove",
    "required", "sbyte", "scoped", "sealed", "select", "set", "short", "sizeof", "stackalloc",
    "static", "string", "struct", "this", "true", "typeof", "uint", "ulong", "unchecked",
    "unmanaged", "unsafe", "ushort", "using", "value", "var", "virtual", "void", "volatile",
    "when", "where", "with",
];

const CS_CONTROL_KEYWORDS: &[&str] = &[
    "break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "foreach",
    "goto", "if", "return", "switch", "throw", "try", "while", "yield",
];

const EXTENSIONS: &[(&str, usize)] = &[
    ("c", LANG_C), ("h", LANG_C), ("cc", LANG_C), ("cpp", LANG_C), ("cxx", LANG_C),
    ("hpp", LANG_C), ("hh", LANG_C), ("hxx", LANG_C), ("inl", LANG_C),
    ("cs", LANG_CS), ("csx", LANG_CS),
    ("css", LANG_CSS), ("scss", LANG_CSS), ("less", LANG_CSS),
    ("go", LANG_GO),
    ("html", LANG_HTML), ("htm", LANG_HTML),
    ("java", LANG_JAVA),
    ("json", LANG_JSON), ("jsonc", LANG_JSON),
    ("kt", LANG_KOTLIN), ("kts", LANG_KOTLIN),
    ("ps1", LANG_POWERSHELL), ("psm1", LANG_POWERSHELL), ("psd1", LANG_POWERSHELL),
    ("py", LANG_PYTHON), ("pyw", LANG_PYTHON), ("pyi", LANG_PYTHON),
    ("rs", LANG_RUST),
    ("sh", LANG_SHELL), ("bash", LANG_SHELL), ("zsh", LANG_SHELL),
    ("sql", LANG_SQL),
    ("xml", LANG_XML), ("xaml", LANG_XML), ("csproj", LANG_XML), ("vcxproj", LANG_XML),
    ("props", LANG_XML), ("targets", LANG_XML), ("config", LANG_XML), ("resx", LANG_XML),
    ("svg", LANG_XML), ("nuspec", LANG_XML), ("manifest", LANG_XML),
    ("yml", LANG_YAML), ("yaml", LANG_YAML),
    ("zig", LANG_ZIG), ("zon", LANG_ZIG),
    ("js", LANG_JS), ("mjs", LANG_JS), ("cjs", LANG_JS), ("jsx", LANG_JS),
    ("ts", LANG_TS), ("mts", LANG_TS), ("cts", LANG_TS), ("tsx", LANG_TS),
];

fn in_list(list: &[&str], s: &[u8]) -> bool {
    if let Ok(st) = std::str::from_utf8(s) {
        list.binary_search(&st).is_ok()
    } else {
        false
    }
}

fn emit(tokens: &mut Option<&mut Vec<Token>>, kind: u8, start: usize, end: usize) {
    if let Some(t) = tokens {
        if end > start && t.len() < 1024 {
            t.push(Token { start, end, kind });
        }
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn is_ident(c: u8) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

fn count_quotes(s: &[u8], i: usize) -> usize {
    let mut n = 0;
    while i + n < s.len() && s[i + n] == b'"' {
        n += 1;
    }
    n
}

fn block_comment_end(s: &[u8], mut i: usize) -> Option<usize> {
    while i + 1 < s.len() {
        if s[i] == b'*' && s[i + 1] == b'/' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

fn verbatim_end(s: &[u8], mut i: usize) -> Option<usize> {
    while i < s.len() {
        if s[i] == b'"' {
            if i + 1 < s.len() && s[i + 1] == b'"' {
                i += 2;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn raw_end(s: &[u8], mut i: usize, quotes: usize) -> Option<usize> {
    while i < s.len() {
        let n = count_quotes(s, i);
        if n >= quotes {
            return Some(i + n);
        }
        i += if n > 0 { n } else { 1 };
    }
    None
}

fn string_end(s: &[u8], mut i: usize, interpolated: bool) -> usize {
    let mut depth = 0;
    while i < s.len() {
        let c = s[i];
        if depth == 0 {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                return i + 1;
            }
            if interpolated && c == b'{' {
                if i + 1 < s.len() && s[i + 1] == b'{' {
                    i += 2;
                    continue;
                }
                depth = 1;
            }
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
        } else if c == b'"' {
            i = string_end(s, i + 1, false);
            continue;
        }
        i += 1;
    }
    s.len()
}

fn char_end(s: &[u8], mut i: usize) -> usize {
    while i < s.len() {
        if s[i] == b'\\' {
            i += 2;
            continue;
        }
        if s[i] == b'\'' {
            return i + 1;
        }
        i += 1;
    }
    s.len()
}

const ST_NORMAL: u8 = 0;
const ST_COMMENT: u8 = 1;
const ST_VERBATIM: u8 = 2;
const ST_RAW: u8 = 3;

fn cs_scan_line(s: &[u8], state: u8, mut tokens: Option<&mut Vec<Token>>) -> u8 {
    let len = s.len();
    let mut i = 0;
    let quotes = (state >> 2) as usize;
    let mut next = ST_NORMAL;

    let end = match state & 3 {
        ST_COMMENT => block_comment_end(s, 0),
        ST_VERBATIM => verbatim_end(s, 0),
        ST_RAW => raw_end(s, 0, quotes),
        _ => Some(0),
    };

    if (state & 3) != ST_NORMAL {
        let end_idx = end.unwrap_or(len);
        emit(&mut tokens, if (state & 3) == ST_COMMENT { TOK_COMMENT } else { TOK_STRING }, 0, end_idx);
        if end.is_none() {
            return state;
        }
        i = end_idx;
    }

    while i < len {
        let c = s[i];
        if c == b' ' || c == b'\t' {
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < len && s[i + 1] == b'/' {
            emit(&mut tokens, TOK_COMMENT, i, len);
            break;
        }
        if c == b'/' && i + 1 < len && s[i + 1] == b'*' {
            let end_opt = block_comment_end(s, i + 2);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_COMMENT, i, end_idx);
            if end_opt.is_none() {
                next = ST_COMMENT;
                break;
            }
            i = end_idx;
            continue;
        }
        if c == b'#' {
            let first = s[..i].iter().all(|&b| b == b' ' || b == b'\t');
            if first {
                emit(&mut tokens, TOK_PREPROCESSOR, i, len);
                break;
            }
        }

        let mut j = i;
        let mut dollars = 0;
        let mut verbatim = false;
        while j < len && (s[j] == b'$' || s[j] == b'@') {
            if s[j] == b'$' { dollars += 1; }
            if s[j] == b'@' { verbatim = true; }
            j += 1;
        }
        if j < len && s[j] == b'"' {
            let q = count_quotes(s, j);
            if q >= 3 && !verbatim {
                let end_opt = raw_end(s, j + q, q);
                let end_idx = end_opt.unwrap_or(len);
                emit(&mut tokens, TOK_STRING, i, end_idx);
                if end_opt.is_none() {
                    let clamped = if q < 63 { q as u8 } else { 63 };
                    next = ST_RAW | (clamped << 2);
                    break;
                }
                i = end_idx;
            } else if verbatim {
                let end_opt = verbatim_end(s, j + 1);
                let end_idx = end_opt.unwrap_or(len);
                emit(&mut tokens, TOK_STRING, i, end_idx);
                if end_opt.is_none() {
                    next = ST_VERBATIM;
                    break;
                }
                i = end_idx;
            } else {
                let end_idx = if q == 2 { j + 2 } else { string_end(s, j + 1, dollars > 0) };
                emit(&mut tokens, TOK_STRING, i, end_idx);
                i = end_idx;
            }
            continue;
        }
        if c == b'\'' {
            let end_idx = char_end(s, i + 1);
            emit(&mut tokens, TOK_STRING, i, end_idx);
            i = end_idx;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && i + 1 < len && s[i + 1].is_ascii_digit()) {
            j = i;
            while j < len && (is_ident(s[j]) || s[j] == b'.') {
                j += 1;
            }
            emit(&mut tokens, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if c == b'@' && i + 1 < len && is_ident_start(s[i + 1]) {
            j = i + 1;
            while j < len && is_ident(s[j]) {
                j += 1;
            }
            i = j;
            continue;
        }
        if is_ident_start(c) {
            j = i;
            while j < len && is_ident(s[j]) {
                j += 1;
            }
            if tokens.is_some() {
                let mut k = j;
                while k < len && s[k] == b' ' {
                    k += 1;
                }
                let word = &s[i..j];
                if in_list(CS_CONTROL_KEYWORDS, word) {
                    emit(&mut tokens, TOK_CONTROL, i, j);
                } else if in_list(CS_KEYWORDS, word) {
                    emit(&mut tokens, TOK_KEYWORD, i, j);
                } else if k < len && s[k] == b'(' {
                    emit(&mut tokens, TOK_METHOD, i, j);
                } else if c.is_ascii_uppercase() {
                    emit(&mut tokens, TOK_TYPE, i, j);
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }

    next
}

// C-family keyword tables
const C_KEYWORDS: &[&str] = &[
    "NULL", "_Atomic", "_Bool", "_Noreturn", "_Static_assert", "_Thread_local", "alignas", "alignof",
    "asm", "auto", "bool", "char", "char16_t", "char32_t", "char8_t", "class", "concept", "const",
    "const_cast", "consteval", "constexpr", "constinit", "decltype", "delete", "double",
    "dynamic_cast", "enum", "explicit", "export", "extern", "false", "float", "friend", "inline",
    "int", "long", "mutable", "namespace", "new", "noexcept", "nullptr", "operator", "private",
    "protected", "public", "register", "reinterpret_cast", "requires", "restrict", "short",
    "signed", "sizeof", "static", "static_assert", "static_cast", "struct", "template", "this",
    "thread_local", "true", "typedef", "typeid", "typename", "union", "unsigned", "using",
    "virtual", "void", "volatile", "wchar_t",
];

const C_CONTROL_KEYWORDS: &[&str] = &[
    "break", "case", "catch", "co_await", "co_return", "co_yield", "continue", "default", "do",
    "else", "for", "goto", "if", "return", "switch", "throw", "try", "while",
];

const JAVA_KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "byte", "char", "class", "const", "double", "enum", "extends",
    "false", "final", "float", "implements", "import", "instanceof", "int", "interface", "long",
    "native", "new", "null", "package", "permits", "private", "protected", "public", "record",
    "sealed", "short", "static", "strictfp", "super", "synchronized", "this", "throws",
    "transient", "true", "var", "void", "volatile",
];

const JAVA_CONTROL_KEYWORDS: &[&str] = &[
    "break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "if",
    "return", "switch", "throw", "try", "while", "yield",
];

const JS_KEYWORDS: &[&str] = &[
    "as", "async", "class", "const", "debugger", "delete", "export", "extends", "false", "from",
    "function", "get", "import", "in", "instanceof", "let", "new", "null", "of", "set", "static",
    "super", "this", "true", "typeof", "undefined", "var", "void", "with",
];

const JS_CONTROL_KEYWORDS: &[&str] = &[
    "await", "break", "case", "catch", "continue", "default", "do", "else", "finally", "for",
    "if", "return", "switch", "throw", "try", "while", "yield",
];

const TS_KEYWORDS: &[&str] = &[
    "abstract", "any", "bigint", "boolean", "declare", "enum", "implements", "infer", "interface",
    "is", "keyof", "module", "namespace", "never", "number", "object", "override", "private",
    "protected", "public", "readonly", "satisfies", "string", "symbol", "type", "unique",
    "unknown",
];

const GO_KEYWORDS: &[&str] = &[
    "any", "bool", "byte", "chan", "complex128", "complex64", "const", "error", "false",
    "float32", "float64", "func", "import", "int", "int16", "int32", "int64", "int8", "interface",
    "iota", "map", "nil", "package", "rune", "string", "struct", "true", "type", "uint",
    "uint16", "uint32", "uint64", "uint8", "uintptr", "var",
];

const GO_CONTROL_KEYWORDS: &[&str] = &[
    "break", "case", "continue", "default", "defer", "else", "fallthrough", "for", "go", "goto",
    "if", "range", "return", "select", "switch",
];

const KOTLIN_KEYWORDS: &[&str] = &[
    "abstract", "actual", "annotation", "as", "by", "class", "companion", "const", "constructor",
    "crossinline", "data", "enum", "expect", "external", "false", "final", "fun", "get", "import",
    "in", "infix", "init", "inline", "inner", "interface", "internal", "is", "lateinit",
    "noinline", "null", "object", "open", "operator", "out", "override", "package", "private",
    "protected", "public", "reified", "sealed", "set", "super", "suspend", "tailrec", "this",
    "true", "typealias", "val", "var", "vararg",
];

const KOTLIN_CONTROL_KEYWORDS: &[&str] = &[
    "break", "catch", "continue", "do", "else", "finally", "for", "if", "return", "throw", "try",
    "when", "while",
];

const RUST_KEYWORDS: &[&str] = &[
    "Self", "as", "async", "bool", "char", "const", "crate", "dyn", "enum", "extern", "f32",
    "f64", "false", "fn", "i128", "i16", "i32", "i64", "i8", "impl", "in", "isize", "let", "mod",
    "move", "mut", "pub", "ref", "self", "static", "str", "struct", "super", "trait", "true",
    "type", "u128", "u16", "u32", "u64", "u8", "union", "unsafe", "use", "usize", "where",
];

const RUST_CONTROL_KEYWORDS: &[&str] = &[
    "await", "break", "continue", "else", "for", "if", "loop", "match", "return", "while",
    "yield",
];

const ZIG_KEYWORDS: &[&str] = &[
    "addrspace", "align", "allowzero", "and", "anyerror", "anyframe", "anyopaque", "anytype",
    "asm", "bool", "callconv", "comptime", "comptime_int", "const", "enum", "error", "export",
    "extern", "f128", "f16", "f32", "f64", "false", "fn", "i16", "i32", "i64", "i8", "inline",
    "isize", "linksection", "noalias", "noinline", "noreturn", "null", "opaque", "or", "packed",
    "pub", "struct", "test", "threadlocal", "true", "type", "u16", "u32", "u64", "u8",
    "undefined", "union", "usingnamespace", "usize", "var", "void", "volatile",
];

const ZIG_CONTROL_KEYWORDS: &[&str] = &[
    "break", "catch", "continue", "defer", "else", "errdefer", "for", "if", "orelse", "resume",
    "return", "suspend", "switch", "try", "unreachable", "while",
];

const HAS_PREPROCESSOR: u32 = 1;
const HAS_TEMPLATE: u32 = 2;
const HAS_TEXT_BLOCK: u32 = 4;
const HAS_RAW_CPP: u32 = 8;
const HAS_AT_NAME: u32 = 16;

const CF_NORMAL: u8 = 0;
const CF_COMMENT: u8 = 1;
const CF_TEMPLATE: u8 = 2;
const CF_TEXT_BLOCK: u8 = 3;
const CF_RAW: u8 = 4;
const CF_STRING: u8 = 5;

fn c_family_flags(language: usize) -> u32 {
    match language {
        LANG_C => HAS_PREPROCESSOR | HAS_RAW_CPP,
        LANG_GO => HAS_TEMPLATE,
        LANG_JAVA | LANG_KOTLIN => HAS_TEXT_BLOCK | HAS_AT_NAME,
        LANG_JS | LANG_TS => HAS_TEMPLATE | HAS_AT_NAME,
        LANG_ZIG => HAS_AT_NAME,
        _ => 0,
    }
}

fn c_family_keyword(language: usize, s: &[u8]) -> Option<u8> {
    match language {
        LANG_C => {
            if in_list(C_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(C_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_JAVA => {
            if in_list(JAVA_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(JAVA_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_GO => {
            if in_list(GO_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(GO_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_JS => {
            if in_list(JS_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(JS_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_KOTLIN => {
            if in_list(KOTLIN_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(KOTLIN_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_RUST => {
            if in_list(RUST_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(RUST_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_TS => {
            if in_list(TS_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else if in_list(JS_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(JS_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_ZIG => {
            if in_list(ZIG_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(ZIG_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        _ => None,
    }
}

fn backtick_end(s: &[u8], mut i: usize, escapes: bool) -> Option<usize> {
    while i < s.len() {
        if s[i] == b'\\' && escapes {
            i += 1;
        } else if s[i] == b'`' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn quoted_end(s: &[u8], mut i: usize) -> Option<usize> {
    while i < s.len() {
        if s[i] == b'\\' {
            i += 1;
        } else if s[i] == b'"' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn nested_comment_end(s: &[u8], mut i: usize, depth: &mut usize) -> Option<usize> {
    while i + 1 < s.len() {
        if s[i] == b'/' && s[i + 1] == b'*' {
            *depth += 1;
            i += 2;
        } else if s[i] == b'*' && s[i + 1] == b'/' {
            i += 2;
            *depth -= 1;
            if *depth == 0 {
                return Some(i);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn raw_rust_end(s: &[u8], mut i: usize, hashes: usize) -> Option<usize> {
    while i < s.len() {
        if s[i] != b'"' {
            i += 1;
            continue;
        }
        let mut n = 0;
        while n < hashes && i + 1 + n < s.len() && s[i + 1 + n] == b'#' {
            n += 1;
        }
        if n == hashes {
            return Some(i + 1 + n);
        }
        i += 1;
    }
    None
}

fn raw_cpp_end(s: &[u8], mut i: usize, delimiter: usize) -> Option<usize> {
    while i + delimiter + 1 < s.len() {
        if s[i] == b')' && s[i + delimiter + 1] == b'"' {
            return Some(i + delimiter + 2);
        }
        i += 1;
    }
    None
}

fn is_raw_cpp_prefix(s: &[u8]) -> bool {
    let n = s.len();
    if n == 0 { return false; }
    s[n - 1] == b'R' && (n == 1 || (n == 2 && (s[0] == b'u' || s[0] == b'U' || s[0] == b'L'))
        || (n == 3 && s[0] == b'u' && s[1] == b'8'))
}

fn c_family_scan_line(language: usize, s: &[u8], state: u8, mut tokens: Option<&mut Vec<Token>>) -> u8 {
    let flags = c_family_flags(language);
    let rust = language == LANG_RUST;
    let zig = language == LANG_ZIG;
    let len = s.len();
    let mut i = 0;
    let mut depth = (state >> 3) as usize;

    let end = match state & 7 {
        CF_COMMENT => {
            if rust { nested_comment_end(s, 0, &mut depth) }
            else { block_comment_end(s, 0) }
        }
        CF_TEMPLATE => backtick_end(s, 0, language != LANG_GO),
        CF_TEXT_BLOCK => raw_end(s, 0, 3),
        CF_RAW => {
            if rust { raw_rust_end(s, 0, (state >> 3) as usize) }
            else { raw_cpp_end(s, 0, (state >> 3) as usize) }
        }
        CF_STRING => quoted_end(s, 0),
        _ => Some(0),
    };

    if (state & 7) != 0 {
        let end_idx = end.unwrap_or(len);
        emit(&mut tokens, if (state & 7) == CF_COMMENT { TOK_COMMENT } else { TOK_STRING }, 0, end_idx);
        if end.is_none() {
            let clamped_depth = if depth < 31 { depth as u8 } else { 31 };
            return (state & 7) | (clamped_depth << 3);
        }
        i = end_idx;
    }

    while i < len {
        let c = s[i];
        if c == b' ' || c == b'\t' {
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < len && s[i + 1] == b'/' {
            emit(&mut tokens, TOK_COMMENT, i, len);
            return CF_NORMAL;
        }
        if c == b'/' && i + 1 < len && s[i + 1] == b'*' && !zig {
            depth = 1;
            let end_opt = if rust { nested_comment_end(s, i + 2, &mut depth) } else { block_comment_end(s, i + 2) };
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_COMMENT, i, end_idx);
            if end_opt.is_none() {
                let clamped = if depth < 31 { depth as u8 } else { 31 };
                return CF_COMMENT | (if rust { clamped << 3 } else { 0 });
            }
            i = end_idx;
            continue;
        }
        if c == b'#' && (flags & HAS_PREPROCESSOR) != 0 && s[..i].iter().all(|&b| b == b' ' || b == b'\t') {
            emit(&mut tokens, TOK_PREPROCESSOR, i, len);
            return CF_NORMAL;
        }
        if c == b'#' && rust && i + 1 < len && (s[i + 1] == b'[' || s[i + 1] == b'!') {
            let mut brackets = 0;
            let mut j = i + 1;
            while j < len {
                if s[j] == b'[' { brackets += 1; }
                if s[j] == b']' {
                    brackets -= 1;
                    if brackets == 0 { break; }
                }
                j += 1;
            }
            let end_idx = if j < len { j + 1 } else { len };
            emit(&mut tokens, TOK_PREPROCESSOR, i, end_idx);
            i = end_idx;
            continue;
        }
        if c == b'\\' && zig && i + 1 < len && s[i + 1] == b'\\' {
            emit(&mut tokens, TOK_STRING, i, len);
            return CF_NORMAL;
        }
        if c == b'"' && rust {
            let end_opt = quoted_end(s, i + 1);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_STRING, i, end_idx);
            if end_opt.is_none() {
                return CF_STRING;
            }
            i = end_idx;
            continue;
        }
        if c == b'\'' && rust && i + 1 < len && is_ident_start(s[i + 1]) {
            let mut j = i + 1;
            while j < len && is_ident(s[j]) {
                j += 1;
            }
            if j >= len || s[j] != b'\'' {
                emit(&mut tokens, TOK_KEYWORD, i, j);
                i = j;
                continue;
            }
        }
        if c == b'"' && (flags & HAS_TEXT_BLOCK) != 0 && count_quotes(s, i) >= 3 {
            let end_opt = raw_end(s, i + 3, 3);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_STRING, i, end_idx);
            if end_opt.is_none() {
                return CF_TEXT_BLOCK;
            }
            i = end_idx;
            continue;
        }
        if c == b'"' || c == b'\'' {
            let end_idx = if c == b'"' { string_end(s, i + 1, false) } else { char_end(s, i + 1) };
            emit(&mut tokens, TOK_STRING, i, end_idx);
            i = end_idx;
            continue;
        }
        if c == b'`' && (flags & HAS_TEMPLATE) != 0 {
            let end_opt = backtick_end(s, i + 1, language != LANG_GO);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_STRING, i, end_idx);
            if end_opt.is_none() {
                return CF_TEMPLATE;
            }
            i = end_idx;
            continue;
        }
        if c == b'@' && (flags & HAS_AT_NAME) != 0 && i + 1 < len && is_ident_start(s[i + 1]) {
            let mut j = i + 1;
            while j < len && is_ident(s[j]) {
                j += 1;
            }
            emit(&mut tokens, TOK_PREPROCESSOR, i, j);
            i = j;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && i + 1 < len && s[i + 1].is_ascii_digit()) {
            let mut j = i;
            while j < len && (is_ident(s[j]) || s[j] == b'.') {
                j += 1;
            }
            emit(&mut tokens, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if !is_ident_start(c) {
            i += 1;
            continue;
        }

        let mut j = i;
        while j < len && is_ident(s[j]) {
            j += 1;
        }

        if (flags & HAS_RAW_CPP) != 0 && j < len && s[j] == b'"' && is_raw_cpp_prefix(&s[i..j]) {
            let mut open = j + 1;
            while open < len && open - j - 1 < 16 && s[open] != b'(' {
                open += 1;
            }
            if open < len && s[open] == b'(' {
                let delimiter = open - j - 1;
                let end_opt = raw_cpp_end(s, open + 1, delimiter);
                let end_idx = end_opt.unwrap_or(len);
                emit(&mut tokens, TOK_STRING, i, end_idx);
                if end_opt.is_none() {
                    return CF_RAW | ((delimiter as u8) << 3);
                }
                i = end_idx;
                continue;
            }
        }

        if rust && s[j - 1] == b'r' && (j - i == 1 || (j - i == 2 && (s[i] == b'b' || s[i] == b'c')))
            && j < len && (s[j] == b'"' || s[j] == b'#')
        {
            let mut hashes = 0;
            while j + hashes < len && hashes < 31 && s[j + hashes] == b'#' {
                hashes += 1;
            }
            if j + hashes < len && s[j + hashes] == b'"' {
                let end_opt = raw_rust_end(s, j + hashes + 1, hashes);
                let end_idx = end_opt.unwrap_or(len);
                emit(&mut tokens, TOK_STRING, i, end_idx);
                if end_opt.is_none() {
                    return CF_RAW | ((hashes as u8) << 3);
                }
                i = end_idx;
                continue;
            }
        }

        if tokens.is_some() {
            let word = &s[i..j];
            let kind = c_family_keyword(language, word);
            let mut k = j;
            while k < len && s[k] == b' ' {
                k += 1;
            }
            if let Some(k_kind) = kind {
                emit(&mut tokens, k_kind, i, j);
            } else if (k < len && s[k] == b'(') || (rust && j + 1 < len && s[j] == b'!' && s[j + 1] != b'=') {
                emit(&mut tokens, TOK_METHOD, i, j);
            } else if c.is_ascii_uppercase() {
                emit(&mut tokens, TOK_TYPE, i, j);
            }
        }

        i = j;
    }

    CF_NORMAL
}

// Script family keyword tables
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "class", "cls", "def", "del", "from",
    "global", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "self",
];

const PYTHON_CONTROL_KEYWORDS: &[&str] = &[
    "await", "break", "case", "continue", "elif", "else", "except", "finally", "for", "if",
    "match", "raise", "return", "try", "while", "with", "yield",
];

const SHELL_KEYWORDS: &[&str] = &[
    "alias", "cd", "declare", "echo", "eval", "exec", "export", "false", "function", "local",
    "printf", "read", "readonly", "set", "shift", "source", "test", "trap", "true", "unset",
];

const SHELL_CONTROL_KEYWORDS: &[&str] = &[
    "break", "case", "continue", "do", "done", "elif", "else", "esac", "exit", "fi", "for", "if",
    "in", "return", "select", "then", "until", "while",
];

const POWERSHELL_KEYWORDS: &[&str] = &[
    "begin", "class", "data", "dynamicparam", "end", "enum", "filter", "function", "hidden", "in",
    "param", "process", "static", "using", "workflow",
];

const POWERSHELL_CONTROL_KEYWORDS: &[&str] = &[
    "break", "catch", "continue", "do", "else", "elseif", "exit", "finally", "for", "foreach",
    "if", "return", "switch", "throw", "trap", "try", "until", "while",
];

const SQL_KEYWORDS: &[&str] = &[
    "add", "all", "alter", "and", "as", "asc", "begin", "between", "bigint", "bit", "boolean",
    "by", "cascade", "char", "check", "column", "commit", "constraint", "create", "cross",
    "database", "date", "datetime", "decimal", "declare", "default", "delete", "desc", "distinct",
    "drop", "exec", "exists", "float", "foreign", "from", "full", "function", "go", "grant",
    "group", "having", "identity", "if", "in", "index", "inner", "insert", "int", "integer",
    "into", "is", "join", "key", "left", "like", "limit", "merge", "not", "null", "nvarchar",
    "offset", "on", "or", "order", "outer", "over", "partition", "primary", "procedure",
    "references", "returning", "revoke", "right", "rollback", "schema", "select", "set", "table",
    "text", "top", "transaction", "trigger", "truncate", "union", "unique", "update", "using",
    "values", "varchar", "view", "where", "window", "with",
];

const SQL_CONTROL_KEYWORDS: &[&str] = &[
    "case", "else", "end", "return", "then", "when", "while",
];

const YAML_KEYWORDS: &[&str] = &[
    "false", "no", "null", "off", "on", "true", "yes",
];

const SF_NORMAL: u8 = 0;
const SF_COMMENT: u8 = 1;
const SF_TRIPLE_DOUBLE: u8 = 2;
const SF_TRIPLE_SINGLE: u8 = 3;
const SF_HERE_DOUBLE: u8 = 4;
const SF_HERE_SINGLE: u8 = 5;

fn script_keyword(language: usize, s: &[u8]) -> Option<u8> {
    match language {
        LANG_PYTHON => {
            if in_list(PYTHON_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(PYTHON_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        LANG_SHELL => {
            if in_list(SHELL_CONTROL_KEYWORDS, s) { Some(TOK_CONTROL) }
            else if in_list(SHELL_KEYWORDS, s) { Some(TOK_KEYWORD) }
            else { None }
        }
        _ => {
            let mut lower = [0u8; 32];
            if s.len() >= lower.len() { return None; }
            for (idx, &b) in s.iter().enumerate() {
                lower[idx] = b.to_ascii_lowercase();
            }
            let word = &lower[..s.len()];
            match language {
                LANG_POWERSHELL => {
                    if in_list(POWERSHELL_CONTROL_KEYWORDS, word) { Some(TOK_CONTROL) }
                    else if in_list(POWERSHELL_KEYWORDS, word) { Some(TOK_KEYWORD) }
                    else { None }
                }
                LANG_SQL => {
                    if in_list(SQL_CONTROL_KEYWORDS, word) { Some(TOK_CONTROL) }
                    else if in_list(SQL_KEYWORDS, word) { Some(TOK_KEYWORD) }
                    else { None }
                }
                LANG_YAML => {
                    if in_list(YAML_KEYWORDS, word) { Some(TOK_KEYWORD) }
                    else { None }
                }
                _ => None,
            }
        }
    }
}

fn quote_end(s: &[u8], mut i: usize, quote: u8, escape: u8) -> usize {
    while i < s.len() {
        if escape != 0 && s[i] == escape {
            i += 1;
        } else if s[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    s.len()
}

fn script_construct_end(language: usize, kind: u8, s: &[u8], mut i: usize) -> Option<usize> {
    let close: &[u8] = match kind {
        SF_COMMENT => if language == LANG_SQL { b"*/" } else { b"#>" },
        SF_TRIPLE_DOUBLE => b"\"\"\"",
        SF_TRIPLE_SINGLE => b"'''",
        SF_HERE_DOUBLE => b"\"@",
        _ => b"'@",
    };
    let n = close.len();
    while i + n <= s.len() {
        if &s[i..i + n] == close {
            if (kind == SF_HERE_DOUBLE || kind == SF_HERE_SINGLE) && !s[..i].iter().all(|&b| b == b' ' || b == b'\t') {
                i += 1;
                continue;
            }
            return Some(i + n);
        }
        i += 1;
    }
    None
}

fn is_python_prefix(c: u8) -> bool {
    matches!(c, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U')
}

fn script_scan_line(language: usize, s: &[u8], state: u8, mut tokens: Option<&mut Vec<Token>>) -> u8 {
    let python = language == LANG_PYTHON;
    let shell = language == LANG_SHELL;
    let ps = language == LANG_POWERSHELL;
    let sql = language == LANG_SQL;
    let yaml = language == LANG_YAML;
    let len = s.len();
    let mut i = 0;

    if state != 0 {
        let end_opt = script_construct_end(language, state, s, 0);
        let end_idx = end_opt.unwrap_or(len);
        emit(&mut tokens, if state == SF_COMMENT { TOK_COMMENT } else { TOK_STRING }, 0, end_idx);
        if end_opt.is_none() {
            return state;
        }
        i = end_idx;
    }

    if yaml && i == 0 {
        if len >= 3 && (&s[..3] == b"---" || &s[..3] == b"...") {
            emit(&mut tokens, TOK_PREPROCESSOR, 0, 3);
            i = 3;
        }
        let mut k = i;
        while k < len && (s[k] == b' ' || s[k] == b'\t' || (s[k] == b'-' && k + 1 < len && s[k + 1] == b' ')) {
            k += 1;
        }
        let mut j = k;
        while j < len && s[j] != b':' && s[j] != b'#' && s[j] != b'"' && s[j] != b'\'' && s[j] != b'{' && s[j] != b'[' {
            j += 1;
        }
        if j > k && j < len && s[j] == b':' && (j + 1 == len || s[j + 1] == b' ' || s[j + 1] == b'\t') {
            emit(&mut tokens, TOK_TYPE, k, j);
            i = j + 1;
        }
    }

    while i < len {
        let c = s[i];
        let after_blank = i == 0 || s[i - 1] == b' ' || s[i - 1] == b'\t';
        if c == b' ' || c == b'\t' {
            i += 1;
            continue;
        }
        if c == b'$' && (shell || ps) && i + 1 < len {
            let mut j = i + 1;
            if s[j] == b'{' {
                while j < len && s[j] != b'}' { j += 1; }
                j = if j < len { j + 1 } else { len };
            } else if is_ident(s[j]) {
                while j < len && (is_ident(s[j]) || (ps && s[j] == b':')) { j += 1; }
            } else {
                j += 1;
            }
            emit(&mut tokens, TOK_TYPE, i, j);
            i = j;
            continue;
        }
        if c == b'#' && !sql && (python || ps || after_blank) {
            emit(&mut tokens, TOK_COMMENT, i, len);
            return SF_NORMAL;
        }
        if c == b'-' && sql && i + 1 < len && s[i + 1] == b'-' {
            emit(&mut tokens, TOK_COMMENT, i, len);
            return SF_NORMAL;
        }
        if (sql && c == b'/' && i + 1 < len && s[i + 1] == b'*') || (ps && c == b'<' && i + 1 < len && s[i + 1] == b'#') {
            let end_opt = script_construct_end(language, SF_COMMENT, s, i + 2);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_COMMENT, i, end_idx);
            if end_opt.is_none() {
                return SF_COMMENT;
            }
            i = end_idx;
            continue;
        }
        if ps && c == b'@' && i + 1 < len && (s[i + 1] == b'"' || s[i + 1] == b'\'') && s[i + 2..].iter().all(|&b| b == b' ' || b == b'\t') {
            emit(&mut tokens, TOK_STRING, i, len);
            return if s[i + 1] == b'"' { SF_HERE_DOUBLE } else { SF_HERE_SINGLE };
        }

        let mut quote_at = None;
        if c == b'"' || c == b'\'' {
            quote_at = Some(i);
        } else if python && is_python_prefix(c) {
            let mut j = i;
            while j < len && j - i < 2 && is_python_prefix(s[j]) { j += 1; }
            if j < len && (s[j] == b'"' || s[j] == b'\'') {
                quote_at = Some(j);
            }
        }
        if yaml && quote_at == Some(i) && !after_blank && s[i - 1] != b':' && s[i - 1] != b'[' && s[i - 1] != b'{' && s[i - 1] != b',' {
            quote_at = None;
        }

        if let Some(qa) = quote_at {
            let quote = s[qa];
            if python && qa + 2 < len && s[qa + 1] == quote && s[qa + 2] == quote {
                let kind = if quote == b'"' { SF_TRIPLE_DOUBLE } else { SF_TRIPLE_SINGLE };
                let end_opt = script_construct_end(language, kind, s, qa + 3);
                let end_idx = end_opt.unwrap_or(len);
                emit(&mut tokens, TOK_STRING, i, end_idx);
                if end_opt.is_none() {
                    return kind;
                }
                i = end_idx;
                continue;
            }
            let escape = if sql || (quote == b'\'' && !python) { 0 } else if ps { b'`' } else { b'\\' };
            let end_idx = quote_end(s, qa + 1, quote, escape);
            emit(&mut tokens, if sql && quote == b'"' { TOK_TYPE } else { TOK_STRING }, i, end_idx);
            i = end_idx;
            continue;
        }

        if (python && c == b'@') || (yaml && after_blank && (c == b'&' || c == b'*' || c == b'!')) {
            let mut j = i + 1;
            while j < len && (is_ident(s[j]) || s[j] == b'.') { j += 1; }
            emit(&mut tokens, TOK_PREPROCESSOR, i, j);
            i = j;
            continue;
        }
        if ps && c == b'-' && after_blank && i + 1 < len && is_ident_start(s[i + 1]) {
            let mut j = i + 1;
            while j < len && is_ident(s[j]) { j += 1; }
            emit(&mut tokens, TOK_KEYWORD, i, j);
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < len && (is_ident(s[j]) || s[j] == b'.') { j += 1; }
            emit(&mut tokens, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if !is_ident_start(c) {
            i += 1;
            continue;
        }

        let mut dashed = false;
        let mut j = i;
        while j < len {
            if ps && s[j] == b'-' && j + 1 < len && is_ident_start(s[j + 1]) {
                dashed = true;
            } else if !is_ident(s[j]) {
                break;
            }
            j += 1;
        }

        if tokens.is_some() {
            let word = &s[i..j];
            let kind = if dashed { Some(TOK_METHOD) } else { script_keyword(language, word) };
            let mut k = j;
            while k < len && s[k] == b' ' { k += 1; }
            if let Some(k_kind) = kind {
                emit(&mut tokens, k_kind, i, j);
            } else if !yaml && !shell && k < len && s[k] == b'(' {
                emit(&mut tokens, TOK_METHOD, i, j);
            } else if python && c.is_ascii_uppercase() {
                emit(&mut tokens, TOK_TYPE, i, j);
            }
        }

        i = j;
    }

    SF_NORMAL
}

// Markup (HTML, XML)
const ML_TEXT: u8 = 0;
const ML_COMMENT: u8 = 1;
const ML_CDATA: u8 = 2;
const ML_TAG: u8 = 3;

fn find_text(s: &[u8], mut i: usize, close: &[u8]) -> Option<usize> {
    let n = close.len();
    while i + n <= s.len() {
        if &s[i..i + n] == close {
            return Some(i + n);
        }
        i += 1;
    }
    None
}

fn markup_scan_line(s: &[u8], mut state: u8, mut tokens: Option<&mut Vec<Token>>) -> u8 {
    let len = s.len();
    let mut i = 0;

    if state == ML_COMMENT || state == ML_CDATA {
        let close = if state == ML_COMMENT { b"-->" } else { b"]]>" };
        let end_opt = find_text(s, 0, close);
        let end_idx = end_opt.unwrap_or(len);
        emit(&mut tokens, if state == ML_COMMENT { TOK_COMMENT } else { TOK_STRING }, 0, end_idx);
        if end_opt.is_none() {
            return state;
        }
        i = end_idx;
        state = ML_TEXT;
    }

    while i < len {
        let c = s[i];
        if state == ML_TAG {
            if c == b'>' {
                state = ML_TEXT;
                i += 1;
            } else if c == b'"' || c == b'\'' {
                let end_idx = quote_end(s, i + 1, c, 0);
                emit(&mut tokens, TOK_STRING, i, end_idx);
                i = end_idx;
            } else if is_ident_start(c) {
                let mut j = i;
                while j < len && (is_ident(s[j]) || s[j] == b'-' || s[j] == b':' || s[j] == b'.') {
                    j += 1;
                }
                emit(&mut tokens, TOK_TYPE, i, j);
                i = j;
            } else {
                i += 1;
            }
            continue;
        }

        if c == b'&' {
            let mut j = i + 1;
            while j < len && j - i < 12 && s[j] != b';' && s[j] != b' ' && s[j] != b'<' {
                j += 1;
            }
            if j < len && s[j] == b';' {
                emit(&mut tokens, TOK_NUMBER, i, j + 1);
                i = j + 1;
                continue;
            }
        }

        if c != b'<' {
            i += 1;
            continue;
        }

        if i + 4 <= len && &s[i..i + 4] == b"<!--" {
            let end_opt = find_text(s, i + 4, b"-->");
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_COMMENT, i, end_idx);
            if end_opt.is_none() {
                return ML_COMMENT;
            }
            i = end_idx;
            continue;
        }

        if i + 9 <= len && &s[i..i + 9] == b"<![CDATA[" {
            let end_opt = find_text(s, i + 9, b"]]>");
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_STRING, i, end_idx);
            if end_opt.is_none() {
                return ML_CDATA;
            }
            i = end_idx;
            continue;
        }

        let mut j = i + 1;
        let declaration = j < len && (s[j] == b'!' || s[j] == b'?');
        if j < len && (s[j] == b'/' || declaration) {
            j += 1;
        }
        if j >= len || !is_ident_start(s[j]) {
            i += 1;
            continue;
        }
        let name = j;
        while j < len && (is_ident(s[j]) || s[j] == b'-' || s[j] == b':' || s[j] == b'.') {
            j += 1;
        }
        emit(&mut tokens, if declaration { TOK_PREPROCESSOR } else { TOK_KEYWORD }, if declaration { i } else { name }, j);
        state = ML_TAG;
        i = j;
    }

    state
}

// CSS
fn css_scan_line(s: &[u8], state: u8, mut tokens: Option<&mut Vec<Token>>) -> u8 {
    let mut depth = (state >> 1) as usize;
    let len = s.len();
    let mut i = 0;

    if (state & 1) != 0 {
        let end_opt = block_comment_end(s, 0);
        let end_idx = end_opt.unwrap_or(len);
        emit(&mut tokens, TOK_COMMENT, 0, end_idx);
        if end_opt.is_none() {
            return state;
        }
        i = end_idx;
    }

    while i < len {
        let c = s[i];
        if c == b'/' && i + 1 < len && s[i + 1] == b'*' {
            let end_opt = block_comment_end(s, i + 2);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_COMMENT, i, end_idx);
            if end_opt.is_none() {
                return 1 | ((depth as u8) << 1);
            }
            i = end_idx;
            continue;
        }
        if c == b'/' && i + 1 < len && s[i + 1] == b'/' {
            emit(&mut tokens, TOK_COMMENT, i, len);
            break;
        }
        if c == b'{' && depth < 127 {
            depth += 1;
            i += 1;
            continue;
        }
        if c == b'}' && depth > 0 {
            depth -= 1;
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            let end_idx = quote_end(s, i + 1, c, b'\\');
            emit(&mut tokens, TOK_STRING, i, end_idx);
            i = end_idx;
            continue;
        }
        if c == b'@' || (c == b'!' && depth > 0) {
            let mut j = i + 1;
            while j < len && (is_ident(s[j]) || s[j] == b'-') { j += 1; }
            emit(&mut tokens, if c == b'@' { TOK_PREPROCESSOR } else { TOK_KEYWORD }, i, j);
            i = j;
            continue;
        }
        if is_ident_start(c) || c == b'.' || c == b'#' || c == b':' || c == b'*' {
            let mut j = i;
            while depth > 0 && j < len && s[j] != b'{' && s[j] != b';' && s[j] != b'}' {
                j += 1;
            }
            if depth == 0 || (j < len && s[j] == b'{') {
                let mut k = i + 1;
                while k < len && (is_ident(s[k]) || s[k] == b'-' || s[k] == b'.' || s[k] == b'#' || s[k] == b':' || s[k] == b'*') {
                    k += 1;
                }
                emit(&mut tokens, TOK_KEYWORD, i, k);
                i = k;
                continue;
            }
        }
        if depth > 0 && (c.is_ascii_digit() || c == b'#' || (c == b'.' && i + 1 < len && s[i + 1].is_ascii_digit())) {
            let mut j = i + 1;
            while j < len && (is_ident(s[j]) || s[j] == b'.' || s[j] == b'%') { j += 1; }
            emit(&mut tokens, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if depth > 0 && (is_ident_start(c) || c == b'-') {
            let mut j = i + 1;
            while j < len && (is_ident(s[j]) || s[j] == b'-') { j += 1; }
            let mut k = j;
            while k < len && s[k] == b' ' { k += 1; }
            if k < len && s[k] == b':' {
                emit(&mut tokens, TOK_TYPE, i, j);
            } else if k < len && s[k] == b'(' {
                emit(&mut tokens, TOK_METHOD, i, j);
            }
            i = j;
            continue;
        }
        i += 1;
    }

    (depth as u8) << 1
}

// JSON
fn json_scan_line(s: &[u8], state: u8, mut tokens: Option<&mut Vec<Token>>) -> u8 {
    let len = s.len();
    let mut i = 0;

    if state != 0 {
        let end_opt = block_comment_end(s, 0);
        let end_idx = end_opt.unwrap_or(len);
        emit(&mut tokens, TOK_COMMENT, 0, end_idx);
        if end_opt.is_none() {
            return 1;
        }
        i = end_idx;
    }

    while i < len {
        let c = s[i];
        if c == b'/' && i + 1 < len && s[i + 1] == b'/' {
            emit(&mut tokens, TOK_COMMENT, i, len);
            return 0;
        }
        if c == b'/' && i + 1 < len && s[i + 1] == b'*' {
            let end_opt = block_comment_end(s, i + 2);
            let end_idx = end_opt.unwrap_or(len);
            emit(&mut tokens, TOK_COMMENT, i, end_idx);
            if end_opt.is_none() {
                return 1;
            }
            i = end_idx;
            continue;
        }
        if c == b'"' {
            let end_idx = quote_end(s, i + 1, b'"', b'\\');
            let mut k = end_idx;
            while k < len && (s[k] == b' ' || s[k] == b'\t') { k += 1; }
            emit(&mut tokens, if k < len && s[k] == b':' { TOK_TYPE } else { TOK_STRING }, i, end_idx);
            i = end_idx;
            continue;
        }
        if c.is_ascii_digit() || (c == b'-' && i + 1 < len && s[i + 1].is_ascii_digit()) {
            let mut j = i + 1;
            while j < len && (is_ident(s[j]) || s[j] == b'.' || s[j] == b'+' || s[j] == b'-') {
                j += 1;
            }
            emit(&mut tokens, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if is_ident_start(c) {
            let mut j = i;
            while j < len && is_ident(s[j]) { j += 1; }
            let word = &s[i..j];
            if word == b"true" || word == b"false" || word == b"null" {
                emit(&mut tokens, TOK_KEYWORD, i, j);
            }
            i = j;
            continue;
        }
        i += 1;
    }

    0
}

pub fn syntax_scan_line(
    language: usize,
    s: &[u8],
    state: u8,
    tokens: &mut Option<&mut Vec<Token>>,
) -> u8 {
    let tok_opt = tokens.as_deref_mut();
    match language {
        LANG_CS => cs_scan_line(s, state, tok_opt),
        LANG_C | LANG_GO | LANG_JAVA | LANG_JS | LANG_KOTLIN | LANG_RUST | LANG_TS | LANG_ZIG => {
            c_family_scan_line(language, s, state, tok_opt)
        }
        LANG_POWERSHELL | LANG_PYTHON | LANG_SHELL | LANG_SQL | LANG_YAML => {
            script_scan_line(language, s, state, tok_opt)
        }
        LANG_HTML | LANG_XML => markup_scan_line(s, state, tok_opt),
        LANG_CSS => css_scan_line(s, state, tok_opt),
        LANG_JSON => json_scan_line(s, state, tok_opt),
        _ => 0,
    }
}

pub fn syntax_language_for_path(path: &str) -> usize {
    let dot = path.rfind('.').map(|i| &path[i + 1..]);
    let ext = match dot {
        Some(e) => {
            if e.contains('/') || e.contains('\\') {
                return LANG_PLAIN;
            }
            e.to_ascii_lowercase()
        }
        None => return LANG_PLAIN,
    };

    for &(e, lang) in EXTENSIONS {
        if ext == e {
            return lang;
        }
    }
    LANG_PLAIN
}
