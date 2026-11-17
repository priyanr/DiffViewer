# C / Win32 Style: Small, Fast, Direct

Write code the way an experienced Win32 developer would write it by hand: direct, readable, few files, few
allocations. The product's two criteria are **very low memory** and **fast**. Every line is judged against them.

## Write less code
- Make the smallest change that fully solves the request. If 200 lines were written and 50 would do, rewrite it as 50.
- No features, options, config or flexibility that weren't asked for.
- Before writing new code, check in this order: existing code in this repo → the Win32 API / C runtime → only then
  new code. No new library, DLL or `-l<lib>` link flag without asking first.
- Find the closest existing code that does something similar and match its shape. Don't invent a new pattern.
- No caching, extra threads, pools or lookup tables without a named performance need. Fixed by design: git runs on
  one worker thread, and only visible lines are tokenized and painted.
- No compatibility shims or unused parameters for a caller that doesn't exist. Change the call sites instead.
- Prefer editing an existing file to adding one. A new `.c`/`.h` file needs a reason tied to the request.
- Would an experienced developer reviewing this call it overcomplicated? If so, simplify before finishing.

## Memory and speed
- Budget: private memory (Task Manager's number) under **4 MB** with a mid-sized repo open, start to first paint
  under **100 ms**. The Stop hook measures both (`DiffViewer.exe --perf <repo>`); don't change the thresholds
  without asking. Peak working set is printed too; a jump there means a new system DLL got loaded, so find out why.
- No allocation per line, per token or per file entry. Allocate one block (an arena, or one buffer sized from the
  input) and point into it. Free a whole view at once, not piece by piece.
- A file is read once into one buffer. Lines are `{offset, length}` spans into that buffer, never copied strings.
- Paint only what is visible: compute the first and last visible line from the scroll position and stop there.
  Never tokenize or measure the whole file to paint one screen.
- No work in `WM_PAINT` that isn't drawing. Diffing and layout happen when data changes, not on every paint.
- Anything that can block (spawning git, reading a big file, diffing) runs on the worker thread and reports back
  with `PostMessage`. The UI thread never waits on a pipe or a process.
- Measure before optimizing. A change made "for speed" names what it made faster and by how much.

## Win32 rules
- Unicode only: `UNICODE`/`_UNICODE` defined, `W` functions, `wchar_t` for UI text. Git output and file contents
  stay UTF-8 bytes until the moment they are drawn.
- Plain C, Win32 and GDI. No C++, MFC, ATL, WTL, .NET, WinRT, Direct2D or third-party UI libraries.
- Every GDI object, handle, pipe and process you create is released on every path (`DeleteObject`, `CloseHandle`,
  `ReleaseDC`). Create fonts, brushes and pens once per theme/DPI change, not per paint.
- Per-monitor DPI aware (v2). Sizes are scaled from 96-DPI values; handle `WM_DPICHANGED`.
- Theme follows Windows (dark/light) via the `AppsUseLightTheme` setting; dark title bar with
  `DwmSetWindowAttribute`. Colors come from one theme table, never hard-coded at the draw call.
- One window procedure per window class, a `switch` on the message, each case short. A long case calls a function.
- Git is started with `CreateProcessW`, `CREATE_NO_WINDOW` and redirected pipes. Quote every argument; never put an
  unquoted user path or ref name on a command line.

## Method shape
- A long function is fine if it reads top to bottom as one flow; mark steps with a short comment.
- A `static` function is for code that two or more functions use, or a step long enough to hide the flow.
- No helper or utility functions for code used in one place. Extract on reuse (rule of three) or when it is a real
  rule with a clear input → output (the diff, the C# tokenizer, the porcelain parser).
- Flat over nested: early returns, no deep if/else chains. `goto cleanup` is fine for releasing resources.
- A struct used by one file is declared in that file, not in a header.

## Errors
- No error handling for states that can't happen: null checks on values that are never null, re-validating what the
  caller already checked.
- Out of memory: fail fast with a message box naming what failed, and exit. No recovery paths.
- Expected failures (folder isn't a git repo, git not on PATH, file deleted while viewing, binary file, bad ref) are
  shown in the window or status bar with the reason, and the app keeps running.
- Check the return value of every Win32 call that can fail for a real reason (`CreateProcessW`, `CreateFileW`,
  `ReadFile`). Don't check calls that can't fail in practice (`GetClientRect` on our own window).

## Comments
- Comment the reason for a branch and constraints the code can't show (why a buffer is sized like this, an order
  that matters, a Win32 quirk), next to the code they apply to.
- No comments that repeat the code, narrate steps ("// create the brush") or describe the change ("// added for X").
- No commented-out code, no author/date headers (git has them).

## Changing existing code
- Stay surgical: every changed line traces to the request. Don't refactor, rename, reformat or tidy adjacent code
  that isn't broken; mention it in the summary instead.
- When fixing a bug, change only the lines that cause it. A guard that only hides the crash is not a fix.
- Remove includes, variables and functions your change made unused. Report unrelated dead code; don't delete it
  unless asked.
- If the request is unclear in a way that changes what gets built, ask before building, not after.

## Project layout
One project, one exe, no DLLs. Don't add projects, libraries or top-level folders without asking.
- `src/` holds all `.c`/`.h` files, a handful in total, split by job (window/UI, git, diff, C# highlighter), not by
  layer.
- `build.bat` builds `build\DiffViewer.exe` with llvm-mingw `clang`, warnings as errors. It is the only build.
- `DiffViewer.exe --perf <repo>` is the perf gate's entry point: opens the repo, paints once, prints peak working
  set and startup time, exits.

## Naming and clarity
- Name a thing for what it is or does, not for its role in your head. No `data`, `info`, `tmp`, `helper`, `mgr`.
  If a name needs a comment to be understood, rename it.
- One word, one meaning. Don't use a word for two different things.
- A function that can return NULL says so (`...OrNull`); a conversion says what it converts to (`utf8_to_wide`).
- Prefix functions by their file's job (`git_`, `diff_`, `cs_`, `ui_`). lower_snake_case for functions and
  variables, PascalCase for struct types, UPPER_SNAKE for macros and constants.
- Don't build a setting, option or UI control that no code uses yet.

## Before you finish
Read your diff and:
- revert edits to files outside the task;
- inline static functions with one caller that don't hide a long step;
- remove checks around code that can't fail;
- remove comments that narrate or repeat the code;
- check every new allocation, handle and GDI object has its release on every path;
- remove anything that doesn't change behavior or trace to the request.

List every new file and struct you created, with one line saying why it exists.
If the reason is "separation of concerns", "future flexibility" or "best practice", delete it and inline the code.

## Consistency
If existing code in the repo already follows a convention, follow it, even if it conflicts with the rules above.
Point out the conflict instead of refactoring it on your own.
