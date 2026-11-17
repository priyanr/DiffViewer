# DiffViewer (Rust Port)

A pure Rust port of **DiffViewer**, a high-performance, very low-memory git diff viewer for Windows written directly against native Win32 and GDI APIs using `windows-sys`.

No web engines, no Electron, no heavy frameworks, no bloated dependencies. Single standalone executable.

---

## Performance & Budgets

Tested against the `--perf` benchmarking gate on the DiffViewer repository:

| Metric | Budget | C Version | **Rust Port** | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Private Working Set** | ≤ 4.0 MB | 1,484 KB (~1.45 MB) | **1,248 KB (~1.22 MB)** | **PASSED** (16% leaner) |
| **Startup to First Paint** | ≤ 100 ms | 42 ms | **42 ms** | **PASSED** |
| **Status Scan Time** | - | 297 ms | **250 ms** | **PASSED** (Faster) |
| **Diff + Highlight Time**| - | 109 ms | **78 ms** | **PASSED** (Faster) |
| **Files Loaded** | - | 158 | **158** | **100% Parity** |
| **Diff Lines** | - | 2 | **2** | **100% Parity** |

---

## Features

- **Side-by-side & Unified View:** Toggle between 2-pane side-by-side and unified views with keyboard shortcut `U` or status bar toggle.
- **Word-Level Highlighting:** LCS/Myers diff identifies character/word-level span edits inside changed lines.
- **20 Syntax Highlighting Modes:** Full syntax highlighting with state carry-over across diff hunks (C, C++, C#, CSS, Go, HTML, Java, JavaScript, JSON, Kotlin, PowerShell, Python, Rust, Shell, SQL, TypeScript, XML, YAML, Zig, Plain text).
- **Auto-Refresh via Directory Watcher:** Non-blocking background thread with `ReadDirectoryChangesW` and debounced 300 ms git status checks (`--no-optional-locks`).
- **Interactive Ref Picker:** Dropdown popup with search filtering for local branches, remote branches, tags, and commit hashes (`git for-each-ref` and `git log`).
- **Ignore Line Endings:** Built-in normalization toggle so CRLF/LF churn doesn't clutter diffs.
- **Dark/Light Theme:** Detects Windows personalization setting (`AppsUseLightTheme`) and matches system dark mode via DWM immersive dark mode attribute.
- **Per-Monitor DPI v2:** Embedded application manifest with `PerMonitorV2` awareness.

---

## Architecture & Code Structure

All code is organized under `rust/`:

- [Cargo.toml](file:///F:/priyan/Projects/AI/DIffViewer/rust/Cargo.toml): Cargo configuration using `windows-sys` 0.61.2 with minimal Win32 feature flags.
- [build.rs](file:///F:/priyan/Projects/AI/DIffViewer/rust/build.rs): Build script that compiles `../app.rc` into coff object and links it to embed the manifest.
- [src/types.rs](file:///F:/priyan/Projects/AI/DIffViewer/rust/src/types.rs): Common data models, color palettes, theme definitions, language tokens, and line representations.
- [src/syntax.rs](file:///F:/priyan/Projects/AI/DIffViewer/rust/src/syntax.rs): Syntax lexer with binary search keywords for 20 languages and multi-line token state tracking.
- [src/git.rs](file:///F:/priyan/Projects/AI/DIffViewer/rust/src/git.rs): Git process runner via anonymous pipes, background worker thread, Myers diff word splitter, ref listings, and directory watcher.
- [src/main.rs](file:///F:/priyan/Projects/AI/DIffViewer/rust/src/main.rs): Win32 GUI entry point, DPI scaling, window classes, custom double-buffered GDI drawing, custom edit control subclassing, and `--perf` reporting.

---

## Building

### Quick Build (Script)

Run the included build script:

```cmd
cd rust
build.bat
```

The resulting executable will be placed in `rust\build\DiffViewer.exe`.

### Using Cargo

Ensure Rust (`1.80+`) and a C linker (either MSVC or `llvm-mingw`) are installed:

```bash
cd rust
cargo build --release
```

Binary is output to `target/x86_64-pc-windows-gnullvm/release/diffviewer.exe` (or `target/release/diffviewer.exe`).

---

## Running Benchmarks / Performance Gate

Run with `--perf` pointing to any git repository:

```cmd
rust\build\DiffViewer.exe --perf F:\path\to\repo | more
```

Example output:
```
private_kb=1248 peak_ws_kb=9784 startup_ms=42 status_ms=250 diff_ms=78 files=158 diff_lines=2
```
