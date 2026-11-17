"""PreToolUse hook: stop the agent from quietly changing the project's shape or its own gates.

- C++ files and heavy frameworks (MFC, ATL, WTL, WinRT, Direct2D, .NET) are denied: the app is plain C + Win32 + GDI.
- Edits to the build, new link libraries, package installs and edits to the gates themselves need my approval.
- New files named like ceremony (*_helper.c, *_utils.c, *manager*.c, ...) need my approval.

Reads the hook JSON from stdin. Prints a permission decision as JSON, or nothing to allow.
"""
import json
import os
import re
import sys

CPP_FILE = re.compile(r"\.(cpp|cc|cxx|hpp|hh|hxx|cs|vb)$", re.IGNORECASE)

BANNED_INCLUDES = re.compile(
    r"^\s*#\s*include\s*[<\"](afx\w*\.h|atl\w*\.h|wtl\w*\.h|winrt/[^>\"]*|d2d1\w*\.h|dwrite\w*\.h|"
    r"d3d\w*\.h|gdiplus\w*\.h|windows\.ui\.[^>\"]*|wrl[^>\"]*|mscoree\.h|metahost\.h)[>\"]",
    re.MULTILINE | re.IGNORECASE,
)

# Files that define the project's shape or the gates. Editing them needs approval.
GUARDED_FILES = [
    (re.compile(r"(^|/)build\.bat$", re.IGNORECASE), "the build (compiler flags, link libraries, warnings)"),
    (re.compile(r"\.(vcxproj|sln|slnx|filters)$", re.IGNORECASE), "project or solution file"),
    (re.compile(r"\.manifest$", re.IGNORECASE), "application manifest (DPI awareness, common controls)"),
    (re.compile(r"(^|/)\.claude/(settings[^/]*\.json|done-check\.json|hooks/.*)$"), "agent hooks and settings (the gates)"),
]

CEREMONY_NAME = re.compile(
    r"(^|/)\w*(helper|helpers|utils|utility|util|manager|factory|wrapper|common|misc)\w*\.(c|h)$",
    re.IGNORECASE,
)

PACKAGE_COMMANDS = re.compile(
    r"\b(winget|choco|scoop|vcpkg|conan)\s+(install|add)\b"
    r"|\bpip3?\s+install\b"
    r"|\b(npm|pnpm)\s+(i|install|add)\s+(?![-&|;])\S"
    r"|\byarn\s+add\b"
    r"|\bdotnet\s+(new|add)\b",
    re.IGNORECASE,
)


def relative_path(file_path, project_dir):
    path = file_path.replace("\\", "/")
    root = project_dir.replace("\\", "/").rstrip("/")
    if root and path.lower().startswith(root.lower() + "/"):
        path = path[len(root) + 1:]
    return path


def new_text(tool_name, tool_input):
    if tool_name == "Write":
        return tool_input.get("content", "")
    if tool_name == "Edit":
        return tool_input.get("new_string", "")
    if tool_name == "MultiEdit":
        return "\n".join(e.get("new_string", "") for e in tool_input.get("edits", []))
    return ""


def decide(event, project_dir):
    tool_name = event.get("tool_name", "")
    tool_input = event.get("tool_input", {}) or {}

    if tool_name in ("Bash", "PowerShell"):
        command = tool_input.get("command", "")
        if PACKAGE_COMMANDS.search(command):
            return "ask", "ClaudeRules: this command installs a package or tool. Nothing new without my approval (coding guideline: Write less code)."
        return None

    if tool_name not in ("Write", "Edit", "MultiEdit"):
        return None

    path = relative_path(tool_input.get("file_path", ""), project_dir)

    if CPP_FILE.search(path):
        return "deny", f"ClaudeRules: {path} is not C. The app is plain C + Win32 + GDI, one exe (coding guideline: Win32 rules)."

    if path.endswith((".c", ".h")) and BANNED_INCLUDES.search(new_text(tool_name, tool_input)):
        return "deny", "ClaudeRules: MFC/ATL/WTL/WinRT/Direct2D/DirectWrite/GDI+/.NET headers are not used. Plain Win32 and GDI only (coding guideline: Win32 rules)."

    for pattern, what in GUARDED_FILES:
        if pattern.search(path):
            return "ask", f"ClaudeRules: {path} is {what}. Changing it needs my approval. Say why in one line."

    if tool_name == "Write" and CEREMONY_NAME.search(path) and not os.path.exists(os.path.join(project_dir, path)):
        return "ask", f"ClaudeRules: new file {path} looks like a helper/utils/manager file. Files are split by job (window, git, diff, highlighter), not by kind (coding guideline: Project layout). Say what job it does."

    return None


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or os.getcwd()
    result = decide(event, project_dir)
    if result:
        decision, reason = result
        print(json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": decision,
                "permissionDecisionReason": reason,
            }
        }))
    return 0


if __name__ == "__main__":
    sys.exit(main())
