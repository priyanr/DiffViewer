# Memory usage in the status bar

## Goal
See the app's own memory use (private working set, as Task Manager shows it) at the end of the status bar.

## Affected existing features
- Status bar: gains a trailing `· <n.n> MB` field. Confirmed by the user on 2026-11-17.

## Stages
### Stage 1: show private memory on every status bar paint
- Behaviour: the status bar ends with e.g. `1.5 MB`, refreshed whenever the main window repaints (no timer).
- How to check: open a repo; the last field of the status bar is the memory in MB.
- New files/structs: `MemoryCounters` in `main.c`, the PROCESS_MEMORY_COUNTERS_EX2 layout the mingw headers
  lack; it was a local struct in `perf_finish` and is now shared by the status bar.
- Files touched: `src/main.c`.
- [x] done

## Out of scope
A timer to keep the number live while idle; peak working set or other counters.
