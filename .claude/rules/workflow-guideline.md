# Workflow Guideline

How a feature goes from request to merged code. The coding guideline says what the code looks like; this file says how the work is done. Hooks in `.claude/hooks/` enforce the mechanical parts, so don't try to get around them.

## 1. Clarify

- If the request is vague enough that two reasonable developers would build different things, ask questions before planning. One question at a time, each with your recommended answer.

## 2. Plan

Plan in plan mode. Write the approved plan to `docs/plans/<feature>.md` before writing code, so it survives the session. Use this shape:

```markdown
# <Feature>

## Goal
One or two sentences: what the user can do afterwards.

## Affected existing features
Name each existing feature whose UI, flow, shortcuts or defaults change, or "None".

## Stages
### Stage 1: <thin end-to-end path>
- Behaviour: what works after this stage, from the outside.
- How to check: what to open and what you should see.
- New files/structs: each one with one line saying why it exists.
- Files touched: the existing files that change.
- [ ] done

### Stage 2: ...

## Out of scope
What this plan deliberately does not do.
```

- Stages are vertical slices: each one works end to end (git call → parse → draw on screen) for a narrow case. Stage 1 is the thinnest path that proves the design. Never "all the parsing first, then all the drawing".
- Keep a stage small enough to review in one sitting: roughly under 400 changed lines. Split it if it is bigger.
- Every new file or struct listed in the plan needs a reason tied to the request. "Separation of concerns", "future flexibility" or "best practice" is not a reason; drop it.
- Wait for approval of the plan before stage 1.

## 3. Build, one stage at a time

- Work only on the current stage. Don't start code that belongs to a later stage.
- There are no unit tests. Run the app and check the stage's behaviour yourself (the `control-exe` skill can launch it and take a screenshot). Say what you checked and what you couldn't.
- If the plan turns out to be wrong, stop and say what changed. Update the plan file before continuing.
- A stage is done when:
  - `build.bat` passes with no warnings and `DiffViewer.exe --perf .` stays inside the memory and startup budget (the Stop hook runs both and will not let you finish otherwise);
  - you have done the "Before you finish" check from the coding guideline;
  - the stage is ticked in the plan file.
- After each stage, stop and summarise: what changed, new files/structs and why, the perf numbers, anything you noticed but did not fix. Then I commit and we start the next stage from green.

## 4. Review

After the last stage, review in a fresh session with the `rules-review` skill: the plan file and the full diff against the base branch, checked against the coding guideline. Refactoring found in review is done there, not during the stages.

## 5. Retro

For each problem the review found, propose where the fix belongs, in this order of preference:

1. A gate: a compiler warning flag in `build.bat`, a perf threshold, or a hook check.
2. A skill: when the same kind of change keeps coming out wrong.
3. A line in a rules file: only when it can't be checked mechanically.

Rules that become gates are removed from the markdown.
