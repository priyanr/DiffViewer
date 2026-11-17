# Future ideas

Suggestions not planned yet. Each one becomes its own plan in `docs/plans/` before any code is written, and has to
fit the budget: private memory ≤ 4 MB, first paint ≤ 100 ms.

## Reviewing an agent's changes faster
In order of payoff:

1. **Reviewed marks.** Mark a file as reviewed. The mark clears when the file changes again: compare the
   HEAD/index hashes and file time/size the diff already records. With an agent editing in rounds, you then read
   only what is new.
2. **Fold unchanged stretches.** The diff loads the whole file. Collapse long runs of context lines to a "⋯ 120
   lines" row that expands on click.
3. **Search in the diff, and select/copy text.**
4. **Notes for the agent.** Click a line, type a note, and copy all notes as `file:line — note` to paste back into
   Claude Code.
5. **Ignore all whitespace.** The `git diff -w` counterpart of the "Ignore line endings" checkbox, for
   reformatting noise.

## Gaps in what exists
- **Dark popup menu.** The highlighter menu stays light in dark mode. It needs the uxtheme dark-mode call.
- **Word marks on long lines.** A line whose changed middle is over 64 tokens gets one span, which usually fails
  the similarity check and so shows no marks. Raise the cap, or match in windows.
- **Highlighting gaps:**
  - `<script>`/`<style>` inside HTML;
  - JS regex literals;
  - YAML `|`/`>` block scalars;
  - Shell multi-line strings.
- **Mode-only vs line-ending-only.** With "Ignore line endings" ticked, an empty diff always says "Only line
  endings changed.", even for a mode-only change (rare on Windows).
- **Screenshots of the window.** Screenshot tools get a blank image because the window doesn't answer
  `WM_PRINTCLIENT`. Only automation is affected.

## Tooling (outside this repo)
- The `control-exe` skill's doc says port 5165; the running server listens on 5000.
