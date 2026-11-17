#include "app.h"

// Lexer state carried from one line to the next: the construct kind in the low 2 bits, and for a
// raw string the number of quotes that closes it in the rest.
enum { ST_NORMAL, ST_COMMENT, ST_VERBATIM, ST_RAW };

// Keyword lists are sorted (binary search) and fixed width, never `const char *[]`: with ASLR the
// loader patches every pointer in .rdata, which makes those pages private memory.
static const char cs_keywords[][11] = {
    "abstract", "add", "alias", "and", "as", "async", "await", "base", "bool", "by", "byte", "char", "checked",
    "class", "const", "decimal", "delegate", "descending", "double", "dynamic", "enum", "equals", "event",
    "explicit", "extern", "false", "file", "fixed", "float", "from", "get", "global", "group", "implicit", "in",
    "init", "int", "interface", "internal", "into", "is", "join", "let", "lock", "long", "managed", "nameof",
    "namespace", "new", "nint", "not", "notnull", "nuint", "null", "object", "on", "operator", "or", "orderby",
    "out", "override", "params", "partial", "private", "protected", "public", "readonly", "record", "ref",
    "remove", "required", "sbyte", "scoped", "sealed", "select", "set", "short", "sizeof", "stackalloc",
    "static", "string", "struct", "this", "true", "typeof", "uint", "ulong", "unchecked", "unmanaged", "unsafe",
    "ushort", "using", "value", "var", "virtual", "void", "volatile", "when", "where", "with",
};

static const char cs_control_keywords[][9] = {
    "break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "foreach", "goto", "if",
    "return", "switch", "throw", "try", "while", "yield",
};

// One row per extension: Auto picks the language from the file name.
static const struct {
    char ext[9];
    unsigned char language;
} extensions[] = {
    {"c", LANG_C}, {"h", LANG_C}, {"cc", LANG_C}, {"cpp", LANG_C}, {"cxx", LANG_C}, {"hpp", LANG_C},
    {"hh", LANG_C}, {"hxx", LANG_C}, {"inl", LANG_C},
    {"cs", LANG_CS}, {"csx", LANG_CS},
    {"css", LANG_CSS}, {"scss", LANG_CSS}, {"less", LANG_CSS},
    {"go", LANG_GO},
    {"html", LANG_HTML}, {"htm", LANG_HTML},
    {"java", LANG_JAVA},
    {"json", LANG_JSON}, {"jsonc", LANG_JSON},
    {"kt", LANG_KOTLIN}, {"kts", LANG_KOTLIN},
    {"ps1", LANG_POWERSHELL}, {"psm1", LANG_POWERSHELL}, {"psd1", LANG_POWERSHELL},
    {"py", LANG_PYTHON}, {"pyw", LANG_PYTHON}, {"pyi", LANG_PYTHON},
    {"rs", LANG_RUST},
    {"sh", LANG_SHELL}, {"bash", LANG_SHELL}, {"zsh", LANG_SHELL},
    {"sql", LANG_SQL},
    {"xml", LANG_XML}, {"xaml", LANG_XML}, {"csproj", LANG_XML}, {"vcxproj", LANG_XML}, {"props", LANG_XML},
    {"targets", LANG_XML}, {"config", LANG_XML}, {"resx", LANG_XML}, {"svg", LANG_XML}, {"nuspec", LANG_XML},
    {"manifest", LANG_XML},
    {"yml", LANG_YAML}, {"yaml", LANG_YAML},
    {"zig", LANG_ZIG}, {"zon", LANG_ZIG},
    {"js", LANG_JS}, {"mjs", LANG_JS}, {"cjs", LANG_JS}, {"jsx", LANG_JS},
    {"ts", LANG_TS}, {"mts", LANG_TS}, {"cts", LANG_TS}, {"tsx", LANG_TS},
};

typedef struct {
    Token *tokens;
    int cap, count;
} TokenOut;

static void emit(TokenOut *out, int kind, int start, int end)
{
    if (out->tokens && out->count < out->cap && end > start)
        out->tokens[out->count++] = (Token){start, end, kind};
}

static int compare_word(const char *word, const char *s, int len)
{
    for (int i = 0; i < len; i++) {
        if (!word[i])
            return -1;
        if (word[i] != s[i])
            return (unsigned char)word[i] - (unsigned char)s[i];
    }
    return word[len] ? 1 : 0;
}

// `list` is a fixed-width keyword array; IN_LIST passes its row width and row count.
static int in_list(const char *list, int width, int n, const char *s, int len)
{
    int lo = 0, hi = n - 1;
    while (lo <= hi) {
        int mid = (lo + hi) / 2;
        int cmp = compare_word(list + mid * width, s, len);
        if (cmp == 0)
            return 1;
        if (cmp < 0)
            lo = mid + 1;
        else
            hi = mid - 1;
    }
    return 0;
}

#define IN_LIST(list, s, len) in_list((list)[0], sizeof(list)[0], sizeof(list) / sizeof(list)[0], s, len)

static int is_ident_start(unsigned char c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || c >= 0x80;
}

static int is_ident(unsigned char c)
{
    return is_ident_start(c) || (c >= '0' && c <= '9');
}

static int is_digit(unsigned char c)
{
    return c >= '0' && c <= '9';
}

static int count_quotes(const char *s, int len, int i)
{
    int n = 0;
    while (i + n < len && s[i + n] == '"')
        n++;
    return n;
}

// Each *_end returns the offset just past the construct, or -1 when it continues on the next line.

static int block_comment_end(const char *s, int len, int i)
{
    for (; i + 1 < len; i++) {
        if (s[i] == '*' && s[i + 1] == '/')
            return i + 2;
    }
    return -1;
}

static int verbatim_end(const char *s, int len, int i)
{
    while (i < len) {
        if (s[i] == '"') {
            if (i + 1 < len && s[i + 1] == '"') {  // "" is an escaped quote
                i += 2;
                continue;
            }
            return i + 1;
        }
        i++;
    }
    return -1;
}

static int raw_end(const char *s, int len, int i, int quotes)
{
    while (i < len) {
        int n = count_quotes(s, len, i);
        if (n >= quotes)
            return i + n;
        i += n ? n : 1;
    }
    return -1;
}

// A regular or interpolated string ends on its line; an unterminated one runs to the end.
static int string_end(const char *s, int len, int i, int interpolated)
{
    int depth = 0;  // inside an interpolation hole, which may hold its own strings
    while (i < len) {
        char c = s[i];
        if (depth == 0) {
            if (c == '\\') {
                i += 2;
                continue;
            }
            if (c == '"')
                return i + 1;
            if (interpolated && c == '{') {
                if (i + 1 < len && s[i + 1] == '{') {
                    i += 2;
                    continue;
                }
                depth = 1;
            }
        } else if (c == '{') {
            depth++;
        } else if (c == '}') {
            depth--;
        } else if (c == '"') {
            i = string_end(s, len, i + 1, 0);
            continue;
        }
        i++;
    }
    return len;
}

static int char_end(const char *s, int len, int i)
{
    while (i < len) {
        if (s[i] == '\\') {
            i += 2;
            continue;
        }
        if (s[i] == '\'')
            return i + 1;
        i++;
    }
    return len;
}

static unsigned char cs_scan_line(const char *s, int len, unsigned char state, Token *tokens, int cap, int *count)
{
    TokenOut out = {tokens, cap, 0};
    int i = 0, end = 0;
    int quotes = state >> 2;
    unsigned char next = ST_NORMAL;

    // A construct that started on an earlier line.
    switch (state & 3) {
    case ST_COMMENT: end = block_comment_end(s, len, 0); break;
    case ST_VERBATIM: end = verbatim_end(s, len, 0); break;
    case ST_RAW: end = raw_end(s, len, 0, quotes); break;
    }
    if ((state & 3) != ST_NORMAL) {
        emit(&out, (state & 3) == ST_COMMENT ? TOK_COMMENT : TOK_STRING, 0, end < 0 ? len : end);
        if (end < 0) {
            next = state;
            goto done;
        }
        i = end;
    }

    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        if (c == ' ' || c == '\t') {
            i++;
            continue;
        }
        if (c == '/' && i + 1 < len && s[i + 1] == '/') {
            emit(&out, TOK_COMMENT, i, len);
            break;
        }
        if (c == '/' && i + 1 < len && s[i + 1] == '*') {
            end = block_comment_end(s, len, i + 2);
            emit(&out, TOK_COMMENT, i, end < 0 ? len : end);
            if (end < 0) {
                next = ST_COMMENT;
                break;
            }
            i = end;
            continue;
        }
        if (c == '#') {
            int first = 1;
            for (int k = 0; k < i; k++)
                first &= s[k] == ' ' || s[k] == '\t';
            if (first) {
                emit(&out, TOK_PREPROCESSOR, i, len);
                break;
            }
        }

        // Strings: "..", @"..", $"..", $@"..", @$"..", """raw""", $$"""raw"""
        int j = i, dollars = 0, verbatim = 0;
        while (j < len && (s[j] == '$' || s[j] == '@')) {
            dollars += s[j] == '$';
            verbatim |= s[j] == '@';
            j++;
        }
        if (j < len && s[j] == '"') {
            int q = count_quotes(s, len, j);
            if (q >= 3 && !verbatim) {  // raw strings take no '@': @"""" is a verbatim string of one '"'
                end = raw_end(s, len, j + q, q);
                emit(&out, TOK_STRING, i, end < 0 ? len : end);
                if (end < 0) {
                    next = (unsigned char)(ST_RAW | (q < 63 ? q : 63) << 2);
                    break;
                }
            } else if (verbatim) {
                end = verbatim_end(s, len, j + 1);
                emit(&out, TOK_STRING, i, end < 0 ? len : end);
                if (end < 0) {
                    next = ST_VERBATIM;
                    break;
                }
            } else {
                end = q == 2 ? j + 2 : string_end(s, len, j + 1, dollars > 0);
                emit(&out, TOK_STRING, i, end);
            }
            i = end;
            continue;
        }
        if (c == '\'') {
            end = char_end(s, len, i + 1);
            emit(&out, TOK_STRING, i, end);
            i = end;
            continue;
        }
        if (is_digit(c) || (c == '.' && i + 1 < len && is_digit((unsigned char)s[i + 1]))) {
            j = i;
            while (j < len && (is_ident((unsigned char)s[j]) || s[j] == '.'))
                j++;
            emit(&out, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if (c == '@' && i + 1 < len && is_ident_start((unsigned char)s[i + 1])) {
            j = i + 1;  // @name: an identifier, even when the name is a keyword
            while (j < len && is_ident((unsigned char)s[j]))
                j++;
            i = j;
            continue;
        }
        if (is_ident_start(c)) {
            j = i;
            while (j < len && is_ident((unsigned char)s[j]))
                j++;
            if (!out.tokens) {  // the whole-file state pass needs no keyword lookups
                i = j;
                continue;
            }
            int k = j;
            while (k < len && s[k] == ' ')
                k++;
            if (IN_LIST(cs_control_keywords, s + i, j - i))
                emit(&out, TOK_CONTROL, i, j);
            else if (IN_LIST(cs_keywords, s + i, j - i))
                emit(&out, TOK_KEYWORD, i, j);
            else if (k < len && s[k] == '(')
                emit(&out, TOK_METHOD, i, j);
            else if (c >= 'A' && c <= 'Z')  // no semantic model: PascalCase names are taken as types
                emit(&out, TOK_TYPE, i, j);
            i = j;
            continue;
        }
        i++;
    }

done:
    if (count)
        *count = out.count;
    return next;
}

// ---- C family: C/C++, Go, Java, JavaScript, Kotlin, Rust, TypeScript, Zig -----------------------

static int only_blanks_before(const char *s, int i)
{
    while (i > 0 && (s[i - 1] == ' ' || s[i - 1] == '\t'))
        i--;
    return i == 0;
}

static int only_blanks_after(const char *s, int len, int i)
{
    while (i < len && (s[i] == ' ' || s[i] == '\t'))
        i++;
    return i == len;
}

static const char c_keywords[][17] = {
    "NULL", "_Atomic", "_Bool", "_Noreturn", "_Static_assert", "_Thread_local", "alignas", "alignof", "asm", "auto",
    "bool", "char", "char16_t", "char32_t", "char8_t", "class", "concept", "const", "const_cast", "consteval",
    "constexpr", "constinit", "decltype", "delete", "double", "dynamic_cast", "enum", "explicit", "export",
    "extern", "false", "float", "friend", "inline", "int", "long", "mutable", "namespace", "new", "noexcept",
    "nullptr", "operator", "private", "protected", "public", "register", "reinterpret_cast", "requires", "restrict",
    "short", "signed", "sizeof", "static", "static_assert", "static_cast", "struct", "template", "this",
    "thread_local", "true", "typedef", "typeid", "typename", "union", "unsigned", "using", "virtual", "void",
    "volatile", "wchar_t",
};

static const char c_control_keywords[][10] = {
    "break", "case", "catch", "co_await", "co_return", "co_yield", "continue", "default", "do", "else", "for",
    "goto", "if", "return", "switch", "throw", "try", "while",
};

static const char java_keywords[][13] = {
    "abstract", "assert", "boolean", "byte", "char", "class", "const", "double", "enum", "extends", "false",
    "final", "float", "implements", "import", "instanceof", "int", "interface", "long", "native", "new", "null",
    "package", "permits", "private", "protected", "public", "record", "sealed", "short", "static", "strictfp",
    "super", "synchronized", "this", "throws", "transient", "true", "var", "void", "volatile",
};

static const char java_control_keywords[][9] = {
    "break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "if", "return", "switch",
    "throw", "try", "while", "yield",
};

static const char js_keywords[][11] = {
    "as", "async", "class", "const", "debugger", "delete", "export", "extends", "false", "from", "function", "get",
    "import", "in", "instanceof", "let", "new", "null", "of", "set", "static", "super", "this", "true", "typeof",
    "undefined", "var", "void", "with",
};

static const char js_control_keywords[][9] = {
    "await", "break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "if", "return",
    "switch", "throw", "try", "while", "yield",
};

static const char ts_keywords[][11] = {
    "abstract", "any", "bigint", "boolean", "declare", "enum", "implements", "infer", "interface", "is", "keyof",
    "module", "namespace", "never", "number", "object", "override", "private", "protected", "public", "readonly",
    "satisfies", "string", "symbol", "type", "unique", "unknown",
};

static const char go_keywords[][11] = {
    "any", "bool", "byte", "chan", "complex128", "complex64", "const", "error", "false", "float32", "float64",
    "func", "import", "int", "int16", "int32", "int64", "int8", "interface", "iota", "map", "nil", "package",
    "rune", "string", "struct", "true", "type", "uint", "uint16", "uint32", "uint64", "uint8", "uintptr", "var",
};

static const char go_control_keywords[][12] = {
    "break", "case", "continue", "default", "defer", "else", "fallthrough", "for", "go", "goto", "if", "range",
    "return", "select", "switch",
};

static const char kotlin_keywords[][12] = {
    "abstract", "actual", "annotation", "as", "by", "class", "companion", "const", "constructor", "crossinline",
    "data", "enum", "expect", "external", "false", "final", "fun", "get", "import", "in", "infix", "init", "inline",
    "inner", "interface", "internal", "is", "lateinit", "noinline", "null", "object", "open", "operator", "out",
    "override", "package", "private", "protected", "public", "reified", "sealed", "set", "super", "suspend",
    "tailrec", "this", "true", "typealias", "val", "var", "vararg",
};

static const char kotlin_control_keywords[][9] = {
    "break", "catch", "continue", "do", "else", "finally", "for", "if", "return", "throw", "try", "when", "while",
};

static const char rust_keywords[][7] = {
    "Self", "as", "async", "bool", "char", "const", "crate", "dyn", "enum", "extern", "f32", "f64", "false", "fn",
    "i128", "i16", "i32", "i64", "i8", "impl", "in", "isize", "let", "mod", "move", "mut", "pub", "ref", "self",
    "static", "str", "struct", "super", "trait", "true", "type", "u128", "u16", "u32", "u64", "u8", "union",
    "unsafe", "use", "usize", "where",
};

static const char rust_control_keywords[][9] = {
    "await", "break", "continue", "else", "for", "if", "loop", "match", "return", "while", "yield",
};

static const char zig_keywords[][15] = {
    "addrspace", "align", "allowzero", "and", "anyerror", "anyframe", "anyopaque", "anytype", "asm", "bool",
    "callconv", "comptime", "comptime_int", "const", "enum", "error", "export", "extern", "f128", "f16", "f32",
    "f64", "false", "fn", "i16", "i32", "i64", "i8", "inline", "isize", "linksection", "noalias", "noinline",
    "noreturn", "null", "opaque", "or", "packed", "pub", "struct", "test", "threadlocal", "true", "type", "u16",
    "u32", "u64", "u8", "undefined", "union", "usingnamespace", "usize", "var", "void", "volatile",
};

static const char zig_control_keywords[][12] = {
    "break", "catch", "continue", "defer", "else", "errdefer", "for", "if", "orelse", "resume", "return", "suspend",
    "switch", "try", "unreachable", "while",
};

// What a C-family language has beyond // and /* */ comments, "strings" and 'chars'. Rust and Zig,
// which differ more, are checked by name in the scanner.
enum {
    HAS_PREPROCESSOR = 1,  // # lines (C/C++)
    HAS_TEMPLATE = 2,      // `strings` that may span lines (JS/TS templates, Go raw strings)
    HAS_TEXT_BLOCK = 4,    // """text blocks""" (Java, Kotlin)
    HAS_RAW_CPP = 8,       // R"delim(raw)delim" (C++)
    HAS_AT_NAME = 16,      // @annotations, @decorators, Zig @builtins
};

// Lexer state: the construct in the low 3 bits, and above them its parameter: the delimiter length
// of a C++ raw string, the # count of a Rust raw string, the depth of nested Rust comments.
enum { CF_NORMAL, CF_COMMENT, CF_TEMPLATE, CF_TEXT_BLOCK, CF_RAW, CF_STRING };

static int c_family_flags(int language)
{
    switch (language) {
    case LANG_C: return HAS_PREPROCESSOR | HAS_RAW_CPP;
    case LANG_GO: return HAS_TEMPLATE;
    case LANG_JAVA:
    case LANG_KOTLIN: return HAS_TEXT_BLOCK | HAS_AT_NAME;
    case LANG_JS:
    case LANG_TS: return HAS_TEMPLATE | HAS_AT_NAME;
    case LANG_ZIG: return HAS_AT_NAME;
    }
    return 0;
}

#define KEYWORD_KIND(control, keywords, s, len) \
    (IN_LIST(control, s, len) ? TOK_CONTROL : IN_LIST(keywords, s, len) ? TOK_KEYWORD : -1)

// TOK_CONTROL, TOK_KEYWORD, or -1 for any other name.
static int c_family_keyword(int language, const char *s, int len)
{
    switch (language) {
    case LANG_C: return KEYWORD_KIND(c_control_keywords, c_keywords, s, len);
    case LANG_JAVA: return KEYWORD_KIND(java_control_keywords, java_keywords, s, len);
    case LANG_GO: return KEYWORD_KIND(go_control_keywords, go_keywords, s, len);
    case LANG_JS: return KEYWORD_KIND(js_control_keywords, js_keywords, s, len);
    case LANG_KOTLIN: return KEYWORD_KIND(kotlin_control_keywords, kotlin_keywords, s, len);
    case LANG_RUST: return KEYWORD_KIND(rust_control_keywords, rust_keywords, s, len);
    case LANG_TS:
        if (IN_LIST(ts_keywords, s, len))
            return TOK_KEYWORD;
        return KEYWORD_KIND(js_control_keywords, js_keywords, s, len);
    case LANG_ZIG: return KEYWORD_KIND(zig_control_keywords, zig_keywords, s, len);
    }
    return -1;
}

// Go raw strings have no escapes; JS/TS template strings do.
static int backtick_end(const char *s, int len, int i, int escapes)
{
    for (; i < len; i++) {
        if (s[i] == '\\' && escapes)
            i++;
        else if (s[i] == '`')
            return i + 1;
    }
    return -1;
}

// A "string" that may span lines (Rust), from just after the opening quote.
static int quoted_end(const char *s, int len, int i)
{
    for (; i < len; i++) {
        if (s[i] == '\\')
            i++;
        else if (s[i] == '"')
            return i + 1;
    }
    return -1;
}

// Rust block comments nest: `depth` is how many are open, updated as the line is scanned.
static int nested_comment_end(const char *s, int len, int i, int *depth)
{
    while (i + 1 < len) {
        if (s[i] == '/' && s[i + 1] == '*') {
            ++*depth;
            i += 2;
        } else if (s[i] == '*' && s[i + 1] == '/') {
            i += 2;
            if (--*depth == 0)
                return i;
        } else {
            i++;
        }
    }
    return -1;
}

// "### closes a Rust raw string opened with r###".
static int raw_rust_end(const char *s, int len, int i, int hashes)
{
    for (; i < len; i++) {
        if (s[i] != '"')
            continue;
        int n = 0;
        while (n < hashes && i + 1 + n < len && s[i + 1 + n] == '#')
            n++;
        if (n == hashes)
            return i + 1 + n;
    }
    return -1;
}

// )delim" closes a C++ raw string; only the delimiter's length is kept in the lexer state.
static int raw_cpp_end(const char *s, int len, int i, int delimiter)
{
    for (; i + delimiter + 1 < len; i++) {
        if (s[i] == ')' && s[i + delimiter + 1] == '"')
            return i + delimiter + 2;
    }
    return -1;
}

static int is_raw_cpp_prefix(const char *s, int n)
{
    return s[n - 1] == 'R' && (n == 1 || (n == 2 && (s[0] == 'u' || s[0] == 'U' || s[0] == 'L')) ||
                               (n == 3 && s[0] == 'u' && s[1] == '8'));
}

static unsigned char c_family_scan_line(int language, const char *s, int len, unsigned char state, TokenOut *out)
{
    int flags = c_family_flags(language);
    int rust = language == LANG_RUST, zig = language == LANG_ZIG;
    int i = 0, end = 0, depth = state >> 3;

    // A construct that started on an earlier line.
    switch (state & 7) {
    case CF_COMMENT: end = rust ? nested_comment_end(s, len, 0, &depth) : block_comment_end(s, len, 0); break;
    case CF_TEMPLATE: end = backtick_end(s, len, 0, language != LANG_GO); break;
    case CF_TEXT_BLOCK: end = raw_end(s, len, 0, 3); break;
    case CF_RAW: end = rust ? raw_rust_end(s, len, 0, state >> 3) : raw_cpp_end(s, len, 0, state >> 3); break;
    case CF_STRING: end = quoted_end(s, len, 0); break;
    }
    if (state & 7) {
        emit(out, (state & 7) == CF_COMMENT ? TOK_COMMENT : TOK_STRING, 0, end < 0 ? len : end);
        if (end < 0)
            return (unsigned char)((state & 7) | (depth < 31 ? depth : 31) << 3);
        i = end;
    }

    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        int j;
        if (c == ' ' || c == '\t') {
            i++;
            continue;
        }
        if (c == '/' && i + 1 < len && s[i + 1] == '/') {
            emit(out, TOK_COMMENT, i, len);
            return CF_NORMAL;
        }
        if (c == '/' && i + 1 < len && s[i + 1] == '*' && !zig) {  // Zig has only line comments
            depth = 1;
            end = rust ? nested_comment_end(s, len, i + 2, &depth) : block_comment_end(s, len, i + 2);
            emit(out, TOK_COMMENT, i, end < 0 ? len : end);
            if (end < 0)
                return (unsigned char)(CF_COMMENT | (rust ? (depth < 31 ? depth : 31) << 3 : 0));
            i = end;
            continue;
        }
        if (c == '#' && (flags & HAS_PREPROCESSOR) && only_blanks_before(s, i)) {
            emit(out, TOK_PREPROCESSOR, i, len);
            return CF_NORMAL;
        }
        if (c == '#' && rust && i + 1 < len && (s[i + 1] == '[' || s[i + 1] == '!')) {  // #[attribute]
            int brackets = 0;
            for (j = i + 1; j < len; j++) {
                brackets += s[j] == '[';
                if (s[j] == ']' && --brackets == 0)
                    break;
            }
            j = j < len ? j + 1 : len;
            emit(out, TOK_PREPROCESSOR, i, j);
            i = j;
            continue;
        }
        if (c == '\\' && zig && i + 1 < len && s[i + 1] == '\\') {  // \\ line string, to the end of the line
            emit(out, TOK_STRING, i, len);
            return CF_NORMAL;
        }
        if (c == '"' && rust) {
            end = quoted_end(s, len, i + 1);
            emit(out, TOK_STRING, i, end < 0 ? len : end);
            if (end < 0)
                return CF_STRING;
            i = end;
            continue;
        }
        if (c == '\'' && rust && i + 1 < len && is_ident_start((unsigned char)s[i + 1])) {
            for (j = i + 1; j < len && is_ident((unsigned char)s[j]); j++) {
            }
            if (j >= len || s[j] != '\'') {  // 'a with no closing quote is a lifetime, not a char
                emit(out, TOK_KEYWORD, i, j);
                i = j;
                continue;
            }
        }
        if (c == '"' && (flags & HAS_TEXT_BLOCK) && count_quotes(s, len, i) >= 3) {
            end = raw_end(s, len, i + 3, 3);
            emit(out, TOK_STRING, i, end < 0 ? len : end);
            if (end < 0)
                return CF_TEXT_BLOCK;
            i = end;
            continue;
        }
        if (c == '"' || c == '\'') {
            end = c == '"' ? string_end(s, len, i + 1, 0) : char_end(s, len, i + 1);
            emit(out, TOK_STRING, i, end);
            i = end;
            continue;
        }
        if (c == '`' && (flags & HAS_TEMPLATE)) {
            end = backtick_end(s, len, i + 1, language != LANG_GO);
            emit(out, TOK_STRING, i, end < 0 ? len : end);
            if (end < 0)
                return CF_TEMPLATE;
            i = end;
            continue;
        }
        if (c == '@' && (flags & HAS_AT_NAME) && i + 1 < len && is_ident_start((unsigned char)s[i + 1])) {
            for (j = i + 1; j < len && is_ident((unsigned char)s[j]); j++) {
            }
            emit(out, TOK_PREPROCESSOR, i, j);
            i = j;
            continue;
        }
        if (is_digit(c) || (c == '.' && i + 1 < len && is_digit((unsigned char)s[i + 1]))) {
            for (j = i; j < len && (is_ident((unsigned char)s[j]) || s[j] == '.'); j++) {
            }
            emit(out, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if (!is_ident_start(c)) {
            i++;
            continue;
        }

        for (j = i; j < len && is_ident((unsigned char)s[j]); j++) {
        }
        if ((flags & HAS_RAW_CPP) && j < len && s[j] == '"' && is_raw_cpp_prefix(s + i, j - i)) {
            int open = j + 1;
            while (open < len && open - j - 1 < 16 && s[open] != '(')  // the delimiter is at most 16 chars
                open++;
            if (open < len && s[open] == '(') {
                int delimiter = open - j - 1;
                end = raw_cpp_end(s, len, open + 1, delimiter);
                emit(out, TOK_STRING, i, end < 0 ? len : end);
                if (end < 0)
                    return (unsigned char)(CF_RAW | delimiter << 3);
                i = end;
                continue;
            }
        }
        if (rust && s[j - 1] == 'r' && (j - i == 1 || (j - i == 2 && (s[i] == 'b' || s[i] == 'c'))) && j < len &&
            (s[j] == '"' || s[j] == '#')) {  // r"raw", br#"raw"#; r#name is a raw identifier
            int hashes = 0;
            while (j + hashes < len && hashes < 31 && s[j + hashes] == '#')
                hashes++;
            if (j + hashes < len && s[j + hashes] == '"') {
                end = raw_rust_end(s, len, j + hashes + 1, hashes);
                emit(out, TOK_STRING, i, end < 0 ? len : end);
                if (end < 0)
                    return (unsigned char)(CF_RAW | hashes << 3);
                i = end;
                continue;
            }
        }
        if (out->tokens) {  // the whole-file state pass needs no keyword lookups
            int kind = c_family_keyword(language, s + i, j - i);
            int k = j;
            while (k < len && s[k] == ' ')
                k++;
            if (kind >= 0)
                emit(out, kind, i, j);
            else if ((k < len && s[k] == '(') || (rust && j + 1 < len && s[j] == '!' && s[j + 1] != '='))
                emit(out, TOK_METHOD, i, j);  // a call, or a Rust macro!
            else if (c >= 'A' && c <= 'Z')
                emit(out, TOK_TYPE, i, j);
        }
        i = j;
    }
    return CF_NORMAL;
}

// ---- Script family: Python, Shell, PowerShell, SQL, YAML ---------------------------------------

static const char python_keywords[][9] = {
    "False", "None", "True", "and", "as", "assert", "async", "class", "cls", "def", "del", "from", "global",
    "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "self",
};

static const char python_control_keywords[][9] = {
    "await", "break", "case", "continue", "elif", "else", "except", "finally", "for", "if", "match", "raise",
    "return", "try", "while", "with", "yield",
};

static const char shell_keywords[][9] = {
    "alias", "cd", "declare", "echo", "eval", "exec", "export", "false", "function", "local", "printf", "read",
    "readonly", "set", "shift", "source", "test", "trap", "true", "unset",
};

static const char shell_control_keywords[][9] = {
    "break", "case", "continue", "do", "done", "elif", "else", "esac", "exit", "fi", "for", "if", "in", "return",
    "select", "then", "until", "while",
};

static const char powershell_keywords[][13] = {
    "begin", "class", "data", "dynamicparam", "end", "enum", "filter", "function", "hidden", "in", "param",
    "process", "static", "using", "workflow",
};

static const char powershell_control_keywords[][9] = {
    "break", "catch", "continue", "do", "else", "elseif", "exit", "finally", "for", "foreach", "if", "return",
    "switch", "throw", "trap", "try", "until", "while",
};

static const char sql_keywords[][12] = {
    "add", "all", "alter", "and", "as", "asc", "begin", "between", "bigint", "bit", "boolean", "by", "cascade",
    "char", "check", "column", "commit", "constraint", "create", "cross", "database", "date", "datetime", "decimal",
    "declare", "default", "delete", "desc", "distinct", "drop", "exec", "exists", "float", "foreign", "from",
    "full", "function", "go", "grant", "group", "having", "identity", "if", "in", "index", "inner", "insert", "int",
    "integer", "into", "is", "join", "key", "left", "like", "limit", "merge", "not", "null", "nvarchar", "offset",
    "on", "or", "order", "outer", "over", "partition", "primary", "procedure", "references", "returning", "revoke",
    "right", "rollback", "schema", "select", "set", "table", "text", "top", "transaction", "trigger", "truncate",
    "union", "unique", "update", "using", "values", "varchar", "view", "where", "window", "with",
};

static const char sql_control_keywords[][7] = {
    "case", "else", "end", "return", "then", "when", "while",
};

static const char yaml_keywords[][6] = {
    "false", "no", "null", "off", "on", "true", "yes",
};

// Lexer state: the construct that continues on the next line.
enum { SF_NORMAL, SF_COMMENT, SF_TRIPLE_DOUBLE, SF_TRIPLE_SINGLE, SF_HERE_DOUBLE, SF_HERE_SINGLE };

// TOK_CONTROL, TOK_KEYWORD, or -1. PowerShell, SQL and YAML ignore case.
static int script_keyword(int language, const char *s, int len)
{
    switch (language) {
    case LANG_PYTHON: return KEYWORD_KIND(python_control_keywords, python_keywords, s, len);
    case LANG_SHELL: return KEYWORD_KIND(shell_control_keywords, shell_keywords, s, len);
    }
    char lower[32];
    if (len >= (int)sizeof lower)
        return -1;
    for (int i = 0; i < len; i++)
        lower[i] = s[i] >= 'A' && s[i] <= 'Z' ? s[i] | 0x20 : s[i];
    switch (language) {
    case LANG_POWERSHELL: return KEYWORD_KIND(powershell_control_keywords, powershell_keywords, lower, len);
    case LANG_SQL: return KEYWORD_KIND(sql_control_keywords, sql_keywords, lower, len);
    case LANG_YAML: return IN_LIST(yaml_keywords, lower, len) ? TOK_KEYWORD : -1;
    }
    return -1;
}

// End of a one-line string from just after its quote. `escape` is 0 when the string has none.
static int quote_end(const char *s, int len, int i, char quote, char escape)
{
    for (; i < len; i++) {
        if (escape && s[i] == escape)
            i++;
        else if (s[i] == quote)
            return i + 1;
    }
    return len;
}

// End of a construct that spans lines (SF_*), or -1 when it continues on the next line.
static int script_construct_end(int language, int kind, const char *s, int len, int i)
{
    const char *close = kind == SF_COMMENT          ? (language == LANG_SQL ? "*/" : "#>")
                        : kind == SF_TRIPLE_DOUBLE ? "\"\"\""
                        : kind == SF_TRIPLE_SINGLE ? "'''"
                        : kind == SF_HERE_DOUBLE   ? "\"@"
                                                   : "'@";
    int n = kind == SF_TRIPLE_DOUBLE || kind == SF_TRIPLE_SINGLE ? 3 : 2;
    for (; i + n <= len; i++) {
        if (compare_word(close, s + i, n) != 0)
            continue;
        if ((kind == SF_HERE_DOUBLE || kind == SF_HERE_SINGLE) && !only_blanks_before(s, i))
            continue;  // "@ closes a here-string only at the start of a line
        return i + n;
    }
    return -1;
}

static int is_python_prefix(char c)
{
    return c == 'r' || c == 'R' || c == 'b' || c == 'B' || c == 'f' || c == 'F' || c == 'u' || c == 'U';
}

static unsigned char script_scan_line(int language, const char *s, int len, unsigned char state, TokenOut *out)
{
    int python = language == LANG_PYTHON, shell = language == LANG_SHELL, ps = language == LANG_POWERSHELL;
    int sql = language == LANG_SQL, yaml = language == LANG_YAML;
    int i = 0, j, end;
    if (state) {
        end = script_construct_end(language, state, s, len, 0);
        emit(out, state == SF_COMMENT ? TOK_COMMENT : TOK_STRING, 0, end < 0 ? len : end);
        if (end < 0)
            return state;
        i = end;
    }

    // YAML: a document marker, or a "key:" after the indentation and any "- " list dashes.
    if (yaml && i == 0) {
        if (len >= 3 && (compare_word("---", s, 3) == 0 || compare_word("...", s, 3) == 0)) {
            emit(out, TOK_PREPROCESSOR, 0, 3);
            i = 3;
        }
        int k = i;
        while (k < len && (s[k] == ' ' || s[k] == '\t' || (s[k] == '-' && k + 1 < len && s[k + 1] == ' ')))
            k++;
        for (j = k; j < len && s[j] != ':' && s[j] != '#' && s[j] != '"' && s[j] != '\'' && s[j] != '{' && s[j] != '[';
             j++) {
        }
        if (j > k && j < len && s[j] == ':' && (j + 1 == len || s[j + 1] == ' ' || s[j + 1] == '\t')) {
            emit(out, TOK_TYPE, k, j);
            i = j + 1;
        }
    }

    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        int after_blank = i == 0 || s[i - 1] == ' ' || s[i - 1] == '\t';
        if (c == ' ' || c == '\t') {
            i++;
            continue;
        }
        if (c == '$' && (shell || ps) && i + 1 < len) {  // $name, ${name}, $1, $?
            j = i + 1;
            if (s[j] == '{') {
                while (j < len && s[j] != '}')
                    j++;
                j = j < len ? j + 1 : len;
            } else if (is_ident((unsigned char)s[j])) {
                while (j < len && (is_ident((unsigned char)s[j]) || (ps && s[j] == ':')))
                    j++;
            } else {
                j++;
            }
            emit(out, TOK_TYPE, i, j);
            i = j;
            continue;
        }
        // Shell and YAML take # as a comment only at the start of a word: $# and a#b are not.
        if (c == '#' && !sql && (python || ps || after_blank)) {
            emit(out, TOK_COMMENT, i, len);
            return SF_NORMAL;
        }
        if (c == '-' && sql && i + 1 < len && s[i + 1] == '-') {
            emit(out, TOK_COMMENT, i, len);
            return SF_NORMAL;
        }
        if ((sql && c == '/' && i + 1 < len && s[i + 1] == '*') || (ps && c == '<' && i + 1 < len && s[i + 1] == '#')) {
            end = script_construct_end(language, SF_COMMENT, s, len, i + 2);
            emit(out, TOK_COMMENT, i, end < 0 ? len : end);
            if (end < 0)
                return SF_COMMENT;
            i = end;
            continue;
        }
        // PowerShell here-string: @" or @' at the end of a line, closed by "@ or '@ at the start of one.
        if (ps && c == '@' && i + 1 < len && (s[i + 1] == '"' || s[i + 1] == '\'') &&
            only_blanks_after(s, len, i + 2)) {
            emit(out, TOK_STRING, i, len);
            return s[i + 1] == '"' ? SF_HERE_DOUBLE : SF_HERE_SINGLE;
        }

        // Strings, with Python's r/b/f/u prefixes.
        int quote_at = -1;
        if (c == '"' || c == '\'')
            quote_at = i;
        else if (python && is_python_prefix((char)c)) {
            for (j = i; j < len && j - i < 2 && is_python_prefix(s[j]); j++) {
            }
            if (j < len && (s[j] == '"' || s[j] == '\''))
                quote_at = j;
        }
        if (yaml && quote_at == i && !after_blank && s[i - 1] != ':' && s[i - 1] != '[' && s[i - 1] != '{' &&
            s[i - 1] != ',')
            quote_at = -1;  // the apostrophe in don't, inside a plain YAML value
        if (quote_at >= 0) {
            char quote = s[quote_at];
            if (python && quote_at + 2 < len && s[quote_at + 1] == quote && s[quote_at + 2] == quote) {
                int kind = quote == '"' ? SF_TRIPLE_DOUBLE : SF_TRIPLE_SINGLE;
                end = script_construct_end(language, kind, s, len, quote_at + 3);
                emit(out, TOK_STRING, i, end < 0 ? len : end);
                if (end < 0)
                    return (unsigned char)kind;
                i = end;
                continue;
            }
            char escape = sql || (quote == '\'' && !python) ? 0 : ps ? '`' : '\\';
            end = quote_end(s, len, quote_at + 1, quote, escape);
            emit(out, sql && quote == '"' ? TOK_TYPE : TOK_STRING, i, end);  // SQL "quoted identifier"
            i = end;
            continue;
        }

        if ((python && c == '@') || (yaml && after_blank && (c == '&' || c == '*' || c == '!'))) {
            for (j = i + 1; j < len && (is_ident((unsigned char)s[j]) || s[j] == '.'); j++) {
            }
            emit(out, TOK_PREPROCESSOR, i, j);  // decorator; YAML anchor, alias or tag
            i = j;
            continue;
        }
        if (ps && c == '-' && after_blank && i + 1 < len && is_ident_start((unsigned char)s[i + 1])) {
            for (j = i + 1; j < len && is_ident((unsigned char)s[j]); j++) {
            }
            emit(out, TOK_KEYWORD, i, j);  // -Parameter, -eq
            i = j;
            continue;
        }
        if (is_digit(c)) {
            for (j = i; j < len && (is_ident((unsigned char)s[j]) || s[j] == '.'); j++) {
            }
            emit(out, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if (!is_ident_start(c)) {
            i++;
            continue;
        }

        int dashed = 0;  // PowerShell Verb-Noun
        for (j = i; j < len; j++) {
            if (ps && s[j] == '-' && j + 1 < len && is_ident_start((unsigned char)s[j + 1]))
                dashed = 1;
            else if (!is_ident((unsigned char)s[j]))
                break;
        }
        if (out->tokens) {  // the whole-file state pass needs no keyword lookups
            int kind = dashed ? TOK_METHOD : script_keyword(language, s + i, j - i);
            int k = j;
            while (k < len && s[k] == ' ')
                k++;
            if (kind >= 0)
                emit(out, kind, i, j);
            else if (!yaml && !shell && k < len && s[k] == '(')
                emit(out, TOK_METHOD, i, j);
            else if (python && c >= 'A' && c <= 'Z')
                emit(out, TOK_TYPE, i, j);
        }
        i = j;
    }
    return SF_NORMAL;
}

// ---- Markup and data: HTML, XML, CSS, JSON -----------------------------------------------------

// Offset just past the first `close` (n bytes) at or after i, or -1 when it is not on this line.
static int find_text(const char *s, int len, int i, const char *close, int n)
{
    for (; i + n <= len; i++) {
        if (compare_word(close, s + i, n) == 0)
            return i + n;
    }
    return -1;
}

// HTML/XML lexer state. Inside a tag, attributes may continue on the next line.
enum { ML_TEXT, ML_COMMENT, ML_CDATA, ML_TAG };

static unsigned char markup_scan_line(const char *s, int len, unsigned char state, TokenOut *out)
{
    int i = 0, j, end;
    if (state == ML_COMMENT || state == ML_CDATA) {
        end = find_text(s, len, 0, state == ML_COMMENT ? "-->" : "]]>", 3);
        emit(out, state == ML_COMMENT ? TOK_COMMENT : TOK_STRING, 0, end < 0 ? len : end);
        if (end < 0)
            return state;
        i = end;
        state = ML_TEXT;
    }
    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        if (state == ML_TAG) {  // attribute names and values up to > or />
            if (c == '>') {
                state = ML_TEXT;
                i++;
            } else if (c == '"' || c == '\'') {
                end = quote_end(s, len, i + 1, (char)c, 0);
                emit(out, TOK_STRING, i, end);
                i = end;
            } else if (is_ident_start(c)) {
                j = i;
                while (j < len && (is_ident((unsigned char)s[j]) || s[j] == '-' || s[j] == ':' || s[j] == '.'))
                    j++;
                emit(out, TOK_TYPE, i, j);
                i = j;
            } else {
                i++;
            }
            continue;
        }
        if (c == '&') {  // &amp; &#160;
            for (j = i + 1; j < len && j - i < 12 && s[j] != ';' && s[j] != ' ' && s[j] != '<'; j++) {
            }
            if (j < len && s[j] == ';') {
                emit(out, TOK_NUMBER, i, j + 1);
                i = j + 1;
                continue;
            }
        }
        if (c != '<') {
            i++;
            continue;
        }
        if (i + 4 <= len && compare_word("<!--", s + i, 4) == 0) {
            end = find_text(s, len, i + 4, "-->", 3);
            emit(out, TOK_COMMENT, i, end < 0 ? len : end);
            if (end < 0)
                return ML_COMMENT;
            i = end;
            continue;
        }
        if (i + 9 <= len && compare_word("<![CDATA[", s + i, 9) == 0) {
            end = find_text(s, len, i + 9, "]]>", 3);
            emit(out, TOK_STRING, i, end < 0 ? len : end);
            if (end < 0)
                return ML_CDATA;
            i = end;
            continue;
        }
        j = i + 1;
        int declaration = j < len && (s[j] == '!' || s[j] == '?');  // <!DOCTYPE ...>, <?xml ...?>
        if (j < len && (s[j] == '/' || declaration))
            j++;
        if (j >= len || !is_ident_start((unsigned char)s[j])) {
            i++;
            continue;
        }
        int name = j;
        while (j < len && (is_ident((unsigned char)s[j]) || s[j] == '-' || s[j] == ':' || s[j] == '.'))
            j++;
        emit(out, declaration ? TOK_PREPROCESSOR : TOK_KEYWORD, declaration ? i : name, j);
        state = ML_TAG;
        i = j;
    }
    return state;
}

// CSS lexer state: bit 0 set inside a comment, the brace depth above it (selectors at depth 0).
static unsigned char css_scan_line(const char *s, int len, unsigned char state, TokenOut *out)
{
    int depth = state >> 1, i = 0, j, end;
    if (state & 1) {
        end = block_comment_end(s, len, 0);
        emit(out, TOK_COMMENT, 0, end < 0 ? len : end);
        if (end < 0)
            return state;
        i = end;
    }
    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        if (c == '/' && i + 1 < len && s[i + 1] == '*') {
            end = block_comment_end(s, len, i + 2);
            emit(out, TOK_COMMENT, i, end < 0 ? len : end);
            if (end < 0)
                return (unsigned char)(1 | depth << 1);
            i = end;
            continue;
        }
        if (c == '/' && i + 1 < len && s[i + 1] == '/') {  // SCSS and Less line comment
            emit(out, TOK_COMMENT, i, len);
            break;
        }
        if (c == '{' && depth < 127) {  // 127: the depth has 7 bits of the state
            depth++;
            i++;
            continue;
        }
        if (c == '}' && depth > 0) {
            depth--;
            i++;
            continue;
        }
        if (c == '"' || c == '\'') {
            end = quote_end(s, len, i + 1, (char)c, '\\');
            emit(out, TOK_STRING, i, end);
            i = end;
            continue;
        }
        if (c == '@' || (c == '!' && depth > 0)) {  // @media, !important
            for (j = i + 1; j < len && (is_ident((unsigned char)s[j]) || s[j] == '-'); j++) {
            }
            emit(out, c == '@' ? TOK_PREPROCESSOR : TOK_KEYWORD, i, j);
            i = j;
            continue;
        }
        // A selector: at the top level, or nested (in @media, SCSS) when a { comes before the next ; or }.
        if (is_ident_start(c) || c == '.' || c == '#' || c == ':' || c == '*') {
            j = i;
            while (depth > 0 && j < len && s[j] != '{' && s[j] != ';' && s[j] != '}')
                j++;
            if (depth == 0 || (j < len && s[j] == '{')) {
                for (j = i + 1; j < len && (is_ident((unsigned char)s[j]) || s[j] == '-' || s[j] == '.' ||
                                           s[j] == '#' || s[j] == ':' || s[j] == '*');
                     j++) {
                }
                emit(out, TOK_KEYWORD, i, j);
                i = j;
                continue;
            }
        }
        if (depth > 0 && (is_digit(c) || c == '#' || (c == '.' && i + 1 < len && is_digit((unsigned char)s[i + 1])))) {
            for (j = i + 1; j < len && (is_ident((unsigned char)s[j]) || s[j] == '.' || s[j] == '%'); j++) {
            }
            emit(out, TOK_NUMBER, i, j);  // 12px, 50%, #fff
            i = j;
            continue;
        }
        if (depth > 0 && (is_ident_start(c) || c == '-')) {
            for (j = i + 1; j < len && (is_ident((unsigned char)s[j]) || s[j] == '-'); j++) {
            }
            int k = j;
            while (k < len && s[k] == ' ')
                k++;
            if (k < len && s[k] == ':')
                emit(out, TOK_TYPE, i, j);  // property:
            else if (k < len && s[k] == '(')
                emit(out, TOK_METHOD, i, j);  // url(, rgb(
            i = j;
            continue;
        }
        i++;
    }
    return (unsigned char)(depth << 1);
}

// JSON lexer state: 1 inside a /* comment */ (JSON with comments).
static unsigned char json_scan_line(const char *s, int len, unsigned char state, TokenOut *out)
{
    int i = 0, j, end;
    if (state) {
        end = block_comment_end(s, len, 0);
        emit(out, TOK_COMMENT, 0, end < 0 ? len : end);
        if (end < 0)
            return 1;
        i = end;
    }
    while (i < len) {
        unsigned char c = (unsigned char)s[i];
        if (c == '/' && i + 1 < len && s[i + 1] == '/') {
            emit(out, TOK_COMMENT, i, len);
            return 0;
        }
        if (c == '/' && i + 1 < len && s[i + 1] == '*') {
            end = block_comment_end(s, len, i + 2);
            emit(out, TOK_COMMENT, i, end < 0 ? len : end);
            if (end < 0)
                return 1;
            i = end;
            continue;
        }
        if (c == '"') {
            end = quote_end(s, len, i + 1, '"', '\\');
            int k = end;
            while (k < len && (s[k] == ' ' || s[k] == '\t'))
                k++;
            emit(out, k < len && s[k] == ':' ? TOK_TYPE : TOK_STRING, i, end);  // "key": is a type
            i = end;
            continue;
        }
        if (is_digit(c) || (c == '-' && i + 1 < len && is_digit((unsigned char)s[i + 1]))) {
            j = i + 1;
            while (j < len && (is_ident((unsigned char)s[j]) || s[j] == '.' || s[j] == '+' || s[j] == '-'))
                j++;
            emit(out, TOK_NUMBER, i, j);
            i = j;
            continue;
        }
        if (is_ident_start(c)) {
            for (j = i; j < len && is_ident((unsigned char)s[j]); j++) {
            }
            if (compare_word("true", s + i, j - i) == 0 || compare_word("false", s + i, j - i) == 0 ||
                compare_word("null", s + i, j - i) == 0)
                emit(out, TOK_KEYWORD, i, j);
            i = j;
            continue;
        }
        i++;
    }
    return 0;
}

unsigned char syntax_scan_line(int language, const char *s, int len, unsigned char state, Token *tokens, int cap,
                               int *count)
{
    TokenOut out = {tokens, cap, 0};
    unsigned char next = 0;
    switch (language) {
    case LANG_CS: return cs_scan_line(s, len, state, tokens, cap, count);
    case LANG_C:
    case LANG_GO:
    case LANG_JAVA:
    case LANG_JS:
    case LANG_KOTLIN:
    case LANG_RUST:
    case LANG_TS:
    case LANG_ZIG: next = c_family_scan_line(language, s, len, state, &out); break;
    case LANG_POWERSHELL:
    case LANG_PYTHON:
    case LANG_SHELL:
    case LANG_SQL:
    case LANG_YAML: next = script_scan_line(language, s, len, state, &out); break;
    case LANG_HTML:
    case LANG_XML: next = markup_scan_line(s, len, state, &out); break;
    case LANG_CSS: next = css_scan_line(s, len, state, &out); break;
    case LANG_JSON: next = json_scan_line(s, len, state, &out); break;
    }
    if (count)
        *count = out.count;
    return next;
}

int syntax_language_for_path(const char *path)
{
    const char *dot = NULL;
    for (const char *p = path; *p; p++) {
        if (*p == '.')
            dot = p + 1;
        else if (*p == '/')
            dot = NULL;
    }
    if (!dot)
        return LANG_PLAIN;
    char ext[8];
    int n = 0;
    while (dot[n] && n < 8) {
        ext[n] = dot[n] >= 'A' && dot[n] <= 'Z' ? dot[n] | 0x20 : dot[n];
        n++;
    }
    if (dot[n])
        return LANG_PLAIN;  // longer than any extension in the table
    for (int i = 0; i < (int)(sizeof extensions / sizeof *extensions); i++) {
        if (compare_word(extensions[i].ext, ext, n) == 0)
            return extensions[i].language;
    }
    return LANG_PLAIN;
}
