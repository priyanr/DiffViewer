# DiffViewer v1

## Goal
Open a git repo folder and see its changed files (added / modified / deleted / untracked / renamed) and each file's
diff side by side or unified, with C# highlighting, in an app that stays under 4 MB private memory and paints in
under 100 ms. (Changed from "10 MB working set" after stage 1: a blank Win32 window already has ~9 MB working set
of shared system DLLs; private memory is the app's own cost and what Task Manager shows.)

## Affected existing features
None (new app).

## Design
- **Four source files**, split by job:
  - `src/main.c`: `wWinMain`, window procs, layout, painting, theme, DPI, `--perf`.
  - `src/git.c`: run `git.exe` on the worker thread, parse `status --porcelain=v2 -z` and unified diff output, folder watcher.
  - `src/cs.c`: C# tokenizer for one line, given the lexer state at the start of the line.
  - `src/app.h`: the few structs and functions shared between them (span, arena, file entry, diff line, messages).
- **No diff algorithm of our own.** `git diff -U999999 --histogram` already returns the whole file with every line
  marked ` `, `-` or `+`. Unified view draws that as-is; side-by-side pairs each `-` block with the following `+`
  block. Untracked files use `git diff --no-index NUL <file>`. One output buffer per open diff; lines are spans into it.
- **Memory:** one arena per open diff, freed at once when another file is selected. File list is one buffer of
  NUL-separated paths from git plus a fixed-size entry array. No common controls (comctl32 not loaded): the file
  list and diff panes are custom-drawn windows; the only standard control is an `EDIT` box.
- **Highlighting:** on load, one linear pass stores a 1-byte lexer state per line (in a block comment, verbatim
  string, raw string, ...). Painting tokenizes only the visible lines starting from that state.
- **Startup:** window shows and paints immediately; `git status` runs on the worker thread and fills the list when
  done. `--perf` reports `private_kb` (after status and the first diff are loaded) and `startup_ms` (process
  creation → first paint), which the gate checks, plus `peak_ws_kb` and `status_ms` for information.

## Stages
### Stage 1: window + changed-file list
- Behaviour: `DiffViewer.exe <repo>` opens a themed (dark/light from Windows), DPI-aware window with the repo path in
  an edit box (type a path + Enter to switch) and a list of changed files (working tree vs HEAD) with A/M/D/U/R badges.
  Not a repo / git missing shows a message in the list. `--perf` works.
- How to check: open this repo with an added, a modified and a deleted file; screenshot. Run `--perf .`.
- New files/structs: `build.bat`, `app.manifest` + `app.rc` (per-monitor DPI v2, UTF-8 code page), `src/main.c`,
  `src/git.c`, `src/app.h`, `.gitignore`; structs `Arena`, `FileEntry`, `StatusResult`, `Theme` (`Span` moved
  to stage 2, where it is first used).
- Files touched: none existing.
- Result: 20 KB exe, no C runtime. `--perf`: private 1.2 MB, peak WS 9.4 MB, startup 43–70 ms, status 125–280 ms.
- [x] done

### Stage 2: unified diff of the selected file
- Behaviour: selecting a file shows its diff against HEAD in the right pane: old/new line numbers, added/removed
  line backgrounds, mouse-wheel / keyboard / scrollbar scrolling over any file size.
- How to check: a 20k-line file scrolls smoothly; memory stays in budget.
- New files/structs: `DiffLine` (8 bytes: offset + length + kind; line numbers are not stored per row),
  `LineNumbers` (old/new number of every 1024th row, so painting never walks the whole file), `DiffResult`
  (arena + rows of one diff), `DiffRequest` (worker's copy of what to diff).
- Files touched: `src/main.c`, `src/git.c`, `src/app.h`, `.gitignore` (`__pycache__/`).
- Result: 29 KB exe. 20k-line file: diff 93–109 ms, private 2.7 MB. `--perf .`: private 1.3 MB, startup 46–73 ms.
  Checked: modified, deleted, untracked, Unicode path, rename (100% and edited), binary, repo without commits.
  Not exercised: the hunk-gap row (only appears in files over 999,999 lines). Lines over 8,192 characters are
  cut off on screen.
- [x] done

### Stage 3: side-by-side view + toggle
- Behaviour: diff pane shows old on the left and new on the right, aligned, one scrollbar; `Tab` (and a button)
  toggles to unified.
- How to check: modified block with more removed than added lines aligns with blank filler rows.
- New files/structs: `SideRow` (left line index, right line index), `NumberCursor` (line numbers for side cells
  without re-walking from a checkpoint per cell).
- Files touched: `src/main.c`, `src/git.c`, `src/app.h`.
- Changed from plan: the toggle key is `U`, not `Tab` (Tab already cycles focus); plus a clickable
  "Unified | Side by side" switch in the top bar. Side by side is the default.
- [x] done

### Stage 4: C# highlighting
- Behaviour: `.cs` files highlighted in both views: keywords, contextual keywords, types, strings (regular, verbatim
  `@""`, interpolated `$""`, raw `"""`), chars, comments (line, block, `///` doc), numbers, preprocessor lines.
  Colors from the theme table, dark and light.
- How to check: a sample file with every token kind, incl. multi-line strings/comments scrolled into the middle.
- New files/structs: `src/cs.c` (one scanner, used for the load-time state pass and for painting), `CsToken`.
- Files touched: `src/main.c`, `src/git.c` (state pass, 2 bytes per row, old/new side), `src/app.h`, `build.bat`.
- Limits: no semantic model, so PascalCase names are coloured as types and `name(` as methods; interpolation holes
  are coloured as part of the string.
- [x] done

### Stage 5: staged vs unstaged, and ref vs ref
- Behaviour: file list has "Staged" and "Unstaged" groups (`git diff --cached` / `git diff`). A compare bar with two
  ref boxes (base, target): when filled, the list shows `git diff --name-status <base> <target>` and diffs
  come from `git diff <base> <target> -- <path>`. Bad ref shows git's message.
- How to check: stage half the changes; compare `HEAD~3` with `HEAD`; compare two branches.
- New files/structs: `ListRow` (a header or a file in one group), `StatusRequest`.
- Files touched: `src/main.c`, `src/git.c`, `src/app.h`.
- Changed from plan: groups appear only when something is staged (otherwise one flat list, as the index equals
  HEAD). Base ref with an empty target compares the base with the working tree (`HEAD` there gives the old
  "working tree vs HEAD" view). Conflicts show `!` instead of `C`. Refs starting with `-` are refused.
- [x] done

### Stage 6: auto-refresh
- Behaviour: `ReadDirectoryChangesW` on the repo (worker thread), debounced 300 ms; ignores `.git/` except `index`
  and `HEAD`. List refreshes keeping the selection and scroll position; the open diff reloads if its file changed.
  Off in ref-vs-ref mode (refs don't change with the working tree).
- How to check: edit a file in another editor, save, see the diff update; `git add` moves it to Staged.
- New files/structs: none.
- Files touched: `src/git.c` (overlapped `ReadDirectoryChangesW` waited on by the same worker thread),
  `src/main.c`, `src/app.h`.
- Result: 49 KB exe. `--perf .`: private 1.3–1.4 MB, startup 40–49 ms. Idle: 0 git launches in 5 s, 0.05 s CPU.
  Checked: new file appears without input; selection and diff scroll (12 → 12) kept across a refresh; `git add`
  moves the file to Staged. Off when both refs are set.
- [x] done

### Stage 7 (added after v1): Prev / Next change
- Behaviour: Prev / Next buttons in the top bar (and `N` / `P`, `F7` / `Shift+F7`) scroll to the previous/next
  block of added/removed lines, 3 rows below the top; past the last/first block they select the next/previous file
  and open it on its first/last block. A block already on screen when a file opens counts as the current one.
- How to check: Next through a file with many changes, across files, Prev back; toggle the view in between.
- New files/structs: none (three `app` fields: `change_row`, `change_generation`, `change_dir`).
- Files touched: `src/main.c`, `README.md`.
- Result: 51 KB exe. `--perf .`: private 1.38 MB, startup 50–68 ms. Checked: first Next moves past a change at
  row 1; 13 Nexts through Big.cs; Program → gone → Service → Untracked → Naïve and Prev back; unified view keeps
  the current change. Not checked: the stale strip seen once at the bottom of the diff after switching to unified,
  where the horizontal scrollbar was (seen in a PrintWindow capture only; may be a capture artifact).
- [x] done

### Review fixes (after rules-review)
- Stale diff under an empty list: `clear_diff` bumps the diff generation, so a diff still loading is dropped.
- `@""""` read as a raw string: raw strings now need no `@` prefix (`cs.c`).
- Refs containing `"` or `\` are refused (they could end the quoting and add git arguments).
- Auto-refresh keeps the open diff when the file is unchanged, as stage 6 planned: the diff records the file's
  status letters, HEAD/index blob hashes (`FileEntry.hashes`) and working-tree write time + size;
  `diff_is_current` compares them. Ref compares and conflicts always reload.
- Checked: editing another file → no diff run (2 git calls: rev-parse + status); editing the open file → diff
  reloads with the new line; `@""""` line followed by correctly coloured code; `HEAD" "--output=…` refused, no
  file written. Not reproduced: the stale-diff race (fix is one line).

### Stage 8 (added after v1): resizable panes
- Behaviour: drag the strip between the file list and the diff (5 px, resize cursor) to change the list width;
  limits 120 px list / 200 px diff (at 96 DPI). Width is kept in 96-DPI units so it scales on a DPI change; not
  saved between runs (no saved settings).
- New files/structs: none (`app.list_w96`, `app.drag_dx`).
- Files touched: `src/main.c`, `README.md`.
- Checked at 125% DPI: wider, clamped at both limits.
- [x] done

### Stage 9 (added after v1): ref dropdown
- Behaviour: clicking a ref box, pressing `Down` / `Alt+Down` in it, or typing in it opens a themed dropdown under it
  listing `HEAD`, local and remote branches (newest commit first), tags (highest version first) and the last 30
  commits (short hash + subject). Typing filters it (case-insensitive, name or subject) and highlights the first match; `Up`/`Down`/
  `PgUp`/`PgDn` move, `Enter` or a click picks the ref and reloads, `Esc` closes. Enter with nothing highlighted
  uses the typed text, as before. Refs are read by the worker (`git for-each-ref`, `git log -n 30`) each time the
  dropdown opens; the window is created on first use, so startup is unchanged. A target ref without a base ref
  now shows a message instead of being ignored.
- How to check: open in a repo with branches, tags and remotes; filter, pick with keys and mouse; Esc; click
  elsewhere closes it; dark and light; `--perf` unchanged.
- New files/structs: `RefEntry`, `RefList` (`app.h`), `RefsRequest` (`git.c`).
- Files touched: `src/main.c`, `src/git.c`, `src/app.h`, `README.md`.
- Changed from plan: tags are sorted by version (`-v:refname`), not date. Sorting by date reads every tag
  object (0.5 s for 2,000 tags, against 0.06 s by name), so it is a separate `for-each-ref` call from the
  branches.
- Result: 64 KB exe. `--perf .`: private 1.38–1.46 MB, startup 40–56 ms. Checked at 125% DPI, light theme:
  - opening: click, `Down`, `Alt+Down`
  - filtering, with the exact match highlighted
  - picking by keyboard (a commit, a branch, a tag) and by mouse click; hover highlight; wheel; PgDn
  - `Esc`; clicking the list closes it
  - "No matching refs." with Enter using the typed text (git's "bad revision")
  - the target-only message
  - 2,000 tags
- Not checked:
  - Dark theme. It uses the same theme colours as the list.
  - A real screen, since the screen was locked and captures were made with PrintWindow. In some captures the
    status bar lagged one step behind the list, but no repaint was pending and identical runs differed, so it is
    probably a capture artifact, like stage 7's stale strip.
- [x] done

## Out of scope
Editing, staging or any command that changes the repo; languages other than C#; word-level (intra-line) diff;
binary/image diffs (shown as "binary file"); search; saved settings; a folder-picker dialog (loads shell/COM and
costs ~10 MB; path comes from the command line or the edit box).
