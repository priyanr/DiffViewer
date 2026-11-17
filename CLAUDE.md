# DiffViewer

A very low-memory, fast git diff viewer for Windows. Point it at a repo folder and it lists added, modified,
deleted and untracked files and shows each file's diff side by side (or unified), with syntax highlighting.
Plain C + Win32 + GDI, one exe. It talks to nothing but `git.exe`.

## Commands

- Build: `build.bat` → `build\DiffViewer.exe` (llvm-mingw clang from `LLVM_MINGW` or PATH, warnings as errors;
  the local Stop hook sets `LLVM_MINGW` in the git-ignored `.claude/done-check.json`)
- Run: `build\DiffViewer.exe <repo-folder>`
- Perf gate: `build\DiffViewer.exe --perf <repo-folder>` → prints `private_kb=<n> peak_ws_kb=<n> startup_ms=<n> ...`
  and exits
- No unit tests. Check behaviour by running the app (`control-exe` skill for launch + screenshot).

## Budget

Private memory ≤ 4 MB, start to first paint ≤ 100 ms (enforced by the Stop hook). Peak working set (~9–11 MB) is
reported only: it is mostly shared Windows DLLs any GUI window loads. If a change can't fit, stop and say so instead
of working around the gate.

## Layout

Follows **Project layout** in `.claude/rules/coding-guideline.md`: everything in `src/`, split by job.

## Boundaries

- `git.exe` from PATH, run on the worker thread with redirected pipes. Commands used: `git rev-parse --show-toplevel`,
  `git status --porcelain=v2 -z --branch`, `git diff --name-status -z <base> [<target>]`, and `git diff` (unstaged),
  `git diff --cached HEAD` (staged), `git diff --no-index NUL <file>` (untracked), `git diff <base> [<target>]`.
- The same worker thread watches the repo folder (`ReadDirectoryChangesW`, overlapped) for auto-refresh.
- The repo is read-only to this app: it never runs a git command that changes the repo, index or working tree.

## Exceptions to the rules

<!-- Deliberate departures from the coding guideline in this repo, with the reason. -->
