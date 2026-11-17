# Syntax highlighter picker + more languages

## Context
Only `.cs` files are colored today (`ends_with_cs` in `src/git.c:795` sets `DiffRequest.highlight_cs`; `src/cs.c`
scans C#). The user wants a menu to pick the highlighter, with automatic choice by default, and highlighters for
C/C++, JavaScript, TypeScript, Rust, Go, Zig and HTML, plus Python, Java, Kotlin, CSS, JSON, XML, Shell,
PowerShell, SQL and YAML. Private memory must not grow.

Decisions already made by the user:
- Picker: a field at the right end of the status bar ("Auto: Rust" / "Rust" when chosen by hand). Click opens a
  `TrackPopupMenu` popup.
- A hand-picked language applies to the current file only. Selecting another file goes back to Auto.
- Memory: `--perf` private_kb stays within noise of today's ~1.4 MB. Exe size may grow (expect +20–40 KB).

## Goal
The status bar shows which highlighter the open diff uses. Clicking it lets you pick another or Auto. Files of all
the listed languages are colored automatically by extension.

## Affected existing features
- **Status bar:** gains a clickable language field at the right end.
- **Diff pane:** files with the new extensions (`.c`, `.js`, `.py`, `.json`, `.xml`, ...) are now colored. Before,
  they were plain. `.cs` files look exactly as before. No new colors: every language maps onto the existing 8
  `TOK_*` kinds and `Theme.syntax[]`.
- **No shortcut, default or flow changes.**

## Design
- **`src/cs.c` → `src/syntax.c`** (git mv; the file is still untracked). It becomes the one highlighter file.
  Functions are renamed `cs_*` → `syntax_*`, and `CsToken` becomes `Token`. The C# code itself is unchanged.
- **`enum Language`** in `app.h`: `LANG_PLAIN, LANG_C, LANG_CS, LANG_JAVA, ...`, plus `LANG_AUTO` used only by the
  request.
- **`syntax_language_for_path(path)`** replaces `ends_with_cs`, a lowercase extension table, for example:
  - C/C++: `c h cc cpp cxx hpp hh hxx inl`
  - JS/TS: `js mjs cjs jsx` / `ts mts cts tsx`
  - Other C-family: `java`, `kt kts`, `go`, `rs`, `zig zon`
  - Script languages: `py pyw pyi`, `sh bash zsh`, `ps1 psm1 psd1`, `sql`, `yml yaml`
  - Markup and data: `html htm`, `xml xaml csproj vcxproj props targets config resx svg nuspec manifest`,
    `css scss less`, `json jsonc`
- **`syntax_scan_line(lang, s, len, state, tokens, cap, &count)`** is a `switch` on language that dispatches to one
  of four scanners. Each keeps the existing contract: a 1-byte state in and out, and tokens only when asked.
  1. **C#**: existing code.
  2. **C-family** (C/C++, Java, Kotlin, JS, TS, Go, Rust, Zig): one scanner driven by per-language flags. The flags
     cover:
     - `//` and `/* */` comments; Rust nested comments, depth kept in the state.
     - `#` preprocessor lines (C/C++).
     - Strings that can span lines, each tracked in the state:
       - backtick strings: JS/TS template, Go raw;
       - Rust `r#"…"#`, with the hash count kept in the state;
       - Java/Kotlin `"""` text blocks;
       - C++ `R"(…)"`.
     - Zig `\\` line strings, which end on their line.
     - Rust lifetimes (`'a`) versus char literals.
  3. **Hash-comment family** (Python, Shell, PowerShell, SQL with `--` and `/* */`, YAML):
     - Python `'''` and `"""`, PowerShell `<# #>` and `@" "@` span lines, tracked in the state.
     - YAML keys are colored as types.
  4. **Markup** (HTML, XML, CSS, JSON):
     - HTML/XML: `<!-- -->` and CDATA span lines. Tag name → keyword, attribute → type, value → string,
       entity → number.
     - CSS: selector → keyword, property → type, plus comments.
     - JSON: keys → type, strings, numbers, `true/false/null` → keyword.
- **No pointers in the keyword tables.** The exe has `.reloc` with ASLR, so any `const char *const[]` in `.rdata`
  is patched at load. That turns its pages into private copy-on-write memory, which is how const data could
  raise private_kb. Keyword lists are therefore `static const char name[][N]`: sorted, fixed width, binary
  searched with the existing `in_list`/`compare_word`. That includes the existing C# lists. The per-language
  choice of list is a `switch` in code, not a struct of pointers. Keyword lookup is skipped when `tokens == NULL`,
  so the whole-file state pass stays as fast as today.
- **Request/result:** `DiffRequest.highlight_cs` → `language` (`LANG_AUTO` or a pick). The worker resolves Auto
  with `syntax_language_for_path`, stores the resolved language and whether it was auto in `DiffResult`, and runs
  the existing `compute_cs_states` (renamed `compute_syntax_states`) unless the language is Plain. The UI's
  `draw_code` passes `d->language` to `syntax_scan_line`.
- **Picker UI (`main.c`):**
  - `app.lang_rect` is a fixed-width field (≈150 px at 96 DPI), right-aligned in the status bar and computed in
    `layout()`. It is shown only when a text diff is open.
  - Click in `main_proc` `WM_LBUTTONDOWN` → `CreatePopupMenu` with Auto, Plain text, a separator, then the
    languages A–Z, with a check on the current one. `TrackPopupMenu(TPM_RETURNCMD)`, then `DestroyMenu`.
  - Picking a language sets `app.language_pick` and re-requests the same row's diff through
    `git_request_diff(..., language)`. `keep_scroll_generation` is set (the existing path in `on_diff`,
    `main.c:1516`), so the scroll position is kept.
  - `list_select` resets `app.language_pick = LANG_AUTO`.
  - The menu is created on click and destroyed right after, so no GDI or user object stays alive.

## Stages
### Stage 1: picker end to end, with C# and Plain only
- Behaviour: a `.cs` diff shows "Auto: C#". Picking Plain text turns colors off and keeps the scroll position;
  picking Auto turns them back on. Other files show "Auto: Plain text". Selecting another file resets to Auto.
- How to check: open this repo; select `src/main.c`, `docs/history.md`, then a `.cs` file in another repo; use the
  popup on each.
- New files/structs: none. `cs.c` is renamed to `syntax.c` because it will hold every language.
  `enum Language` is new because the request, result and menu need to name the language.
- Files touched: `src/cs.c`→`src/syntax.c`, `src/app.h`, `src/git.c`, `src/main.c`, `build.bat` (file name).
- Also: the C# keyword tables are converted to pointer-free arrays, and keyword lookup is skipped in the state
  pass.
- [x] done (tokens checked with a harness over the real `syntax_scan_line`; not yet seen on screen)

### Stage 2: C-family scanner for C/C++, Java, JavaScript, TypeScript
- Behaviour: these files are colored automatically and can be picked from the menu.
- How to check: `src/*.c` and `src/app.h` in this repo; a JS/TS file with a multi-line template string; a C++ raw
  string.
- New files/structs: a flags value per language inside `syntax.c` (comment, string and preprocessor rules), so one
  scanner covers the family.
- Files touched: `src/syntax.c`, `src/app.h` (enum).
- [x] done (tokens checked with a harness over the real `syntax_scan_line`; not yet seen on screen)

### Stage 3: Go, Rust, Zig, Kotlin on the C-family scanner
- Behaviour: as stage 2 for these languages. Rust nested comments, raw strings and lifetimes; Go raw strings; Zig
  line strings; Kotlin text blocks.
- How to check: sample files in a scratch git repo, viewed as untracked files.
- Files touched: `src/syntax.c`, `src/app.h`.
- [x] done (tokens checked with a harness over the real `syntax_scan_line`; not yet seen on screen)

### Stage 4: hash-comment scanner for Python, Shell, PowerShell, SQL, YAML
- Behaviour: as above; multi-line Python and PowerShell strings and comments carry over lines.
- How to check: `.claude/hooks/done_check.py` in this repo; sample files in the scratch repo.
- Files touched: `src/syntax.c`, `src/app.h`.
- [x] done (tokens checked with a harness over the real `syntax_scan_line`; not yet seen on screen)

### Stage 5: markup scanner for HTML, XML, CSS, JSON
- Behaviour: as above; `app.manifest` (XML) and `.claude/settings.json` in this repo are colored.
- How to check: those two files plus an HTML and a CSS sample in the scratch repo.
- Files touched: `src/syntax.c`, `src/app.h`.
- [x] done (tokens checked with a harness over the real `syntax_scan_line`; not yet seen on screen)

Each stage: `build.bat` clean, `--perf .` within budget with private_kb compared to before the stage, a screenshot
check, ticked here, and an entry in `docs/history.md` for the affected features.

## Out of scope
- Detecting a language from file content or a shebang (extension only).
- Embedded languages: `<script>`/`<style>` inside HTML, SQL in strings.
- Remembering a pick between files or runs.
- New token colors.
- Semantic accuracy beyond a line lexer (types are guessed as today).
- Markdown, Ruby, PHP, Swift, Lua.

## Verification
- After each stage: `build.bat`, then `build\DiffViewer.exe --perf .`; private_kb must stay within ±16 KB of the
  stage-0 baseline (1392–1416 KB).
- Check that no `.rdata` pointer tables were added: `llvm-objdump -h` `.reloc` size stays about 0x104.
- Screenshot via the control-exe server (port 5000), using a screen capture of the window rect because
  `PrintWindow` returns blank for this app.
- The plan is copied to `docs/plans/syntax-picker.md` once approved.
