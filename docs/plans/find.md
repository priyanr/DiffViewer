# Text selection + find in current file

## Goal
Select text in the diff with the mouse and copy it (Ctrl+C). Ctrl+F opens a find bar, pre-filled from the selection; Next/Prev (buttons, Enter/Shift+Enter, F3/Shift+F3) step through matches, which are highlighted.

## Affected existing features
- **Diff pane mouse:** left click today only focuses the pane. Now click sets the caret, drag selects, double-click selects a word. The IBEAM cursor is already there.
- **Diff pane keys:** Ctrl+C (copy) and Ctrl+A (select all) are new; unused today. Plain `N`/`P`/`U`/F7 are unchanged. Ctrl+F and F3 are unused today.
- **Layout:** while the find bar is open the diff pane is one bar-height shorter. Closed = identical to today.
- **View toggle / file change / refresh:** the selection is cleared on Unified <-> Side by side, on a new file and on a reloaded diff (rows move). Today nothing is cleared because nothing is selected.
- **Tab order:** `move_focus` also visits the find box while the bar is open.
- **Theme:** new entries `find_bg`, `find_current_bg`; the selection reuses the list's existing selection color (`sel_bg`-style entry) so no extra color for it.

## Design
### Selection
- State: `app.sel_side` (0 old/left or unified, 1 new/right), `app.sel_anchor` and `app.sel_caret` as (row, byte offset) in the current view, `app.sel_active`.
- Hit test: click y -> row (`diff_top + y / code_row_h`); click x -> side (side-by-side: left/right half, a drag stays in the side where it began; unified: whole row) -> byte offset via binary search over the line's wide chars with `GetTabbedTextExtentW` (same tab stops as `draw_code`), then back to bytes. One line measured per mouse event.
- Drag: `SetCapture` on `WM_LBUTTONDOWN`, `WM_MOUSEMOVE` updates the caret and scrolls if the mouse is above/below the pane, `WM_LBUTTONUP` releases. Double-click: word (identifier run / other-byte run), as the word-highlight tokenizer defines words.
- Paint: in `draw_code`, next to the word-span fill, fill the selected part of that line (rows between anchor and caret; first/last rows partial). Only visible rows are touched.
- Copy: Ctrl+C builds the text from the selected lines (UTF-8 spans in `d->text` -> `MultiByteToWideChar` into one `GlobalAlloc` block, lines joined by `\r\n`), `SetClipboardData(CF_UNICODETEXT)`. Side-by-side copies only the side that was selected; filler rows are skipped. Unified copies the code text only, not the +/- marker or line numbers. Ctrl+A selects the whole file for the active side.
- Limit: a line is converted at most up to the same 8192 bytes `draw_code` already caps at; the copy buffer is allocated per copy and freed after, not kept.

### Find
- Bar: a strip between the top bar and the diff pane: `[ find box ........ ] 3 / 17  [▲ Prev] [▼ Next] [✕]`. One more child EDIT (same subclass `edit_proc` as the ref boxes), buttons drawn with `paint_pair` and hit-tested in `main_proc` `WM_LBUTTONDOWN` like the change Prev/Next. No new window class.
- Keys in the message loop in `run()` (before `TranslateMessage`, since focus may be in the list, diff or an edit): Ctrl+F opens/focuses the bar and selects the box text; F3 / Shift+F3 step. In `edit_proc` for the find box: Enter = next, Shift+Enter = prev, Esc = close and focus the diff.
- Seed (suggestion 2): on Ctrl+F, if a selection exists and is on one line, its text (cut at 255 bytes) goes into the box; otherwise the box keeps the last query.
- Search: over the diff's lines (`d->lines` spans into `d->text`), the whole file, since the diff includes it. Case-insensitive ASCII fold on UTF-8 bytes; the query is converted once with `WideCharToMultiByte`. A match = (line, byte offset). No allocation, no copy.
- Next/Prev: current match stored as (line, offset); scan forward/backward, wrap around. Removed-line and added-line twins are both matches (that is what is on screen). Line -> row in the current view, then `diff_scroll_to` so the match sits a few rows below the top; horizontal scroll only if it is off-screen.
- Count `n / total`: recounted when the query changes or a new diff arrives (one linear pass). `No results` in the dim color.
- Highlight: in `draw_code`, matches in visible lines use `find_bg`, the current one `find_current_bg`.
- Query change: jump to the first match at/after the top of the screen. File change: keep the query and the bar open; no auto jump, the count updates.

## Stages
### Stage 1: select and copy
- Behaviour: click/drag/double-click selects in both views; selection is painted; Ctrl+C / Ctrl+A work; selection clears on view toggle, file change and refresh.
- How to check: open a repo with a changed file; drag across lines in unified and in side by side (both sides); double-click a word; paste into Notepad; switch view; scroll while dragging.
- New files/structs: none (`src/main.c`, state in `app`).
- Files touched: `src/main.c`.
- [x] done

### Stage 2: find bar, step
- Behaviour: Ctrl+F opens the bar (seeded from a one-line selection); typing finds the first match and scrolls to it; Enter / F3 / Next and Shift+Enter / Shift+F3 / Prev wrap around; Esc / ✕ close; count shown.
- How to check: Ctrl+F, a word that occurs several times, F3 / Shift+F3 / buttons, both views, select a word then Ctrl+F.
- New files/structs: none.
- Files touched: `src/main.c`.
- [x] done

### Stage 3: highlight matches
- Behaviour: every visible match gets a background, the current one a stronger one, in both themes.
- How to check: search a common word, scroll, dark and light, both views.
- Files touched: `src/main.c` (`draw_code`, `Theme` and both tables).
- [x] done

## Further suggestions (not in this plan)
1. **Match case toggle** (`Aa`): small, after stage 3.
2. **Search across all files in the list**: a different feature (needs every diff loaded).
3. **Match marks on the scroll bar**: costs a pass over all matches.
4. **Whole word / regex**: skipped, adds options nobody asked for.
5. **Select in the file list / copy path**: separate.

## Out of scope
Replace, regex, cross-file search, keyboard selection (Shift+arrows; the diff pane has no caret), drag-and-drop of text.

## Verification
Each stage: `build.bat` clean; `DiffViewer.exe --perf .` inside budget (private ≤ 4 MB, start ≤ 100 ms); run the app (`control-exe`) and check the stage's "How to check", noting what could not be checked (screenshots are blank per `docs/future.md`). Save this plan as `docs/plans/find.md`; list affected existing features in `docs/history.md`.
