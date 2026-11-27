#pragma once
#include <windows.h>

// Reserve-then-commit block: grows in place, so a pointer into it stays valid, and the whole
// thing is released with one VirtualFree.
typedef struct {
    char *base;
    size_t used, committed, reserved;
} Arena;

void arena_init(Arena *a, size_t reserve);
void *arena_push(Arena *a, size_t size);
char *arena_tail(Arena *a, size_t min_free);
void arena_free(Arena *a);
__attribute__((noreturn)) void fatal(const wchar_t *what);
int utf8_to_wide(const char *src, int len, wchar_t *dst, int cap);

// One changed file. Paths point into git's output.
typedef struct {
    char kind;          // '1' changed, '2' renamed/copied, 'u' unmerged, '?' untracked, 'r' ref compare
    char x, y;          // index and working-tree status letters, '.' when unchanged; ref compare: x only
    const char *path;   // UTF-8, '/' separated, relative to the repo root
    const char *orig_path;
    const char *hashes; // '1'/'2': "<HEAD blob> <index blob>", hashes_len chars; NULL otherwise
    int hashes_len;
} FileEntry;

enum { ROW_STAGED_HEADER, ROW_UNSTAGED_HEADER, ROW_STAGED, ROW_UNSTAGED, ROW_REF };

// One row of the file list: a group header, a file shown in one group, or (tree view) a folder of
// a group. A file with staged and unstaged changes has a row in each group.
typedef struct {
    unsigned char type;
    unsigned char folder;  // tree: a folder row; `file` is a file under it, whose path holds the name
    unsigned char depth;   // tree: nesting level, 0 at the top
    unsigned char collapsed;  // folder: its rows are hidden; set by the UI
    int file;
    unsigned short name_start, name_len;  // folder: its name in files[file].path, "a/b" for a compact chain
    int end;               // folder: index of the first row after its subtree
} ListRow;

typedef struct {
    Arena arena;            // owns this struct and everything it points to
    wchar_t root[1024];     // repo top-level folder
    wchar_t base[256], target[256];  // ref compare when base is set; empty target = working tree
    int has_head;           // 0 in a repo without commits
    int auto_refresh;       // 1 when started by a change in the folder, not by the user
    FileEntry *files;
    int file_count;
    ListRow *rows;          // flat view
    int row_count;
    ListRow *tree_rows;     // tree view: the same groups, folders first, then files
    int tree_count;
    int *shown;             // filled by the UI: index into rows or tree_rows of each row the list shows
    int shown_count;
    wchar_t error[512];     // set when the file list could not be read
    DWORD elapsed_ms;
} StatusResult;

enum { LINE_CONTEXT, LINE_REMOVED, LINE_ADDED, LINE_GAP };

// One row of a diff: a span of the git output. 8 bytes, because a big file has many of them.
// A LINE_GAP row (between hunks) has no text; its offset/length hold the next hunk's start line
// numbers (old/new) instead.
typedef struct {
    unsigned offset;
    unsigned length : 30;
    unsigned kind : 2;
} DiffLine;

// Line numbers of every 1024th row, so painting can find any row's numbers without walking
// the whole file.
typedef struct {
    unsigned old_no, new_no;
} LineNumbers;

// Changed words of a line paired with a similar line on the other side: bytes [start, end).
typedef struct {
    int line;  // index into DiffResult.lines
    unsigned short start, end;
} WordSpan;

// One row of the side-by-side view: indexes into DiffResult.lines, -1 for a blank filler.
typedef struct {
    int left, right;
} SideRow;

typedef struct {
    Arena arena;            // owns this struct and everything it points to
    int generation;         // matches the request; older results are dropped
    const char *text;       // git diff output
    DiffLine *lines;
    int line_count;
    LineNumbers *checkpoints;
    SideRow *side;
    int side_count;
    WordSpan *word_spans;   // in line order
    int word_span_count;
    unsigned char *states;  // NULL for plain text: lexer state at the start of each row, [old, new] pairs
    int language;           // LANG_* the states were made for, never LANG_AUTO
    int language_auto;      // the language came from the file name, not a pick
    int ignore_eol;         // made with --ignore-cr-at-eol
    unsigned max_line_no;
    unsigned max_line_len;  // bytes, for the horizontal scroll range
    int added, removed;
    int binary;
    wchar_t error[512];
    DWORD elapsed_ms;
    // What the diff was made from, so an auto-refresh can keep it when the file did not change.
    int mode;
    char x, y;
    char hashes[130];                 // FileEntry.hashes, NUL-terminated
    ULONGLONG file_time, file_size;   // working-tree file; 0 when not read or missing
} DiffResult;

// One entry of the ref dropdown. Strings point into git's output.
typedef struct {
    char kind;           // 'H' HEAD, 'b' branch, 'r' remote branch, 't' tag, 'c' recent commit
    const char *name;    // what goes into the ref box (short hash for a commit)
    const char *detail;  // commit subject; NULL for refs
} RefEntry;

typedef struct {
    Arena arena;            // owns this struct and everything it points to
    int generation;         // matches the request; older results are dropped
    wchar_t root[1024];
    RefEntry *refs;
    int count;
    int *shown;             // filled by the UI: index into refs, or -1 - group for a group header
    int shown_count;
    wchar_t error[512];
} RefList;

#define WM_APP_STATUS (WM_APP + 1)  // lParam: StatusResult*, owned by the receiver
#define WM_APP_DIFF (WM_APP + 2)    // lParam: DiffResult*, owned by the receiver
#define WM_APP_REFS (WM_APP + 3)    // lParam: RefList*, owned by the receiver

void git_start(HWND notify);
void git_request_status(const wchar_t *folder, const wchar_t *base, const wchar_t *target);
void git_request_diff(const StatusResult *status, const ListRow *row, int generation, int language, int ignore_eol);
int diff_is_current(const DiffResult *d, const StatusResult *status, const ListRow *row);
void git_request_refs(const wchar_t *root, int generation);
void status_free(StatusResult *r);
void refs_free(RefList *r);
void diff_free(DiffResult *r);
void diff_line_numbers(const DiffResult *d, int row, unsigned *old_no, unsigned *new_no);
void diff_advance_numbers(const DiffLine *line, unsigned *old_no, unsigned *new_no);

enum { TOK_KEYWORD, TOK_CONTROL, TOK_TYPE, TOK_METHOD, TOK_STRING, TOK_NUMBER, TOK_COMMENT, TOK_PREPROCESSOR };

typedef struct {
    int start, end;  // byte offsets in the line
    int kind;
} Token;

// Highlighters. After LANG_PLAIN the order is the menu order (A-Z). LANG_AUTO only appears in a request.
enum {
    LANG_AUTO, LANG_PLAIN, LANG_C, LANG_CS, LANG_CSS, LANG_GO, LANG_HTML, LANG_JAVA, LANG_JS, LANG_JSON, LANG_KOTLIN,
    LANG_POWERSHELL, LANG_PYTHON, LANG_RUST, LANG_SHELL, LANG_SQL, LANG_TS, LANG_XML, LANG_YAML, LANG_ZIG, LANG_COUNT
};

// Scans one line in `language` starting in lexer `state` (0 at the top of a file) and returns the
// state at the start of the next line. Tokens are written only when `tokens` is not NULL.
unsigned char syntax_scan_line(int language, const char *s, int len, unsigned char state, Token *tokens, int cap,
                               int *count);
int syntax_language_for_path(const char *path);  // LANG_PLAIN when the extension is not known
