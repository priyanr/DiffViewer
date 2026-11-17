#include "app.h"

// Diff against the empty tree in a repo that has no commits yet.
#define EMPTY_TREE L"4b825dc642cb6eb9a060e54bf8d69288fbee4904"
#define DIFF_OPTIONS L"--no-color --no-ext-diff --no-textconv -U999999 --histogram -M"
#define REFRESH_DELAY_MS 300

enum { DIFF_UNSTAGED, DIFF_STAGED, DIFF_UNTRACKED, DIFF_UNMERGED, DIFF_REFS };

typedef struct {
    wchar_t folder[1024], base[256], target[256];
} StatusRequest;

typedef struct {
    wchar_t root[1024], path[1024], orig_path[1024];  // orig_path is empty unless renamed
    wchar_t base[256], target[256];
    int mode, has_head, generation, language, language_auto, ignore_eol;
    char x, y, hashes[130];  // copied into the result, see DiffResult
} DiffRequest;

typedef struct {
    wchar_t root[1024];
    int generation;
} RefsRequest;

static HWND notify_hwnd;
static HANDLE request_event;
static CRITICAL_SECTION request_lock;

// Written by the UI thread under request_lock; the worker copies them out.
static StatusRequest request_status;
static int status_pending;
static DiffRequest request_diff;
static int diff_pending;
static RefsRequest request_refs;
static int refs_pending;

// Only the worker thread uses these, so they need no lock. Static to keep them off the stack.
static wchar_t command_line[4096];
static wchar_t args[4096];
static StatusRequest worker_status;  // the last list the user asked for; rerun when the folder changes
static DiffRequest worker_diff;
static RefsRequest worker_refs;
static wchar_t worker_path[2048];
static HANDLE watch_dir = INVALID_HANDLE_VALUE;
static OVERLAPPED watch_overlapped;
static wchar_t watch_root[1024];
static DWORD watch_buffer[8192];  // 32 KB; ReadDirectoryChangesW needs DWORD alignment

static void copy_wide(wchar_t *dst, int cap, const wchar_t *src)
{
    int i = 0;
    for (; src[i] && i < cap - 1; i++)
        dst[i] = src[i];
    dst[i] = 0;
}

static void append_wide(wchar_t *dst, int cap, const wchar_t *src)
{
    int len = lstrlenW(dst);
    copy_wide(dst + len, cap - len, src);
}

static void append_quoted(wchar_t *dst, int cap, const wchar_t *text)
{
    append_wide(dst, cap, L" \"");
    append_wide(dst, cap, text);
    append_wide(dst, cap, L"\"");
}

// Runs `git <args>` in `folder`. Stdout is appended to the arena, NUL-terminated, and returned in
// *out. Returns 0 when git's exit code is one of `ok_codes` (a bit per code); otherwise the code,
// or -1 if git could not be started, and `err` says why.
static int git_run(Arena *a, const wchar_t *folder, const wchar_t *git_args, unsigned ok_codes, char **out,
                   size_t *out_len, wchar_t *err, int err_cap)
{
    SECURITY_ATTRIBUTES inherit = {sizeof inherit, NULL, TRUE};
    HANDLE out_read, out_write, err_read, err_write;
    // stderr is read only after stdout ends; a large pipe buffer keeps git from blocking on it.
    if (!CreatePipe(&out_read, &out_write, &inherit, 0) || !CreatePipe(&err_read, &err_write, &inherit, 65536))
        fatal(L"CreatePipe");
    SetHandleInformation(out_read, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(err_read, HANDLE_FLAG_INHERIT, 0);
    HANDLE null_in = CreateFileW(L"NUL", GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, &inherit,
                                 OPEN_EXISTING, 0, NULL);

    // --no-optional-locks: status must not refresh (write) the index of the repo being viewed,
    // which would also trigger the folder watcher in a loop.
    command_line[0] = 0;
    append_wide(command_line, 4096, L"git --no-optional-locks ");
    append_wide(command_line, 4096, git_args);

    STARTUPINFOW si = {sizeof si};
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = null_in;
    si.hStdOutput = out_write;
    si.hStdError = err_write;
    PROCESS_INFORMATION pi;
    BOOL started = CreateProcessW(NULL, command_line, NULL, NULL, TRUE, CREATE_NO_WINDOW, NULL, folder, &si, &pi);
    DWORD start_error = GetLastError();
    CloseHandle(out_write);
    CloseHandle(err_write);
    CloseHandle(null_in);

    int result = -1;
    if (!started) {
        if (start_error == ERROR_FILE_NOT_FOUND)
            copy_wide(err, err_cap, L"git.exe was not found on PATH.");
        else if (start_error == ERROR_DIRECTORY)
            wsprintfW(err, L"Folder not found: %s", folder);
        else
            wsprintfW(err, L"Could not start git (error %lu).", start_error);
        goto cleanup;
    }

    size_t start = a->used;
    for (;;) {
        char *dst = arena_tail(a, 65536);
        DWORD got;
        if (!ReadFile(out_read, dst, 65536, &got, NULL) || got == 0)
            break;
        a->used += got;
    }
    *out_len = a->used - start;
    *out = a->base + start;
    *(char *)arena_push(a, 1) = 0;

    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD code;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);

    if (code < 32 && (ok_codes & (1u << code))) {
        result = 0;
    } else {
        result = (int)code;
        char msg[1024];
        DWORD got = 0;
        ReadFile(err_read, msg, sizeof msg - 1, &got, NULL);
        while (got > 0 && (msg[got - 1] == '\n' || msg[got - 1] == '\r'))
            got--;
        int n = utf8_to_wide(msg, (int)got, err, err_cap - 1);
        err[n] = 0;
        if (n == 0)
            wsprintfW(err, L"git exited with code %d.", result);
    }

cleanup:
    CloseHandle(out_read);
    CloseHandle(err_read);
    return result;
}

// Skips `count` space-separated fields of a porcelain v2 record.
static const char *skip_fields(const char *p, int count)
{
    while (count > 0 && *p) {
        if (*p++ == ' ')
            count--;
    }
    return p;
}

static int starts_with(const char *s, const char *prefix)
{
    while (*prefix) {
        if (*s++ != *prefix++)
            return 0;
    }
    return 1;
}

static const char *next_token(const char **p)
{
    const char *token = *p;
    *p += lstrlenA(token) + 1;
    return token;
}

static void add_row(Arena *arena, StatusResult *r, int type, int file)
{
    ListRow *row = arena_push(arena, sizeof *row);
    row->type = type;
    row->file = file;
    r->row_count++;
}

static void watch_issue(void)
{
    if (!ReadDirectoryChangesW(watch_dir, watch_buffer, sizeof watch_buffer, TRUE,
                               FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE |
                                   FILE_NOTIFY_CHANGE_LAST_WRITE,
                               NULL, &watch_overlapped, NULL)) {
        CloseHandle(watch_dir);  // e.g. the folder is on a share that doesn't support it: no auto-refresh
        watch_dir = INVALID_HANDLE_VALUE;
    }
}

static void watch_folder(const wchar_t *root)
{
    if (watch_dir != INVALID_HANDLE_VALUE && lstrcmpiW(root, watch_root) == 0)
        return;
    if (watch_dir != INVALID_HANDLE_VALUE) {
        DWORD bytes;
        CancelIoEx(watch_dir, &watch_overlapped);
        GetOverlappedResult(watch_dir, &watch_overlapped, &bytes, TRUE);  // the buffer is reused below
        CloseHandle(watch_dir);
    }
    copy_wide(watch_root, 1024, root);
    watch_dir = CreateFileW(root, FILE_LIST_DIRECTORY, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
                            OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED, NULL);
    if (watch_dir != INVALID_HANDLE_VALUE)
        watch_issue();
}

static int name_starts(const wchar_t *name, int len, const wchar_t *prefix)
{
    int i = 0;
    for (; prefix[i]; i++) {
        if (i >= len || name[i] != prefix[i])
            return 0;
    }
    return 1;
}

// True when a notification touches the working tree or the parts of .git that change what the
// list shows (the index, HEAD, branches). Object and log writes inside .git are ignored.
static int watch_changes_matter(DWORD bytes)
{
    if (bytes == 0)
        return 1;  // the buffer overflowed: assume something relevant changed
    const char *p = (const char *)watch_buffer;
    for (;;) {
        const FILE_NOTIFY_INFORMATION *n = (const FILE_NOTIFY_INFORMATION *)p;
        int len = (int)(n->FileNameLength / sizeof(wchar_t));
        int in_git = (len == 4 && name_starts(n->FileName, len, L".git")) || name_starts(n->FileName, len, L".git\\");
        if (!in_git || (len == 10 && name_starts(n->FileName, len, L".git\\index")) ||
            (len == 9 && name_starts(n->FileName, len, L".git\\HEAD")) || name_starts(n->FileName, len, L".git\\refs\\"))
            return 1;
        if (!n->NextEntryOffset)
            return 0;
        p += n->NextEntryOffset;
    }
}

// Entries are pushed one after another right after git's output, so they form one array; the
// rows array follows the same way.
static void load_working_tree(Arena *arena, StatusResult *r, const char *out, size_t len)
{
    r->files = arena_push(arena, 0);
    const char *p = out, *end = out + len;
    while (p < end) {
        const char *record = next_token(&p);
        char kind = record[0];
        FileEntry *f;
        switch (kind) {
        case '1':
        case '2':
        case 'u':
            f = arena_push(arena, sizeof *f);
            f->kind = kind;
            f->x = record[2];
            f->y = record[3];
            f->path = skip_fields(record, kind == '1' ? 8 : kind == '2' ? 9 : 10);
            f->orig_path = kind == '2' ? next_token(&p) : NULL;  // a rename's original path follows
            f->hashes = kind == 'u' ? NULL : skip_fields(record, 6);  // fields 6, 7: HEAD and index blob
            f->hashes_len = kind == 'u' ? 0 : (int)(skip_fields(record, 8) - f->hashes - 1);
            break;
        case '?':
            f = arena_push(arena, sizeof *f);
            f->kind = '?';
            f->x = f->y = '?';
            f->path = record + 2;
            f->orig_path = NULL;
            f->hashes = NULL;
            f->hashes_len = 0;
            break;
        case '#':
            if (starts_with(record, "# branch.oid (initial)"))
                r->has_head = 0;
            continue;
        default:
            continue;  // ignored files
        }
        r->file_count++;
    }

    // Groups only when something is staged; otherwise one flat list, as the index equals HEAD.
    int staged = 0, unstaged = 0;
    for (int i = 0; i < r->file_count; i++) {
        const FileEntry *f = &r->files[i];
        staged += f->kind != '?' && f->kind != 'u' && f->x != '.';
        unstaged += f->kind == '?' || f->kind == 'u' || f->y != '.';
    }
    r->rows = arena_push(arena, 0);
    if (staged) {
        add_row(arena, r, ROW_STAGED_HEADER, -1);
        for (int i = 0; i < r->file_count; i++) {
            const FileEntry *f = &r->files[i];
            if (f->kind != '?' && f->kind != 'u' && f->x != '.')
                add_row(arena, r, ROW_STAGED, i);
        }
        if (unstaged)
            add_row(arena, r, ROW_UNSTAGED_HEADER, -1);
    }
    for (int i = 0; i < r->file_count; i++) {
        const FileEntry *f = &r->files[i];
        if (f->kind == '?' || f->kind == 'u' || f->y != '.')
            add_row(arena, r, ROW_UNSTAGED, i);
    }
}

static void load_ref_compare(Arena *arena, StatusResult *r, const char *out, size_t len)
{
    r->files = arena_push(arena, 0);
    const char *p = out, *end = out + len;
    while (p < end) {
        const char *status = next_token(&p);
        FileEntry *f = arena_push(arena, sizeof *f);
        f->kind = 'r';
        f->x = status[0];
        f->y = '.';
        f->orig_path = status[0] == 'R' || status[0] == 'C' ? next_token(&p) : NULL;
        f->path = next_token(&p);
        f->hashes = NULL;
        f->hashes_len = 0;
        r->file_count++;
    }
    r->rows = arena_push(arena, 0);
    for (int i = 0; i < r->file_count; i++)
        add_row(arena, r, ROW_REF, i);
}

static StatusResult *load_status(const StatusRequest *req, int auto_refresh)
{
    Arena arena;
    arena_init(&arena, (size_t)1 << 30);
    StatusResult *r = arena_push(&arena, sizeof *r);
    r->has_head = 1;
    r->auto_refresh = auto_refresh;
    copy_wide(r->base, 256, req->base);
    copy_wide(r->target, 256, req->target);
    DWORD started = GetTickCount();
    char *out;
    size_t len;

    // Paths are relative to the repo root, so git runs from there.
    if (git_run(&arena, req->folder, L"rev-parse --show-toplevel", 1, &out, &len, r->error, 512) != 0)
        goto done;
    while (len > 0 && (out[len - 1] == '\n' || out[len - 1] == '\r'))
        len--;
    int n = utf8_to_wide(out, (int)len, r->root, 1023);
    r->root[n] = 0;
    for (int i = 0; i < n; i++) {
        if (r->root[i] == '/')
            r->root[i] = '\\';
    }
    watch_folder(r->root);

    if (!req->base[0] && req->target[0]) {
        copy_wide(r->error, 512, L"Fill in the base ref too, or clear the target ref.");
        goto done;
    }
    if (!req->base[0]) {
        if (git_run(&arena, r->root, L"status --porcelain=v2 -z --branch --untracked-files=all", 1, &out, &len,
                    r->error, 512) == 0)
            load_working_tree(&arena, r, out, len);
        goto done;
    }

    // A ref typed as "-x" would be read as an option; a '"' or '\' could end the quoting and add
    // arguments. Git ref names can contain neither.
    if (req->base[0] == '-' || req->target[0] == '-') {
        copy_wide(r->error, 512, L"A ref can't start with '-'.");
        goto done;
    }
    for (const wchar_t *p = req->base; *p; p++) {
        if (*p == '"' || *p == '\\') {
            copy_wide(r->error, 512, L"A ref can't contain '\"' or '\\'.");
            goto done;
        }
    }
    for (const wchar_t *p = req->target; *p; p++) {
        if (*p == '"' || *p == '\\') {
            copy_wide(r->error, 512, L"A ref can't contain '\"' or '\\'.");
            goto done;
        }
    }
    args[0] = 0;
    append_wide(args, 4096, L"diff --name-status -z -M --no-color");
    append_quoted(args, 4096, req->base);
    if (req->target[0])
        append_quoted(args, 4096, req->target);
    append_wide(args, 4096, L" --");
    if (git_run(&arena, r->root, args, 1, &out, &len, r->error, 512) == 0)
        load_ref_compare(&arena, r, out, len);

done:
    r->arena = arena;
    r->elapsed_ms = GetTickCount() - started;
    return r;
}

// Cuts the line at *p off with a NUL, moves *p past it and returns it; NULL at `end`.
static char *next_line(char **p, char *end)
{
    if (*p >= end)
        return NULL;
    char *line = *p, *q = line;
    while (q < end && *q != '\n')
        q++;
    *p = q + 1;
    if (q > line && q[-1] == '\r')
        q--;
    *q = 0;
    return line;
}

static void add_ref(Arena *arena, RefList *r, char kind, const char *name, const char *detail)
{
    RefEntry *e = arena_push(arena, sizeof *e);  // follows the previous entry: nothing else is pushed meanwhile
    e->kind = kind;
    e->name = name;
    e->detail = detail;
    r->count++;
}

static RefList *load_refs(const RefsRequest *req)
{
    static const char *const prefixes[] = {"refs/heads/", "refs/remotes/", "refs/tags/"};
    static const char kinds[] = {'b', 'r', 't'};
    Arena arena;
    arena_init(&arena, (size_t)1 << 28);
    RefList *r = arena_push(&arena, sizeof *r);
    r->generation = req->generation;
    copy_wide(r->root, 1024, req->root);
    char *outs[2], *log_out;
    size_t lens[2], log_len = 0;
    // Branches newest first. Tags by version: sorting thousands of tags by date reads every tag object
    // (0.5 s for 2,000), by name reads none.
    if (git_run(&arena, req->root, L"for-each-ref --sort=-committerdate --format=%(refname) refs/heads refs/remotes", 1,
                &outs[0], &lens[0], r->error, 512) != 0 ||
        git_run(&arena, req->root, L"for-each-ref --sort=-v:refname --format=%(refname) refs/tags", 1, &outs[1],
                &lens[1], r->error, 512) != 0)
        goto done;
    // Fails in a repo without commits; then there is no HEAD either.
    if (git_run(&arena, req->root, L"log -n 30 --format=%h%x09%s", 1, &log_out, &log_len, r->error, 512) != 0) {
        log_len = 0;
        r->error[0] = 0;
    }

    r->refs = arena_push(&arena, 0);
    if (log_len)
        add_ref(&arena, r, 'H', "HEAD", NULL);
    char *p, *line;
    for (int o = 0; o < 2; o++) {
        p = outs[o];
        while ((line = next_line(&p, outs[o] + lens[o]))) {
            for (int i = 0; i < 3; i++) {
                if (!starts_with(line, prefixes[i]))
                    continue;
                const char *name = line + lstrlenA(prefixes[i]);
                int len = lstrlenA(name);
                if (!(kinds[i] == 'r' && len >= 5 && lstrcmpA(name + len - 5, "/HEAD") == 0))  // origin/HEAD
                    add_ref(&arena, r, kinds[i], name, NULL);
            }
        }
    }
    p = log_out;
    while (log_len && (line = next_line(&p, log_out + log_len))) {
        char *tab = line;
        while (*tab && *tab != '\t')
            tab++;
        if (*tab)
            *tab++ = 0;
        add_ref(&arena, r, 'c', line, tab);
    }
    r->shown = arena_push(&arena, (size_t)(r->count + 5) * sizeof *r->shown);  // + a header per group

done:
    r->arena = arena;
    return r;
}

static unsigned parse_number(const char **p)
{
    unsigned n = 0;
    while (**p >= '0' && **p <= '9')
        n = n * 10 + (unsigned)(*(*p)++ - '0');
    return n;
}

static void build_side_rows(Arena *arena, DiffResult *r)
{
    // A run of removed lines is paired, line by line, with the run of added lines after it; the
    // shorter side gets blank fillers.
    r->side = arena_push(arena, (size_t)r->line_count * sizeof(SideRow));
    int i = 0;
    while (i < r->line_count) {
        int kind = r->lines[i].kind;
        if (kind == LINE_REMOVED || kind == LINE_ADDED) {
            int removed_end = i;
            while (removed_end < r->line_count && r->lines[removed_end].kind == LINE_REMOVED)
                removed_end++;
            int added_end = removed_end;
            while (added_end < r->line_count && r->lines[added_end].kind == LINE_ADDED)
                added_end++;
            int removed = removed_end - i, added = added_end - removed_end;
            for (int k = 0; k < removed || k < added; k++) {
                SideRow *row = &r->side[r->side_count++];
                row->left = k < removed ? i + k : -1;
                row->right = k < added ? removed_end + k : -1;
            }
            i = added_end;
        } else {
            r->side[r->side_count++] = (SideRow){i, i};
            i++;
        }
    }
}

enum { WORD_LINE_MAX = 8192, WORD_TOKENS_MAX = 1024 };  // 8192: the part of a line painting draws
enum { WORD_LCS_MAX = 64 };  // changed middles longer than this, in tokens, are marked as one span

// The two lines being compared and their token match table. Static, not on the stack: a frame
// over 4 KB needs __chkstk, which only the C runtime provides.
static unsigned short worker_tokens[2][WORD_TOKENS_MAX + 1];
static unsigned char worker_lcs[WORD_LCS_MAX + 1][WORD_LCS_MAX + 1];

static int is_word_byte(unsigned char c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c >= 0x80;
}

// Splits a line into tokens (a run of word bytes, a run of blanks, or one other byte) and returns
// their count, with starts[count] = len; -1 when there are too many.
static int split_tokens(const char *s, int len, unsigned short *starts)
{
    int n = 0, i = 0;
    while (i < len) {
        if (n == WORD_TOKENS_MAX)
            return -1;
        starts[n++] = (unsigned short)i;
        unsigned char c = (unsigned char)s[i];
        if (is_word_byte(c)) {
            while (i < len && is_word_byte((unsigned char)s[i]))
                i++;
        } else if (c == ' ' || c == '\t') {
            while (i < len && (s[i] == ' ' || s[i] == '\t'))
                i++;
        } else {
            i++;
        }
    }
    starts[n] = (unsigned short)len;
    return n;
}

static int same_token(const char *a, const unsigned short *ta, int i, const char *b, const unsigned short *tb, int j)
{
    int n = ta[i + 1] - ta[i];
    if (n != tb[j + 1] - tb[j])
        return 0;
    for (int k = 0; k < n; k++) {
        if (a[ta[i] + k] != b[tb[j] + k])
            return 0;
    }
    return 1;
}

static int count_solid(const char *s, int from, int to)
{
    int n = 0;
    for (int i = from; i < to; i++)
        n += s[i] != ' ' && s[i] != '\t';
    return n;
}

static void push_span(Arena *arena, DiffResult *r, const char *s, int line, int start, int end)
{
    // Joined to the previous span when only blanks lie between them: that reads as one change.
    if (r->word_span_count > 0) {
        WordSpan *last = &r->word_spans[r->word_span_count - 1];
        if (last->line == line && count_solid(s, last->end, start) == 0) {
            last->end = (unsigned short)end;
            return;
        }
    }
    WordSpan *w = arena_push(arena, sizeof *w);  // 8 bytes: consecutive pushes stay one array
    if (r->word_span_count == 0)
        r->word_spans = w;
    *w = (WordSpan){line, (unsigned short)start, (unsigned short)end};
    r->word_span_count++;
}

// Marks the changed words of one side (new_side 0: old line, 1: new line) of a paired line.
static void mark_pair(Arena *arena, DiffResult *r, int old_line, int new_line, int new_side)
{
    const DiffLine *la = &r->lines[old_line], *lb = &r->lines[new_line];
    const char *a = r->text + la->offset, *b = r->text + lb->offset;
    int len_a = la->length < WORD_LINE_MAX ? (int)la->length : WORD_LINE_MAX;
    int len_b = lb->length < WORD_LINE_MAX ? (int)lb->length : WORD_LINE_MAX;
    unsigned short *ta = worker_tokens[0], *tb = worker_tokens[1];
    int na = split_tokens(a, len_a, ta), nb = split_tokens(b, len_b, tb);
    if (na < 0 || nb < 0)
        return;

    int pre = 0, suf = 0;
    while (pre < na && pre < nb && same_token(a, ta, pre, b, tb, pre))
        pre++;
    while (suf < na - pre && suf < nb - pre && same_token(a, ta, na - 1 - suf, b, tb, nb - 1 - suf))
        suf++;

    // A mostly rewritten line gets no marks: they would cover nearly all of it. `common` counts the
    // unchanged non-blank bytes.
    int common = count_solid(a, 0, ta[pre]) + count_solid(a, ta[na - suf], len_a);
    int solid_a = count_solid(a, 0, len_a), solid_b = count_solid(b, 0, len_b);
    int longer = solid_a > solid_b ? solid_a : solid_b;
    int ma = na - suf - pre, mb = nb - suf - pre;  // the changed middle, in tokens
    if (ma > WORD_LCS_MAX || mb > WORD_LCS_MAX) {
        if (common * 2 < longer)
            return;
        if (new_side && mb > 0)
            push_span(arena, r, b, new_line, tb[pre], tb[pre + mb]);
        else if (!new_side && ma > 0)
            push_span(arena, r, a, old_line, ta[pre], ta[pre + ma]);
        return;
    }

    // Longest common subsequence of the middle tokens: worker_lcs[i][j] is its length for the old
    // tokens from pre + i and the new ones from pre + j.
    for (int i = ma; i >= 0; i--) {
        for (int j = mb; j >= 0; j--) {
            if (i == ma || j == mb)
                worker_lcs[i][j] = 0;
            else if (same_token(a, ta, pre + i, b, tb, pre + j))
                worker_lcs[i][j] = (unsigned char)(worker_lcs[i + 1][j + 1] + 1);
            else if (worker_lcs[i + 1][j] > worker_lcs[i][j + 1])
                worker_lcs[i][j] = worker_lcs[i + 1][j];
            else
                worker_lcs[i][j] = worker_lcs[i][j + 1];
        }
    }
    // Walked twice: first to count the unchanged bytes, then to mark this side's changed tokens.
    for (int pass = 0; pass < 2; pass++) {
        int i = 0, j = 0;
        while (i < ma || j < mb) {
            if (i < ma && j < mb && same_token(a, ta, pre + i, b, tb, pre + j)) {
                if (pass == 0)
                    common += count_solid(a, ta[pre + i], ta[pre + i + 1]);
                i++;
                j++;
            } else if (j == mb || (i < ma && worker_lcs[i + 1][j] >= worker_lcs[i][j + 1])) {
                if (pass == 1 && !new_side)
                    push_span(arena, r, a, old_line, ta[pre + i], ta[pre + i + 1]);
                i++;
            } else {
                if (pass == 1 && new_side)
                    push_span(arena, r, b, new_line, tb[pre + j], tb[pre + j + 1]);
                j++;
            }
        }
        if (pass == 0 && common * 2 < longer)
            return;
    }
}

static void compute_word_spans(Arena *arena, DiffResult *r)
{
    // A block's removed lines come before its added lines, and spans must be in line order: so all
    // old sides of a block are marked first, then all new sides.
    int i = 0;
    while (i < r->side_count) {
        const SideRow *row = &r->side[i];
        if (row->left < 0 || row->right < 0 || row->left == row->right) {
            i++;
            continue;
        }
        int end = i + 1;
        while (end < r->side_count && r->side[end].left == r->side[end - 1].left + 1 &&
               r->side[end].right == r->side[end - 1].right + 1)
            end++;
        for (int new_side = 0; new_side < 2; new_side++) {
            for (int k = i; k < end; k++)
                mark_pair(arena, r, r->side[k].left, r->side[k].right, new_side);
        }
        i = end;
    }
}

static void compute_syntax_states(Arena *arena, DiffResult *r)
{
    // The old and new files are scanned separately: a removed line can open a comment that only
    // the old side sees.
    r->states = arena_push(arena, (size_t)r->line_count * 2);
    unsigned char old_state = 0, new_state = 0;
    int lang = r->language;
    for (int i = 0; i < r->line_count; i++) {
        const DiffLine *line = &r->lines[i];
        r->states[i * 2] = old_state;
        r->states[i * 2 + 1] = new_state;
        const char *s = r->text + line->offset;
        int len = (int)line->length;
        switch (line->kind) {
        case LINE_CONTEXT:
            old_state = syntax_scan_line(lang, s, len, old_state, NULL, 0, NULL);
            new_state = r->states[i * 2] == r->states[i * 2 + 1]
                            ? old_state
                            : syntax_scan_line(lang, s, len, new_state, NULL, 0, NULL);
            break;
        case LINE_REMOVED: old_state = syntax_scan_line(lang, s, len, old_state, NULL, 0, NULL); break;
        case LINE_ADDED: new_state = syntax_scan_line(lang, s, len, new_state, NULL, 0, NULL); break;
        case LINE_GAP: old_state = new_state = 0; break;  // the skipped lines are unknown
        }
    }
}

// Last write time and size of a working-tree file, both 0 when it is missing. `buf` holds 2048 chars.
static void file_stamp(const wchar_t *root, const wchar_t *path, wchar_t *buf, ULONGLONG *time, ULONGLONG *size)
{
    buf[0] = 0;
    append_wide(buf, 2048, root);
    append_wide(buf, 2048, L"\\");
    append_wide(buf, 2048, path);
    WIN32_FILE_ATTRIBUTE_DATA a;
    if (!GetFileAttributesExW(buf, GetFileExInfoStandard, &a)) {
        *time = *size = 0;
        return;
    }
    *time = (ULONGLONG)a.ftLastWriteTime.dwHighDateTime << 32 | a.ftLastWriteTime.dwLowDateTime;
    *size = (ULONGLONG)a.nFileSizeHigh << 32 | a.nFileSizeLow;
}

static DiffResult *load_diff(const DiffRequest *req)
{
    Arena arena;
    arena_init(&arena, (size_t)1 << 32);  // DiffLine offsets are 32-bit
    DiffResult *r = arena_push(&arena, sizeof *r);
    r->generation = req->generation;
    DWORD started = GetTickCount();

    // Stamped before git reads the file: a write during the diff then shows up as a change.
    r->mode = req->mode;
    r->x = req->x;
    r->y = req->y;
    r->language = req->language;
    r->language_auto = req->language_auto;
    r->ignore_eol = req->ignore_eol;
    CopyMemory(r->hashes, req->hashes, sizeof r->hashes);
    if (req->mode == DIFF_UNSTAGED || req->mode == DIFF_UNTRACKED)
        file_stamp(req->root, req->path, worker_path, &r->file_time, &r->file_size);

    // Full context, so the whole file is shown with every line marked.
    args[0] = 0;
    append_wide(args, 4096, req->ignore_eol ? L"diff --ignore-cr-at-eol " : L"diff ");
    switch (req->mode) {
    case DIFF_UNTRACKED:
        append_wide(args, 4096, L"--no-index " DIFF_OPTIONS L" -- NUL");
        break;
    case DIFF_UNSTAGED:
        append_wide(args, 4096, DIFF_OPTIONS L" --");
        break;
    case DIFF_UNMERGED:  // against HEAD: the index holds several versions, which gives a combined diff
        append_wide(args, 4096, DIFF_OPTIONS L" HEAD --");
        break;
    case DIFF_STAGED:
        append_wide(args, 4096, DIFF_OPTIONS L" --cached ");
        append_wide(args, 4096, req->has_head ? L"HEAD" : EMPTY_TREE);
        append_wide(args, 4096, L" --");
        break;
    case DIFF_REFS:
        append_wide(args, 4096, DIFF_OPTIONS);
        append_quoted(args, 4096, req->base);
        if (req->target[0])
            append_quoted(args, 4096, req->target);
        append_wide(args, 4096, L" --");
        break;
    }
    if (req->orig_path[0])
        append_quoted(args, 4096, req->orig_path);
    append_quoted(args, 4096, req->path);

    char *out;
    size_t len;
    // --no-index exits with 1 when the files differ, which is the normal case there.
    if (git_run(&arena, req->root, args, req->mode == DIFF_UNTRACKED ? 3 : 1, &out, &len, r->error, 512) != 0)
        goto done;
    r->text = out;

    // Count lines first so the row and checkpoint arrays can be laid out one after the other.
    size_t line_cap = 1;
    for (size_t i = 0; i < len; i++) {
        if (out[i] == '\n')
            line_cap++;
    }
    r->lines = arena_push(&arena, line_cap * sizeof(DiffLine));
    r->checkpoints = arena_push(&arena, (line_cap / 1024 + 1) * sizeof(LineNumbers));

    unsigned old_no = 1, new_no = 1;
    int in_hunk = 0;
    const char *p = out, *end = out + len;
    while (p < end) {
        const char *line = p;
        while (p < end && *p != '\n')
            p++;
        size_t line_len = (size_t)(p - line);
        p++;
        if (line_len > 0 && line[line_len - 1] == '\r')
            line_len--;

        DiffLine row = {0, 0, LINE_CONTEXT};
        if (starts_with(line, "@@ -")) {
            const char *q = line + 4;
            unsigned hunk_old = parse_number(&q);
            if (*q == ',') {
                q++;
                parse_number(&q);
            }
            while (*q == ' ' || *q == '+')
                q++;
            unsigned hunk_new = parse_number(&q);
            // Hunk starts are 0 when that side is empty; the first real line is still line 1.
            hunk_old = hunk_old ? hunk_old : 1;
            hunk_new = hunk_new ? hunk_new : 1;
            in_hunk = 1;
            if (r->line_count == 0) {
                old_no = hunk_old;
                new_no = hunk_new;
                continue;
            }
            row.kind = LINE_GAP;
            row.offset = hunk_old;
            row.length = hunk_new;
        } else if (!in_hunk) {
            if (starts_with(line, "Binary files "))
                r->binary = 1;
            continue;
        } else if (line_len == 0 || line[0] == '\\') {
            continue;  // trailing empty line, "\ No newline at end of file"
        } else {
            row.kind = line[0] == '-' ? LINE_REMOVED : line[0] == '+' ? LINE_ADDED : LINE_CONTEXT;
            row.offset = (unsigned)(line + 1 - out);
            row.length = (unsigned)(line_len - 1);
            if (line_len - 1 > r->max_line_len)
                r->max_line_len = (unsigned)(line_len - 1);
            r->added += row.kind == LINE_ADDED;
            r->removed += row.kind == LINE_REMOVED;
        }
        if (r->line_count % 1024 == 0)
            r->checkpoints[r->line_count / 1024] = (LineNumbers){old_no, new_no};
        r->lines[r->line_count++] = row;
        diff_advance_numbers(&row, &old_no, &new_no);
    }
    r->max_line_no = old_no > new_no ? old_no : new_no;
    build_side_rows(&arena, r);
    compute_word_spans(&arena, r);
    if (r->language != LANG_PLAIN)
        compute_syntax_states(&arena, r);

done:
    r->arena = arena;
    r->elapsed_ms = GetTickCount() - started;
    return r;
}

void diff_advance_numbers(const DiffLine *line, unsigned *old_no, unsigned *new_no)
{
    switch (line->kind) {
    case LINE_CONTEXT: (*old_no)++; (*new_no)++; break;
    case LINE_REMOVED: (*old_no)++; break;
    case LINE_ADDED: (*new_no)++; break;
    case LINE_GAP: *old_no = line->offset; *new_no = line->length; break;
    }
}

void diff_line_numbers(const DiffResult *d, int row, unsigned *old_no, unsigned *new_no)
{
    int i = row / 1024 * 1024;
    *old_no = d->checkpoints[i / 1024].old_no;
    *new_no = d->checkpoints[i / 1024].new_no;
    for (; i < row; i++)
        diff_advance_numbers(&d->lines[i], old_no, new_no);
}

static DWORD WINAPI worker(void *unused)
{
    (void)unused;
    DWORD refresh_at = 0;
    int refresh_due = 0;
    for (;;) {
        HANDLE waits[2] = {request_event, watch_overlapped.hEvent};
        DWORD timeout = INFINITE;
        if (refresh_due) {
            int left = (int)(refresh_at - GetTickCount());
            timeout = left > 0 ? (DWORD)left : 0;
        }
        DWORD woke = WaitForMultipleObjects(watch_dir != INVALID_HANDLE_VALUE ? 2 : 1, waits, FALSE, timeout);

        if (woke == WAIT_OBJECT_0 + 1) {
            // Refresh once the folder has been quiet for a moment, not on every saved file.
            DWORD bytes = 0;
            GetOverlappedResult(watch_dir, &watch_overlapped, &bytes, FALSE);
            if (watch_changes_matter(bytes)) {
                refresh_due = 1;
                refresh_at = GetTickCount() + REFRESH_DELAY_MS;
            }
            watch_issue();
            continue;
        }
        if (woke == WAIT_TIMEOUT) {
            refresh_due = 0;
            if (worker_status.folder[0] && !worker_status.target[0])  // two refs don't change with the folder
                PostMessageW(notify_hwnd, WM_APP_STATUS, 0, (LPARAM)load_status(&worker_status, 1));
            continue;
        }

        EnterCriticalSection(&request_lock);
        int do_status = status_pending, do_diff = diff_pending, do_refs = refs_pending;
        status_pending = diff_pending = refs_pending = 0;
        if (do_status)
            worker_status = request_status;
        if (do_diff)
            worker_diff = request_diff;
        if (do_refs)
            worker_refs = request_refs;
        LeaveCriticalSection(&request_lock);
        if (do_status) {
            refresh_due = 0;
            PostMessageW(notify_hwnd, WM_APP_STATUS, 0, (LPARAM)load_status(&worker_status, 0));
        }
        if (do_refs)  // before the diff: the dropdown is waiting on it, and it is quick
            PostMessageW(notify_hwnd, WM_APP_REFS, 0, (LPARAM)load_refs(&worker_refs));
        if (do_diff)
            PostMessageW(notify_hwnd, WM_APP_DIFF, 0, (LPARAM)load_diff(&worker_diff));
    }
}

void git_start(HWND notify)
{
    notify_hwnd = notify;
    InitializeCriticalSection(&request_lock);
    request_event = CreateEventW(NULL, FALSE, FALSE, NULL);
    watch_overlapped.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
    HANDLE thread = CreateThread(NULL, 0, worker, NULL, 0, NULL);
    if (!request_event || !watch_overlapped.hEvent || !thread)
        fatal(L"Starting the git worker thread");
    CloseHandle(thread);
}

void git_request_status(const wchar_t *folder, const wchar_t *base, const wchar_t *target)
{
    EnterCriticalSection(&request_lock);
    copy_wide(request_status.folder, 1024, folder);
    copy_wide(request_status.base, 256, base);
    copy_wide(request_status.target, 256, target);
    status_pending = 1;
    LeaveCriticalSection(&request_lock);
    SetEvent(request_event);
}

void git_request_refs(const wchar_t *root, int generation)
{
    EnterCriticalSection(&request_lock);
    copy_wide(request_refs.root, 1024, root);
    request_refs.generation = generation;
    refs_pending = 1;
    LeaveCriticalSection(&request_lock);
    SetEvent(request_event);
}

static int diff_mode(const FileEntry *f, const ListRow *row)
{
    return row->type == ROW_REF      ? DIFF_REFS
           : row->type == ROW_STAGED ? DIFF_STAGED
           : f->kind == '?'          ? DIFF_UNTRACKED
           : f->kind == 'u'          ? DIFF_UNMERGED
                                     : DIFF_UNSTAGED;
}

// FileEntry.hashes as a NUL-terminated copy of at most 129 chars (two SHA-256 blobs and a space).
static void copy_hashes(char *dst, const FileEntry *f)
{
    int n = f->hashes_len < 129 ? f->hashes_len : 129;
    for (int i = 0; i < n; i++)
        dst[i] = f->hashes[i];
    dst[n] = 0;
}

// Whether `d` still shows `row` of a newer list: same status letters, same HEAD/index blobs and an
// unchanged working-tree file. Diffs against refs and conflicts are always reloaded.
int diff_is_current(const DiffResult *d, const StatusResult *status, const ListRow *row)
{
    static wchar_t path[1024], buf[2048];  // UI thread only
    static char hashes[130];
    const FileEntry *f = &status->files[row->file];
    int mode = diff_mode(f, row);
    if (d->error[0] || mode != d->mode || f->x != d->x || f->y != d->y)
        return 0;
    if (mode != DIFF_UNSTAGED && mode != DIFF_STAGED && mode != DIFF_UNTRACKED)
        return 0;
    copy_hashes(hashes, f);
    if (lstrcmpA(hashes, d->hashes) != 0)
        return 0;
    if (mode == DIFF_STAGED)
        return 1;  // HEAD and index blobs are all it shows
    int n = utf8_to_wide(f->path, lstrlenA(f->path), path, 1023);
    path[n] = 0;
    ULONGLONG time, size;
    file_stamp(status->root, path, buf, &time, &size);
    return time == d->file_time && size == d->file_size;
}

void git_request_diff(const StatusResult *status, const ListRow *row, int generation, int language, int ignore_eol)
{
    const FileEntry *f = &status->files[row->file];
    int mode = diff_mode(f, row);
    EnterCriticalSection(&request_lock);
    DiffRequest *req = &request_diff;
    copy_wide(req->root, 1024, status->root);
    copy_wide(req->base, 256, status->base);
    copy_wide(req->target, 256, status->target);
    int n = utf8_to_wide(f->path, lstrlenA(f->path), req->path, 1023);
    req->path[n] = 0;
    // The rename source only exists on the side that has the rename (index or the base ref).
    int with_orig = f->orig_path && (mode == DIFF_STAGED || mode == DIFF_REFS);
    n = with_orig ? utf8_to_wide(f->orig_path, lstrlenA(f->orig_path), req->orig_path, 1023) : 0;
    req->orig_path[n] = 0;
    req->mode = mode;
    req->has_head = status->has_head;
    req->generation = generation;
    req->language = language == LANG_AUTO ? syntax_language_for_path(f->path) : language;
    req->language_auto = language == LANG_AUTO;
    req->ignore_eol = ignore_eol;
    req->x = f->x;
    req->y = f->y;
    copy_hashes(req->hashes, f);
    diff_pending = 1;
    LeaveCriticalSection(&request_lock);
    SetEvent(request_event);
}

void status_free(StatusResult *r)
{
    Arena a = r->arena;
    arena_free(&a);
}

void diff_free(DiffResult *r)
{
    Arena a = r->arena;
    arena_free(&a);
}

void refs_free(RefList *r)
{
    Arena a = r->arena;
    arena_free(&a);
}
