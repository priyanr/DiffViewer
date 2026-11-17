# Word highlighting in changed lines

## Context
Today a changed line is all red or all green (`paint_unified_row` / `paint_side_cell` in `src/main.c` fill
`added_bg`/`removed_bg`). For an agent's small edits you have to compare the two lines by eye. This feature marks
the exact changed words with a stronger color, as GitHub does.

Decisions already made by the user:
- Park the syntax picker (`docs/plans/syntax-picker.md`) after its stage 1. You check and commit that first; this
  feature starts from green. Picker stages 2–5 come after.
- Word level, several spans per line.
- Both views. Only paired lines that are mostly similar get word colors; a fully rewritten line keeps only today's
  row color.

## Goal
In the diff (side by side and unified), the changed words inside a changed line stand out, so a small edit is
visible at a glance.

## Affected existing features
- **Diff pane colors:** changed lines keep today's row background. The changed words on paired, similar lines get
  a stronger background. Two new colors go in each theme table: `added_word_bg` and `removed_word_bg`.
- Nothing else changes: no shortcut, layout or default changes.

## Design
- **Pairing:** the same as side by side. In `build_side_rows` (`src/git.c:494`), the Nth removed line of a block is
  paired with the Nth added line. Unified view uses the same pairs, keyed by line index, so both views get the
  same marks.
- **Computed on the worker** in `load_diff`, after `build_side_rows` and next to `compute_syntax_states`. A new
  `compute_word_spans(arena, r)` walks `r->side` rows that have both a left and a right line.
- **Storage:** `WordSpan { int line; unsigned short start, end; }` (8 bytes) in `app.h`, because `git.c` writes it
  and `main.c` reads it. `DiffResult` gets `WordSpan *word_spans; int word_span_count;`.
  - Spans are pushed in increasing line order into the diff's arena, so only changed lines cost memory and
    nothing is stored per line.
  - Byte offsets are clamped to 8192, the longest part of a line `draw_code` draws.
  - Freed with the diff, as everything else is.
- **Word split:** a token is one of:
  - a run of identifier characters (`[A-Za-z0-9_]` and UTF-8 bytes ≥ 0x80);
  - a run of whitespace;
  - a single other byte.
- **Diff of two lines:**
  1. Trim the common prefix and suffix of tokens.
  2. Run an LCS over the remaining tokens, capped at 64 tokens per side, with a `unsigned char[65][65]` table.
     *Changed in stage 1:* the token arrays and this table are worker-only statics (`worker_tokens`,
     `worker_lcs`, ~8 KB, touched only once a diff has paired lines), not stack buffers. A stack frame over 4 KB
     needs `__chkstk`, which only the C runtime provides, so the build failed.
  3. Removed tokens become spans on the old line and inserted tokens become spans on the new line. Adjacent
     spans merge, and a whitespace-only gap between two spans merges too, which reads better.
  4. Over the 64-token cap, the whole middle (between the common prefix and suffix) becomes one span.
- **Similarity skip:** if the unchanged non-whitespace bytes are less than half of the longer line's
  non-whitespace bytes, the pair gets no spans.
- **Painting** (`draw_code`, `src/main.c:458`, which already has the line converted to wide chars):
  - Before the text is drawn, find this line's spans by binary search on `word_spans` (visible lines only).
  - For each span, fill a rectangle from x(start) to x(end), with `GetTabbedTextExtentW` on `line_text` using the
    same tab stops as `TabbedTextOutW`, in `added_word_bg` / `removed_word_bg`.
  - Byte→wide offsets use the existing `wide_index_after`.
  - No new GDI objects: it uses the existing `fill` helper.

## Stages
### Stage 1: one span per pair, end to end
- Behaviour: in a paired, similar line, the part between the common prefix and suffix gets the stronger color, in
  both views and both themes. Rewritten lines and unpaired added/removed lines look as today.
- How to check: in a scratch repo, change `foo(a, b);` to `foo(a, c);` and a whole line to something unrelated.
  The first shows only `b`/`c` marked; the second shows plain row color. Toggle `U` for unified.
- New structs: `WordSpan`, the changed range of one line, written on the worker and read by painting.
- Files touched: `src/app.h`, `src/git.c`, `src/main.c` (themes, `draw_code`).
- [x] done (spans checked with a harness over the real `load_diff`; painting not yet seen on screen)

### Stage 2: word-level LCS, several spans
- Behaviour: two edits far apart in one line are marked separately (`foo(a, b)` → `bar(a, c)` marks `foo`/`bar`
  and `b`/`c`, not the `(a, ` between). Lines over the 64-token cap fall back to stage 1's single span.
- How to check: the example above; a line with an inserted word; a very long changed line.
- Files touched: `src/git.c`.
- [x] done (same check). A long line whose changed middle is over the cap usually fails the similarity
  check and gets no marks.

Each stage: `build.bat` clean, `--perf .` within budget with private_kb compared to before, an on-screen check,
ticked here, and an entry in `docs/history.md`.

## Out of scope
- Character-level marks inside a word.
- Moved-line detection.
- Re-pairing lines inside a block by similarity (pairing stays Nth with Nth).
- Marks on unpaired lines.
- Ignore-whitespace.

## Verification
- `build.bat`, then `build\DiffViewer.exe --perf .`; private_kb stays within ±16 KB of the stage-1 picker number
  (1376 KB).
- On screen: scratch repo `scratchpad\langrepo` with edited and committed files, viewed in both views, dark and
  light. Screen captures only when you say the screen is free (last time they caught another window).
- The plan is copied to `docs/plans/word-highlight.md` once approved.
