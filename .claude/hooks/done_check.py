"""Stop hook: the agent can't finish while the build is red or the app is over its memory/speed budget.

When the agent tries to end its turn and C code (or the build) has changed:
1. Runs build.bat (clang, warnings are errors). If it fails, the agent is sent back with the errors.
2. Runs `build\\DiffViewer.exe --perf <repo>`, which prints `private_kb=<n> peak_ws_kb=<n> startup_ms=<n> ...`,
   and sends the agent back if private memory or startup time is over budget. Peak working set is reported only:
   it is mostly shared system DLL pages, not the app's own memory.
   After 3 failed attempts in a row it is let go and you get a message instead, so a red build can't loop forever.
3. If the change adds more new .c/.h files than allowed, the agent is sent back once to justify each new file
   (coding guideline: Before you finish) or inline it.

A green result is remembered for the exact working-tree state, so a turn that changes nothing doesn't rebuild.
Optional settings live in .claude/done-check.json (see README).
"""
import hashlib
import json
import os
import re
import subprocess
import sys

DEFAULTS = {
    "workingDirectory": ".",
    "build": ".\\build.bat",  # cmd here doesn't run a bare name from the current folder
    "perf": "build\\DiffViewer.exe --perf .",
    "maxPrivateKB": 4096,
    "maxStartupMs": 100,
    "maxNewFiles": 6,
    "maxAttempts": 3,
}

CODE_FILE = re.compile(r"\.(c|h|rc|manifest)$|(^|/)build\.bat$", re.IGNORECASE)
COMPILER_ERROR = re.compile(r":\d+:\d+: (error|warning|fatal error):|: (error|undefined reference)", re.IGNORECASE)
PERF_VALUE = re.compile(r"(\w+)=(\d+)")


def git(project_dir, *args):
    result = subprocess.run(["git", *args], cwd=project_dir, capture_output=True)
    return result.returncode, result.stdout


def changed_files(project_dir):
    """(status, path) for every changed or untracked file, paths relative to the repo root."""
    code, out = git(project_dir, "status", "--porcelain", "-uall", "-z")
    if code != 0:
        return None
    entries = []
    parts = out.decode("utf-8", "replace").split("\0")
    i = 0
    while i < len(parts):
        item = parts[i]
        i += 1
        if len(item) < 4:
            continue
        status, path = item[:2], item[3:]
        if status[0] in "RC":
            i += 1  # the next entry is the original path of a rename/copy
        if re.search(r"(^|/)build/", path):
            continue  # build output in a repo without a .gitignore
        entries.append((status, path))
    return entries


def tree_fingerprint(project_dir, entries):
    """Hash of the uncommitted state: the diff against HEAD plus size and time of untracked files."""
    digest = hashlib.sha256()
    code, diff = git(project_dir, "diff", "HEAD", "--binary")
    if code != 0:
        _, diff = git(project_dir, "diff", "--binary")
    digest.update(diff)
    for status, path in sorted(entries):
        if status == "??":
            try:
                stat = os.stat(os.path.join(project_dir, path))
                digest.update(f"{path}|{stat.st_size}|{stat.st_mtime_ns}".encode())
            except OSError:
                pass
    return digest.hexdigest()


def problem_summary(output, project_dir, limit=60):
    """Compiler/linker errors if there are any, otherwise the end of the output."""
    root = project_dir.replace("\\", "/").rstrip("/") + "/"
    lines = [line.rstrip().replace("\\", "/").replace(root, "") for line in output.splitlines() if line.strip()]
    errors = []
    for line in lines:
        if COMPILER_ERROR.search(line) and line not in errors:
            errors.append(line)
    picked = errors if errors else lines
    return "\n".join(picked[-limit:])


def load_json(path, fallback):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return fallback


def run(command, work_dir):
    try:
        result = subprocess.run(command, cwd=work_dir, shell=True, capture_output=True,
                                text=True, encoding="utf-8", errors="replace", timeout=300)
    except subprocess.TimeoutExpired:
        return 1, f"`{command}` timed out after 300 s."
    return result.returncode, result.stdout + "\n" + result.stderr


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        event = {}
    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or os.getcwd()

    config = dict(DEFAULTS)
    config.update(load_json(os.path.join(project_dir, ".claude", "done-check.json"), {}))

    entries = changed_files(project_dir)
    if entries is None:
        return 0  # not a git repository: nothing to compare against
    code, git_dir = git(project_dir, "rev-parse", "--absolute-git-dir")
    state_path = os.path.join(git_dir.decode().strip() if code == 0 else project_dir, "claude-done-check.json")
    state = load_json(state_path, {})
    session = event.get("session_id", "")

    def save():
        try:
            with open(state_path, "w", encoding="utf-8") as f:
                json.dump(state, f)
        except OSError:
            pass

    if not any(CODE_FILE.search(path) for _, path in entries):
        if state.get("attempts"):
            state["attempts"] = {}
            save()
        return 0  # no code changed in the working tree

    fingerprint = tree_fingerprint(project_dir, entries)

    def block(reason):
        print(json.dumps({"decision": "block", "reason": reason}))
        return 0

    def fail(what, details):
        attempts = state.get("attempts", {})
        attempts = {session: attempts.get(session, 0) + 1}
        state["attempts"] = attempts
        save()
        if attempts[session] > config["maxAttempts"]:
            state["attempts"] = {}  # let go once; the next turn is checked again
            save()
            print(json.dumps({"systemMessage": f"done-check: {what} still fails after {config['maxAttempts']} attempts; stopping anyway. Check it before committing."}))
            return 0
        return block(
            f"{what} failed (attempt {attempts[session]} of {config['maxAttempts']}). "
            f"A stage is not done until the build has no warnings and the app is inside its memory and startup "
            f"budget. Fix the cause; don't suppress warnings, change the thresholds or special-case --perf "
            f"without asking. If two fixes resting on the same assumption have now failed, write that assumption "
            f"in one sentence and check it before trying another fix.\n\n{details}"
        )

    # 1. Build, 2. memory and startup budget
    if state.get("green") != fingerprint:
        work_dir = os.path.join(project_dir, config["workingDirectory"])
        code, output = run(config["build"], work_dir)
        if code != 0:
            return fail(f"`{config['build']}`", problem_summary(output, project_dir))

        code, output = run(config["perf"], work_dir)
        values = {key: int(value) for key, value in PERF_VALUE.findall(output)}
        if code != 0 or "private_kb" not in values or "startup_ms" not in values:
            return fail(f"`{config['perf']}`", "It must exit 0 and print `private_kb=<n> startup_ms=<n>`.\n\n"
                        + problem_summary(output, project_dir))
        over = []
        if values["private_kb"] > config["maxPrivateKB"]:
            over.append(f"private memory {values['private_kb']} KB > budget {config['maxPrivateKB']} KB")
        if values["startup_ms"] > config["maxStartupMs"]:
            over.append(f"startup {values['startup_ms']} ms > budget {config['maxStartupMs']} ms")
        if over:
            return fail("The perf budget", "; ".join(over))

        state["green"] = fingerprint
        state["attempts"] = {}
        state["lastPerf"] = values
        save()

    # 3. Too many new files
    new_files = sorted(path for status, path in entries
                       if path.endswith((".c", ".h")) and (status == "??" or status[0] == "A"))
    if len(new_files) > config["maxNewFiles"]:
        listing = hashlib.sha256("\n".join(new_files).encode()).hexdigest()
        if state.get("newFilesAcknowledged") != listing:
            state["newFilesAcknowledged"] = listing
            save()
            files = "\n".join(f"- {path}" for path in new_files)
            return block(
                f"This change adds {len(new_files)} new .c/.h files (limit {config['maxNewFiles']}). "
                f"For each one, say in one line why it exists. Merge any file whose reason is "
                f"'separation of concerns', 'future flexibility' or 'best practice', or that has a single caller. "
                f"If the stage is simply too big, say so and propose a split.\n\n{files}"
            )

    return 0


if __name__ == "__main__":
    sys.exit(main())
