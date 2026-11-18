# Full-width diff + resizable side-by-side divider

## Context
Only the file-list / diff split can be dragged today. The user wants (a) a way to give the diff pane the whole
window width in both Unified and Side by side, (b) in Side by side, a draggable divider between the old and new
halves, and (c) a way to let one side (old or new) fill the diff pane.

## Goal
F11 or a top-bar button hides the file list so the diff fills the window; again brings it back at its old width.
In Side by side the middle divider can be dragged; dragging it to an edge collapses that side. Ctrl+1 shows only
old, Ctrl+2 only new, Ctrl+0 (or double-click on the divider) goes back to 50/50.

## Affected existing features
- **Top bar layout:** a new small square button at the far left (`«` while the list shows, `»` while hidden); the
  path box starts one button-width further right.
- **Keys:** F11, Ctrl+0, Ctrl+1, Ctrl+2 are new (unused today). Caught in the message loop in `run()` like Ctrl+F/F3,
  so they work wherever focus is.
- **Side-by-side pane mouse:** within a few pixels of the middle separator the cursor becomes ↔ and a press drags
  the divider instead of starting a selection; double-click there resets 50/50 instead of selecting a word.
- **Side-by-side horizontal scroll:** the page size becomes the wider visible half (today: exactly half).
- **Tab order:** while the list is hidden, Tab skips it (`move_focus` already skips hidden windows). If the list had
  focus when hidden, focus moves to the diff.
- Defaults unchanged: list shown, 50/50. Neither is saved across restarts (same as list width today).

## Design (all in `src/main.c`; no new files, structs or theme colors)
State in the `app` struct:
- `int list_hidden;` — file list hidden, diff full width.
- `int side_split;` — left half's share of the side-by-side pane in per-mille, 0..1000, default 500
  (ratio, so it follows window resizes).
- `int side_dragging;` — the middle divider is being dragged.
- `RECT list_button;` — top-bar button.
- Constant `MIN_SIDE_W = 60` (96-DPI): dragging a half narrower than this snaps it to 0 (collapsed).

Helpers (reused by paint, hit test, scroll, find — replaces the four `(c.right - 1) / 2` sites):
- `static int side_divider_x(int width)` → `MulDiv(width - 1, app.side_split, 1000)`. Left cell `[0, x)`,
  separator `[x, x+1)`, right cell `[x+1, width)`.

### Stage 1: hide the file list
- Behaviour: button / F11 toggles the list; diff pane and find bar take the full width in both views.
- Changes:
  - `list_width()` returns 0 and `split_rect()` an empty rect while `list_hidden` (so drag and the
    ↔ cursor on the list split go away by themselves).
  - `layout()`: place `app.list_button` at `pad` (square, bar-height − 2·pad), path box after it;
    `ShowWindow(app.list, list_hidden ? SW_HIDE : SW_SHOW)`; diff x = `list_w + (list_hidden ? 0 : split)`;
    find bar `x0` the same.
  - `static void toggle_list(void)`: flip, move focus to the diff if the list had it, `layout()`, invalidate main.
  - `main_paint`: draw the button like `find_close` (edit brush + border + `«`/`»` glyph).
  - `main_proc WM_LBUTTONDOWN`: `PtInRect(&app.list_button, p)` → `toggle_list()`.
  - `run()` loop: `VK_F11` → `toggle_list()`.
- How to check: open a repo, press F11 and click the button in both views; list hides/shows, diff refills,
  list width restored, Ctrl+F bar spans the full width, Tab skips the hidden list.
- [x] done

### Stage 2: draggable side-by-side divider + one-side full
- Behaviour: drag divider; snap-collapse at edges; Ctrl+1/2/0; double-click divider resets.
- Changes:
  - `diff_paint`: `half = side_divider_x(c.right)`; `paint_side_cell` returns early when `x1 <= x0`
    (collapsed side: otherwise its line number would draw over the other side).
  - `diff_hit`: same `half`; when one side is collapsed, `sel_side` is forced to the visible side.
  - `diff_scroll_to` page and `find_step` text width: the wider half, `max(x, c.right - 1 - x)`.
  - `diff_proc`:
    - `WM_LBUTTONDOWN` / `WM_LBUTTONDBLCLK` in side by side with `abs(x - divider) <= scale(3)`:
      double-click → `side_split = 500`; press → `side_dragging = 1`, `SetCapture`. Otherwise the existing
      selection code runs unchanged.
    - `WM_MOUSEMOVE` while `side_dragging`: `side_split = MulDiv(x, 1000, c.right - 1)`, clamp 0..1000, snap to
      0/1000 when a half is under `scale(MIN_SIDE_W)`; `diff_scroll_to` (page size changed) + invalidate.
    - `WM_CAPTURECHANGED` also clears `side_dragging`.
    - new `WM_SETCURSOR`: ↔ over the divider in side by side, else fall through to the class IBEAM.
  - `static void set_side_split(int per_mille)` (Ctrl keys + double-click): set, `sel_clear()` if the
    selected side got collapsed, `diff_scroll_to`, invalidate. Keys in `run()` loop: Ctrl+1 "old only" → 1000,
    Ctrl+2 "new only" → 0, Ctrl+0 → 500. Ignored in Unified.
- How to check: side by side, drag divider both ways, drag to each edge (collapses, ↔ still grabbable at the
  edge), Ctrl+1/2/0, double-click divider; select text in each half and in a lone half; resize the window
  (ratio kept); Unified unaffected.
- [x] done

After each stage: `build.bat`, `build\DiffViewer.exe --perf .` (budget: ≤4 MB private, ≤100 ms), launch with
`control-exe` for screenshots, tick the stage, add the `docs/history.md` entry listing the affected features above.

## Out of scope
- Saving list visibility / split ratio across restarts.
- Separate horizontal scroll per half (both halves keep scrolling together).
- A top-bar control for old/new-only (keys + drag only, as chosen).
- Hiding the top bar or status bar (true full screen).
