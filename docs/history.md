# History

## 2026-11-27: file tree view, stage 1 (plan: `docs/plans/file-tree.md`)

Existing features affected:
- **Top bar:** a `[Flat | Tree]` switch right after the « button; the path box is 110 px (96 DPI) narrower. Flat is
  the default and the choice lasts for the session.
- **File list, tree view only:** folders (single-child chains as one row, e.g. `src/app/models`) come first, then
  files, under the STAGED / UNSTAGED headers; ref compare is one tree. File rows show the badge and name only.
- **List keys / mouse, tree view only:** Up/Down/PgUp/PgDn/Home/End and clicks also select folder rows; a selected
  folder opens no diff and the last file stays open. Prev/Next (N/P/F7) to another file skips folders.
- **Auto-refresh:** keeps a selected folder too, along with the open file.

## 2026-11-18: resizable side-by-side halves, stage 2 of full-fit (plan: `docs/plans/full-fit.md`)

Existing features affected:
- **Side-by-side mouse:** the line between the halves can be dragged (cursor ↔ within a few pixels of it); a press
  there drags instead of starting a selection, and a double-click resets 50/50 instead of selecting a word. Dragging
  a half narrower than 60 px (96 DPI) collapses it, so the other side fills the pane.
- **Keys:** Ctrl+1 shows only the old side, Ctrl+2 only the new side, Ctrl+0 both at 50/50. Unused before; they do
  nothing in Unified.
- **Side-by-side horizontal scroll:** the page size is the wider half (was exactly half).
- The split is a share of the pane, so it follows window resizes. Not saved across restarts.

## 2026-11-18: hide the file list, stage 1 of full-fit (plan: `docs/plans/full-fit.md`)

Existing features affected:
- **Top bar layout:** a small button at the far left (`«` / `»`) hides and shows the file list; the path box starts
  one button-width further right.
- **Keys:** F11 does the same from anywhere. It was unused before.
- **Tab order:** while the list is hidden, Tab skips it. If the list had focus, focus moves to the diff.

## 2026-11-17: find bar, stage 2 of find (plan: `docs/plans/find.md`)

Existing features affected:
- **Diff pane layout:** while the find bar is open (Ctrl+F) the diff pane is one top-bar height shorter. Closed, nothing changes.
- **Keys:** Ctrl+F opens the bar, F3 / Shift+F3 step to the next / previous match, Enter / Shift+Enter in the box too,
  Esc in the box closes it. None of these were used before.
- **Tab order:** Tab also visits the find box while the bar is open.
- **Diff pane colors:** while the bar is open, find matches get a background (`find_bg`), the current one a stronger one
  (`find_current_bg`); a selection is drawn over them. Two new theme colors.

## 2026-11-17: text selection, stage 1 of find (plan: `docs/plans/find.md`)

Existing features affected:
- **Diff pane mouse:** a click used to only focus the pane; now click sets the caret, drag selects (side by side: only
  the side where the drag began), double-click selects a word. Ctrl+C copies the selection as text, Ctrl+A selects all.
- **Selection is cleared** on Unified/Side by side, on a new file and on a reloaded diff (auto-refresh).
- **Diff pane painting:** each row is drawn into a one-row bitmap and copied over, so repainting while selecting does not
  flicker. Looks the same otherwise.

## 2026-11-17: ignore line endings (plan: `docs/plans/ignore-eol.md`)

Existing features affected:
- **Diff (default changed):** diffs ignore CR at the end of lines by default (`git diff --ignore-cr-at-eol`). A
  file re-saved with other line endings shows only its real edits, or "Only line endings changed." when that is
  all. Untick "Ignore line endings" in the status bar to see line-ending changes again.
- **Status bar:** gains the "Ignore line endings" checkbox, left of the highlighter field.

## 2026-11-17: syntax picker, stages 2–5 (plan: `docs/plans/syntax-picker.md`)

Existing features affected:
- **Diff pane:** files with these extensions are now colored automatically, where before they were plain:
  C/C++, Go, Java, JavaScript, Kotlin, Rust, TypeScript, Zig, Python, Shell, PowerShell, SQL, YAML, HTML, XML,
  CSS, JSON. The existing 8 syntax colors are reused; `.cs` files are unchanged.
- **Status bar highlighter menu:** lists all of them.

## 2026-11-17: word highlighting in changed lines (plan: `docs/plans/word-highlight.md`)

Existing features affected:
- **Diff pane colors:** on a changed line paired with a similar line (Nth removed with Nth added in a block), the
  changed words get a stronger green/red background, in both views. Rows keep their usual green/red. Rewritten
  lines and unpaired lines look as before. Two new theme colors: `added_word_bg`, `removed_word_bg`.

## 2026-11-17: syntax picker, stage 1 (plan: `docs/plans/syntax-picker.md`)

Existing features affected:
- **Status bar:** the right end shows the open diff's highlighter ("Auto: C#", "Auto: Plain text", or the picked
  language). Clicking it opens a menu with Auto, Plain text and the languages. A pick lasts for the open file only.
- **C# highlighting:** moved from `src/cs.c` to `src/syntax.c`; colors and behaviour unchanged.

## 2026-11-17: memory usage in the status bar

Existing features affected:
- **Status bar:** now ends with the app's private memory (e.g. `· 1.5 MB`), the number Task Manager and the perf
  gate show. It is read each time the status bar is drawn, so it can be stale while the window sits idle.

## 2026-11-16: stages 3–6 (side by side, C# highlighting, staged/unstaged + refs, auto-refresh)

Existing features affected:
- **File list (stage 1):** when something is staged, files are grouped under STAGED and UNSTAGED, and a file with
  both kinds of change is listed twice. Conflicts show `!` instead of `C`.
- **Diff (stage 2):** an unstaged row now diffs the working tree against the index (`git diff`), not against HEAD;
  the same as before whenever nothing is staged. "Working tree vs HEAD" is still available: base ref `HEAD`, empty
  target.
- **Diff pane:** opens side by side instead of unified; `U` or the top-bar switch returns to unified.
