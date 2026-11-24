"""What differs between Windows and Linux, kept in one place.

Everything else in the program asks this module rather than checking the
platform itself: how a console tool is given a window of its own, how a run's
process tree is stopped, which exit codes mean "somebody interrupted it" rather
than "it crashed", and where per-user data lives.

On Windows a console tool gets its own console from CREATE_NEW_CONSOLE. Linux
has no such thing -- a console is a terminal emulator, a separate program --
so the tool is started inside one. That costs the tool's exit code, which no
terminal passes on, so the command runs under a small shell wrapper that
writes it to a file before the terminal closes.
"""

import os
import shlex
import shutil
import signal
import subprocess
import sys
import time

WINDOWS = sys.platform == "win32"
LINUX = sys.platform.startswith("linux")

# Windows: 0xC000013A, given to a console process sent Ctrl+C or Ctrl+Break,
# or whose console was closed.
STATUS_CONTROL_C_EXIT = 0xC000013A

# Linux: a process ended by SIGHUP (its terminal closed), SIGINT (Ctrl+C) or
# SIGTERM (asked to stop). Seen as a negative return code on a process we
# started directly, and as 128 + the signal number from the shell wrapper.
_INTERRUPT_SIGNALS = (signal.SIGHUP, signal.SIGINT, signal.SIGTERM) if LINUX else ()


def interrupted(code):
    """Whether an exit code means the tool was interrupted, not that it failed.

    Closing a tool's window or pressing Ctrl+C in it says nothing about the
    machine, and must never be reported as a hardware failure.
    """
    if code is None:
        return False
    if WINDOWS:
        return code == STATUS_CONTROL_C_EXIT
    return any(code in (-int(s), 128 + int(s)) for s in _INTERRUPT_SIGNALS)


def describe_exit(code):
    """An exit code in the form that says what happened.

    Windows status codes are recognisable in hex and meaningless in the signed
    decimal Python reports: 3221225477 is nothing, 0xC0000005 is an access
    violation. On Linux a signal is named: "killed by SIGSEGV" is the clue.
    """
    try:
        if WINDOWS:
            return "code %d (0x%08X)" % (code, code & 0xFFFFFFFF)
        number = -code if code < 0 else code - 128 if 128 < code < 160 else 0
        if number:
            try:
                return "code %d (%s)" % (code, signal.Signals(number).name)
            except ValueError:
                pass
        return "code %d" % code
    except (TypeError, ValueError):
        return "code " + str(code)


def data_home():
    """Where per-user application data goes."""
    if WINDOWS:
        return os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
    return os.environ.get("XDG_DATA_HOME") or os.path.join(
        os.path.expanduser("~"), ".local", "share")


# -- a window for a console tool, on Linux ------------------------------------

# Terminal emulators that stay in the foreground for as long as the command
# they were given runs, and how each is told a title and a command. Order is
# only a fallback: $ROCH_TERMINAL, then the desktop's $TERMINAL when it is one
# of these, come first. Single-instance terminals are asked not to be, because
# one that hands the command to a server process and exits at once would look
# to the runner like a tool that ended immediately.
_TERMINALS = (
    ("foot", lambda title: ["--title=" + title, "--app-id=" + _APP_ID]),
    ("alacritty", lambda title: ["--title", title, "--class", _APP_ID, "-e"]),
    ("kitty", lambda title: ["--title", title, "--class", _APP_ID]),
    ("ghostty", lambda title: ["--title=" + title,
                               "--gtk-single-instance=false", "-e"]),
    ("wezterm", lambda title: ["start", "--always-new-process", "--class",
                               _APP_ID, "--"]),
    ("konsole", lambda title: ["--nofork", "-p", "tabtitle=" + title, "-e"]),
    ("gnome-terminal", lambda title: ["--wait", "--title", title, "--"]),
    ("xfce4-terminal", lambda title: ["--disable-server", "--title", title,
                                      "-x"]),
    ("xterm", lambda title: ["-T", title, "-e"]),
    ("xdg-terminal-exec", lambda title: ["--title=" + title, "--"]),
)
_APP_ID = "RochStressTest.tool"

# The wrapper the tool runs under inside the terminal. $1 is where the exit
# code goes, $2 is text to type into the tool before handing its input over to
# the keyboard (empty for none), and the rest is the tool's own command line.
# Typing ahead is how mprime is taken straight to its torture-test questions,
# as Prime95's dialog is opened on Windows: the text is sent, then cat passes
# on whatever is typed in the window, and is stopped once the tool exits.
_WRAPPER = r'''code=$1; feed=$2; shift 2
if [ -n "$feed" ]; then
  { printf '%s' "$feed"; exec cat; } | "$@"
  status=$?
  pkill -P $$ -x cat 2>/dev/null
else
  "$@"
  status=$?
fi
printf '%s\n' "$status" > "$code"
exit "$status"
'''


def terminal():
    """(name, argv prefix builder) for the terminal to use, or None."""
    chosen = os.environ.get("ROCH_TERMINAL", "").strip()
    if chosen:
        parts = shlex.split(chosen)
        if parts and shutil.which(parts[0]):
            return parts[0], lambda title: parts[1:]
    preferred = os.path.basename(os.environ.get("TERMINAL", "").split(" ")[0])
    ordered = sorted(_TERMINALS, key=lambda entry: entry[0] != preferred)
    for name, options in ordered:
        if shutil.which(name):
            return name, options
    return None


def in_terminal(argv, title, code_file, feed=""):
    """*argv* wrapped to run in a terminal window, or None when there is none.

    The command runs under the wrapper above, which writes its exit status to
    *code_file*; if that file is missing when the terminal exits, the window
    was closed before the tool finished.
    """
    found = terminal()
    if found is None:
        return None
    name, options = found
    return ([shutil.which(name)] + options(title)
            + ["sh", "-c", _WRAPPER, "roch-stresstest", code_file, feed]
            + list(argv))


# -- stopping a run -----------------------------------------------------------

def _children_by_parent():
    """{parent pid: [child pids]} for every process on the system."""
    table = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open("/proc/" + entry + "/stat", "rb") as handle:
                stat = handle.read().decode("utf-8", "replace")
            # The command name is in parentheses and may itself contain
            # spaces or parentheses, so the fields are read after the last one.
            parent = int(stat[stat.rindex(")") + 2:].split()[1])
        except (OSError, ValueError, IndexError):
            continue
        table.setdefault(parent, []).append(int(entry))
    return table


def _descendants(pid):
    """Every process started by *pid*, directly or not, deepest first."""
    table = _children_by_parent()
    found, stack = [], [pid]
    while stack:
        for child in table.get(stack.pop(), []):
            if child not in found:
                found.append(child)
                stack.append(child)
    return found[::-1]


def _alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    # A zombie still answers; it has finished all the same.
    try:
        with open("/proc/%d/stat" % pid, "rb") as handle:
            stat = handle.read().decode("utf-8", "replace")
        return stat[stat.rindex(")") + 2] != "Z"
    except (OSError, ValueError, IndexError):
        return False


def kill_tree(process):
    """Stop a process and anything it started.

    Windows: taskkill /T /F, because the windowed tools ignore terminate() and
    Prime95 leaves worker threads mid-FFT if the tree is not taken with it.

    Linux: the whole tree is collected *before* anything is signalled. A tool
    in a terminal is not in the terminal's process group -- the terminal gives
    it a session of its own -- and killing the terminal first would orphan it
    out of reach. Each process is asked to stop (SIGTERM, which mprime and
    y-cruncher handle by finishing cleanly), then anything still there after
    a few seconds is killed.
    """
    if process is None or process.poll() is not None:
        return
    if WINDOWS:
        try:
            subprocess.run(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                capture_output=True,
                timeout=15,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
        except Exception:
            pass
    else:
        tree = _descendants(process.pid) + [process.pid]
        for pid in tree:
            try:
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass
        deadline = time.time() + 5
        while time.time() < deadline and any(
                _alive(pid) for pid in tree if pid != process.pid):
            process.poll()
            time.sleep(0.1)
        for pid in tree:
            if _alive(pid):
                try:
                    os.kill(pid, signal.SIGKILL)
                except OSError:
                    pass
    try:
        process.wait(timeout=10)
    except Exception:
        try:
            process.kill()
        except Exception:
            pass
