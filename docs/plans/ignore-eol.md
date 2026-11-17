# Ignore line endings

## Context
In a C# repo, a test file showed as entirely changed. It was committed with LF line endings and an editor
rewrote it with CRLF. `git diff` sees 2261 changed lines, but with `--ignore-cr-at-eol` the
real change is 59 added and 2 removed. The user wants a checkbox in the status bar to ignore line-ending changes.

Decisions already made by the user:
- A checkbox at the bottom (status bar), **ticked by default**, not remembered between runs.
- With it ticked, a file whose only change is line endings shows "Only line endings changed." in the diff pane.

## Goal
Diffs hide changes that are only CR at the end of a line, so a re-saved file shows its real edits. Unticking the
box shows line-ending changes again.

## Affected existing features
- **Diff (default changes):** diffs now ignore CR-at-end-of-line differences by default. A file whose line endings
  changed shows only its real edits, or "Only line endings changed." The file list is unchanged: git status still
  lists the file as modified.
- **Status bar:** gains an "Ignore line endings" checkbox to the left of the highlighter field. The left-hand text
  is clipped a little earlier.

## Design
- **Request → git:**
  - `DiffRequest` and `DiffResult` (`src/git.c`, `src/app.h`) get `int ignore_eol`.
  - `git_request_diff(..., language, ignore_eol)` gets one more argument; there are two call sites in `main.c`.
  - In `load_diff` (`src/git.c:735`), when `ignore_eol` is set, ` --ignore-cr-at-eol` is appended right after
    `DIFF_OPTIONS` for every mode. It is a read-only diff option, like the ones already there.
- **Empty diff:** `diff_paint` already shows "No content changes." when `line_count == 0`
  (`src/main.c:636`). It shows "Only line endings changed." instead when `d->ignore_eol`.
- **Checkbox (`main.c`):**
  - `app.ignore_eol = 1` at startup.
  - `app.eol_field` is a fixed-width rect (≈170 px at 96 DPI), set in `layout()` to the left of
    `app.language_field`, as `language_field` is.
  - It is drawn in `main_paint`: a small square with `FrameRect(app.border_brush)`, filled inside when ticked,
    then the label "Ignore line endings", in `t->fg`. It is always shown once a repo is open.
    *Changed while building:* the ✓ glyph raised peak working set by ~90 KB, because the UI font lacks it and GDI
    maps in a fallback font. A filled square costs nothing.
  - A click in `WM_LBUTTONDOWN` toggles it. If a diff is open, the same row is re-requested with
    `keep_scroll_generation`, the same path the highlighter pick uses (`language_menu`).
  - The status text clip moves to `eol_field.left`.
- No new GDI objects: it uses the existing `border_brush`, `ui_font` and `fill`.

## Stages
### Stage 1: checkbox end to end
- Behaviour: ticked by default; the CRLF-rewritten file shows only its real changes, and a CRLF-only file shows
  "Only line endings changed." Unticking shows every line changed again and keeps the scroll position.
- How to check: in `scratchpad\langrepo`, convert a committed file to CRLF (with and without a real edit) and
  compare with `git diff --ignore-cr-at-eol --stat`.
- New structs: none. New fields: `ignore_eol` on the request, result and app; `eol_field` rect.
- Files touched: `src/app.h`, `src/git.c`, `src/main.c`.
- [x] done (checked with the harness over the real `load_diff`; the checkbox itself not yet seen on screen)

## Out of scope
- Ignoring other whitespace (`-w`).
- Remembering the choice.
- Hiding line-ending-only files from the file list.
- Telling a mode-only change apart from a line-ending-only change in the empty-diff message (rare on Windows,
  where `core.fileMode` is off).

## Verification
- `build.bat`, then `--perf .`; private_kb stays near 1384 KB.
- Use the harness (`scratchpad\harness\harness.c` calling the real `load_diff`) with `ignore_eol` 0 and 1 on the
  CRLF test files: line counts and added/removed should match git's.
- On screen: the checkbox look and the click, only when you say the screen is free.
- The plan is copied to `docs/plans/ignore-eol.md`.
