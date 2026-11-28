# A stand-in for `wsl.exe`, for the tests of running the AI agents from Windows with the tools in WSL. It takes the
# command lines Limoniter gives wsl.exe, and runs what they ask on this computer.
#
# Its own options come first: --config FILE, a JSON file that says
#   "tools":  {"claude": "/home/a/.local/bin/claude"}   what `command -v NAME` finds, for each distribution named
#   "run":    {"/home/a/.local/bin/claude": ["python", "fake_claude.py", ...]}   what that path runs
#   "log":    a file to keep each command line it is given in
# Then wsl.exe's own: -d NAME, --cd DIR, and --exec or -- followed by the command.
import json
import os
import subprocess
import sys

args = sys.argv[1:]
config = {}
if args and args[0] == "--config":
    with open(args[1]) as f:
        config = json.load(f)
    args = args[2:]

if config.get("log"):
    with open(config["log"], "a", newline="") as log:
        log.write(json.dumps(args) + "\n")

distribution = None
folder = None
command = []
while args:
    word = args.pop(0)
    if word in ("-d", "--distribution"):
        distribution = args.pop(0)
    elif word == "--cd":
        folder = args.pop(0)
    elif word in ("--exec", "-e", "--"):
        command = args
        break
    else:
        command = [word] + args
        break

if distribution not in config.get("distributions", [distribution]):
    sys.stderr.write("There is no distribution with the supplied name.\n")
    sys.exit(1)

# sh -lc SCRIPT [NAME ARGS...]
if command[:2] == ["sh", "-lc"]:
    script = command[2]
    if script.startswith("command -v "):
        found = config.get("tools", {}).get(distribution, {}).get(script[len("command -v "):])
        if found:
            print(found)
            sys.exit(0)
        sys.exit(1)
    if script == 'exec "$@"':
        command = command[4:]

run = config.get("run", {}).get(command[0])
if run is None:
    sys.stderr.write("not found: " + command[0] + "\n")
    sys.exit(127)
run = run + command[1:]


def real(path):
    # A path in WSL's form is Windows' own on Windows.
    if os.name == "nt" and path.startswith("/mnt/") and len(path) >= 6:
        return path[5].upper() + ":" + path[6:].replace("/", "\\")
    return path


env = dict(os.environ)
if os.name == "nt":
    env["LIMONITER_FAKE_MNT"] = "1"
cwd = real(folder) if folder and os.path.isdir(real(folder)) else None
sys.exit(subprocess.call(run, cwd=cwd, env=env))
