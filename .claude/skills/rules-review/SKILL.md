---
name: rules-review
description: Review a finished feature in a fresh session against its plan and the coding guideline, then propose where each finding should be prevented next time. Use after the last stage of a plan, before merging.
---

# Review against the rules

Run this in a new session, not the one that wrote the code: it has to read the diff without the reasons that produced it.

## Inputs

- The plan: `docs/plans/<feature>.md`. Ask if there is more than one candidate.
- The base branch to diff against. Default `main`; ask if it doesn't exist.

## Steps

1. Read the plan, `.claude/rules/coding-guideline.md` and `.claude/rules/development-guideline.md`.
2. Get the change: `git diff <base>...HEAD --stat`, then the full diff. Read each changed file in full where the diff alone doesn't show enough.
3. Check, in this order:
   1. **Scope.** Every changed file and every new struct traces to a stage in the plan. List anything that doesn't.
   2. **Existing features.** Any change to an existing feature's UI, flow, shortcuts or defaults that the plan didn't list.
   3. **New files and structs.** Each one, with its reason. Flag those with one user, or whose only reason is structure.
   4. **Memory and speed.** Allocations per line/token/file entry, work in `WM_PAINT` that isn't drawing, tokenizing or painting more than the visible lines, blocking calls on the UI thread.
   5. **Resources.** Every handle, GDI object, process, pipe and allocation is released on every path, including error paths.
   6. **Coding guideline.** Go section by section (Write less code, Win32 rules, Method shape, Errors, Comments, Naming). Skip what the compiler flags and the perf gate already enforce.
4. Run `build.bat`, then `build\DiffViewer.exe --perf .`, and report the numbers next to the budget.
5. Don't change code yet.

## Output

Findings, most important first. For each one:

- `file:line`, the rule it breaks (section name), and what to do about it in one or two sentences.

Then a **Retro** table, one row per finding that could recur:

| Finding | Prevent it with | Concretely |
|---------|-----------------|------------|
| ... | compiler flag in `build.bat` / perf threshold / hook / skill / rule line | the flag, check or sentence to add |

Prefer a gate over a skill, and a skill over a rule line. When a rule line becomes a gate, say which line to delete from the rules file.

Then ask which findings to fix. Apply only the approved ones, keeping each fix surgical.
