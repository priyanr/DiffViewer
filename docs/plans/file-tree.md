# File tree view

## Goal
A **Flat | Tree** switch in the top bar shows the changed files as a folder tree. Folders can be collapsed and
expanded with the mouse or with Explorer-style keys, and single-child folder chains are shown as one row.

## Affected existing features
- **Top bar:** a new `[Flat | Tree]` pair right after the « list button, so the path box gets 110 px narrower
  (at 96 DPI). Flat is the default and the choice lasts for the session only, so the default view looks as it does
  today apart from the narrower path box.
- **File list (tree view only):** STAGED / UNSTAGED headers stay, each with its own tree. Folders come first, then
  files, in git's byte order. File rows show the badge and the name only (the folder is the tree), indented by depth.
- **List keys / mouse (tree view only):** Up/Down/PgUp/PgDn/Home/End also stop on folder rows. Selecting a folder
  requests no diff, and the diff pane keeps the last file. Prev/Next (N/P/F7) moving to another file skips folders.
  Stage 2 adds Left/Right/Enter and clicking to collapse.
- **Auto-refresh:** keeps the selected folder or file, and from stage 2 also keeps collapsed folders.
- **Flat view:** behaviour unchanged. `ListRow` grows from 8 to 16 bytes, about 16 KB more per 1,000 rows.

## Design
- **`ListRow` (src/app.h)** gets new fields: `type` becomes `unsigned char`; plus `depth`, `collapsed`,
  `name_start`/`name_len` (a folder's name inside the path of `file`, which is a file under that folder) and `end`
  (first row after the folder's subtree). A folder row keeps its group's `type` and sets a `folder` flag (instead of a new `ROW_FOLDER`), so refresh and toggle can match rows by group. Folder names point into git's output, so no strings are
  copied.
- **`StatusResult`** gets `tree_rows`, `tree_count`, built on the worker in the same arena right after `rows`, for
  both working tree and ref compare. Always built, because the toggle should be instant and it costs very little.
- **Build (src/git.c, `build_tree`)**, for each group (or the whole list in ref compare):
  1. Fill an int array with the group's file indices (arena) and sort it by path with a small bottom-up merge sort
     and a byte compare. This is needed because a folder's files have to be next to each other, and git lists
     untracked files after tracked ones. There's no CRT (`-nostdlib`), so `qsort` isn't available.
  2. A recursive `tree_add(range, prefix_len, depth)`. First pass: for each distinct next path component that has a
     `/` after it, add a folder row. Its name keeps growing while every file in its range shares the next component
     too (compact chains). Then recurse into its range and set `end`. Second pass: add the direct files as rows.
     Recursion depth = path depth.
- **UI (src/main.c):** `app.tree` (0/1) picks `rows` or `tree_rows` through two small accessors (`row_at(i)`,
  `row_count()`), which replace the direct `app.status->rows[...]` uses. New `app.diff_sel` = the row whose diff is
  open (equal to `app.sel` unless a folder is selected). `reload_diff` and the refresh logic use it.
  - Toggle: map `sel`/`diff_sel` into the other array by (type, file) through the shared `files[]` indices, with no
    string compares. A folder selected when switching to flat maps to `diff_sel`.
  - Paint: a folder row shows ▾/▸ and the folder name (compact chains as `a/b/c`) in `dim`, and a file row shows
    the badge and the name in `fg`. Indent = `depth * scale(14)`. Same visible-rows loop, same theme colors.
- **Stage 2 collapse:** `app.shown` = an int array (from the status arena, one int per tree row) listing the indices
  of the visible tree rows. It's rebuilt in one O(n) walk only on toggle, collapse/expand and a new status, never in
  paint. Collapsed folders skip `[i+1, end)`. `app.sel` is a list position; `app.diff_sel` indexes the view's rows, so
  the open file is tracked while its folder is collapsed. Clicking a folder row selects and toggles it. In flat view `shown` is the identity. Collapsing a folder that holds
  the selection moves the selection to that folder. On auto-refresh, collapsed state is copied over by (group, folder
  path).

## Stages
### Stage 1: tree view, everything expanded
- Behaviour: the Flat | Tree switch works. Tree view shows the folder tree (compact chains, folders first) under the
  STAGED/UNSTAGED headers and in ref compare. Folder rows can be selected with the mouse and Up/Down, and show no
  diff. Prev/Next across files skips folders. The selection survives toggling and auto-refresh.
- How to check: run `build\DiffViewer.exe .` on this repo (with a few staged and untracked files in nested folders,
  e.g. `src/` plus a new `docs/plans/x/y.md`). Toggle, click folders and files, use Up/Down and N/P. Take a
  screenshot with `control-exe`. Also check ref compare (`HEAD~5` → empty) and a repo with no changes.
- New files/structs: none. `ListRow` gets fields (stated above).
- Files touched: `src/app.h`, `src/git.c`, `src/main.c`, `docs/history.md`.
- [x] done

### Stage 2: collapse / expand
- Behaviour: clicking a folder's ▸/▾ or row, or pressing Enter, toggles it. Left collapses, or goes to the parent
  folder; Right expands, or goes to the first child. Collapsed folders stay collapsed across auto-refresh and
  flat ↔ tree toggling.
- How to check: collapse a folder with the selected file inside, expand it again, use Left/Right on nested and
  compact folders, touch a file to trigger auto-refresh and confirm collapsed folders stay collapsed. Scroll a long
  tree.
- New files/structs: none (`app.shown` is an int array in the status arena).
- Files touched: `src/main.c`, `src/git.c` (arena space for `shown`), `docs/history.md`.
- [x] done

## Out of scope
Remembering flat/tree or collapsed state between runs, a keyboard shortcut for the toggle, folder file counts or
folder-level diffs, Expand all / Collapse all, case-insensitive sorting.

## Verification
- `build.bat` passes with no warnings. `build\DiffViewer.exe --perf .` stays within private memory ≤ 4 MB and
  startup ≤ 100 ms (the Stop hook checks both), and peak working set shouldn't move, since no new DLL is loaded.
- Manual checks as in each stage's "How to check", plus `control-exe` screenshots in light and dark themes.
