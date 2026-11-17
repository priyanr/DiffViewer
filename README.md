# DiffViewer

[![build](https://github.com/priyanr/DiffViewer/actions/workflows/build.yml/badge.svg)](https://github.com/priyanr/DiffViewer/actions/workflows/build.yml)

A very low-memory, fast git diff viewer for Windows, written in plain C against Win32 and GDI. One small exe, no
runtime, no installer. Leave it open beside your editor or a coding agent and watch changes arrive.

- Point it at a repo: lists added / modified / deleted / untracked / renamed files.
- Working tree vs HEAD, staged vs unstaged, and commit/branch vs commit/branch.
- Side-by-side diff with a toggle to unified, Prev/Next change across files.
- Changed words inside a changed line are highlighted, not only the whole line.
- Syntax highlighting, picked by file extension or from the status bar: C/C++, C#, CSS, Go, HTML, Java,
  JavaScript, JSON, Kotlin, PowerShell, Python, Rust, Shell, SQL, TypeScript, XML, YAML, Zig.
- "Ignore line endings" (on by default), so a file re-saved with CRLF shows only its real edits.
- Auto-refresh when files in the repo change, keeping the selection and scroll position.
- Read-only: it never runs a git command that changes the repo, index or working tree.
- Dark/light theme follows Windows; per-monitor DPI aware.

**Budget:** private memory (Task Manager's number) ≤ 4 MB with a repo open, start to first paint ≤ 100 ms. Today
it uses about 1.4 MB and paints in about 50 ms. The status bar shows its current memory.

## Download

Get `DiffViewer.exe` from the [Releases page](https://github.com/priyanr/DiffViewer/releases):

- **Versioned releases** (`v1.0.0`, ...) are the stable builds.
- **Latest build** is a pre-release rebuilt from every push to `main`.

Each release lists the exe's SHA-256. Needs Windows 10 or later and `git` on PATH.

```
DiffViewer.exe C:\path\to\repo
```

## Build

Needs [llvm-mingw](https://github.com/mstorsjo/llvm-mingw/releases) (the `ucrt-x86_64` zip) and `git` on PATH.
Either put llvm-mingw's `bin` on PATH or point `LLVM_MINGW` at the folder you unzipped:

```
set LLVM_MINGW=C:\tools\llvm-mingw
build.bat
build\DiffViewer.exe C:\path\to\repo
```

`build.bat` is the only build: one `clang` call, warnings are errors, no C runtime (own entry point, system DLLs
only). `build\DiffViewer.exe --perf <repo>` opens the repo, paints once, prints
`private_kb=… peak_ws_kb=… startup_ms=…` and exits. CI runs it and fails over 4 MB of private memory.

To publish a version, tag a commit on `main` and push the tag; CI builds it and creates the release:

```
git tag v1.0.0
git push origin v1.0.0
```

## Use

| What | How |
| --- | --- |
| Open another repo | type its path in the top-left box, Enter |
| Staged vs unstaged | automatic: STAGED / UNSTAGED groups appear when something is staged |
| Branch/commit vs working tree | base ref box (e.g. `HEAD`, `main`, `HEAD~3`), target empty, Enter |
| Branch/commit vs branch/commit | base and target ref boxes, Enter; clear both to go back |
| Pick a ref from a list | click a ref box, or `Down` / `Alt+Down` in it: branches, remote branches, tags and recent commits; type to filter, `Enter` or click to pick, `Esc` to close |
| Unified / side by side | `U`, or the switch at the top right |
| Next / previous change | `N` / `P` (or `F7` / `Shift+F7`), or the Prev / Next buttons; past the last change of a file it moves on to the next file's first change (and back) |
| Change the highlighter | click the language at the right of the status bar (`Auto: Rust ▾`); a pick lasts for the open file |
| Show line-ending changes | untick "Ignore line endings" in the status bar |
| Resize the file list | drag the line between the list and the diff |
| Move between boxes and panes | `Tab` / `Shift+Tab` |
| Scroll the diff | wheel, `Shift`+wheel or tilt for sideways, arrows, PgUp/PgDn, Home/End |

The list refreshes by itself 300 ms after files in the repo change (not when comparing two refs).

## Code

| File | Job |
| --- | --- |
| `src/main.c` | entry point, windows, layout, painting, theme, DPI, `--perf` |
| `src/git.c` | runs `git.exe` on a worker thread, parses status and diff output, word marks, folder watcher |
| `src/syntax.c` | line-at-a-time highlighters, one byte of lexer state carried between lines |
| `src/app.h` | the few structs and functions the files share |

Rules of the house: one allocation per view (arenas, spans into one buffer), only visible lines are tokenized and
painted, nothing blocking on the UI thread, no new DLLs. See `.claude/rules/coding-guideline.md`. Plans for
each feature live in `docs/plans/`, the change log in `docs/history.md`, and ideas not started yet in
`docs/future.md`.

## Contributing

Issues and pull requests are welcome. Keep changes small and inside the budget: CI must stay green, and a change
that raises private memory or startup time needs a reason. The project is developed with
[Claude Code](https://claude.com/claude-code), and the setup below is part of the repo. You don't need it to
contribute, but it holds the same rules.

### Claude Code setup

| File | Purpose |
| --- | --- |
| `.claude/rules/coding-guideline.md` | C/Win32 style, memory and speed rules, project layout, naming |
| `.claude/rules/development-guideline.md` | Confirm before changing an existing feature's behaviour |
| `.claude/rules/workflow-guideline.md` | Clarify → plan in vertical-slice stages → build stage by stage → fresh-session review → retro |
| `.claude/hooks/guard_edits.py` | Denies C++ files and MFC/ATL/WinRT/Direct2D/.NET headers; asks before edits to `build.bat`, the manifest, the hooks, package installs, and new `*helper*`/`*utils*`/`*manager*` files |
| `.claude/hooks/done_check.py` | When the agent tries to finish: runs `build.bat` (warnings as errors) and the `--perf` budget check (private memory, startup time), sends it back while either fails; makes it justify each new file when a change adds more than 6 |
| `.claude/skills/rules-review` | Fresh-session review of plan + diff against the rules, ending with a retro table |

The hooks need **Python 3** and **git** on PATH. An optional, git-ignored `.claude/done-check.json` overrides the
defaults, for example to set `LLVM_MINGW` for the hook's build:

```json
{
  "build": "set LLVM_MINGW=C:\\tools\\llvm-mingw&& .\\build.bat",
  "perf": "build\\DiffViewer.exe --perf .",
  "maxPrivateKB": 4096,
  "maxStartupMs": 100,
  "maxNewFiles": 6,
  "maxAttempts": 3
}
```

## License

[MIT](LICENSE)
