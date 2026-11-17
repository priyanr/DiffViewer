# Development Guideline

## Changes to existing features
- Before building a new feature or change, check whether it changes how an existing feature behaves: its UI, flow,
  keyboard shortcuts, colors or default values.
- If it does, stop and ask the user to confirm explicitly before making the change. Say which existing feature is
  affected, what changes for its users and why. Don't infer approval from the new request alone.
- If it can't be done without affecting the existing feature, offer an alternative that leaves it unchanged where one exists.
- After the change, list the affected existing features in the summary and in `docs/history.md`.
