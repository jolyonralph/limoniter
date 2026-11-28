# A stand-in for `codex app-server`, for the tests of the AI agents panel: it reads the JSON-RPC lines Limoniter
# sends and answers with messages shaped as recorded from codex-cli 0.172.0, one scenario for each first word of a
# message. Every line it's sent is kept in the file named by --log, so a test can see what was asked of it.
# Options come before the program's own arguments: --log FILE, --refuse-context (turn/start with additionalContext
# is refused, as a Codex without the experimental field would).
import json
import os
import sys


def real(path):
    """Where a path the program was given is on this computer. Run through the stand-in for wsl.exe on Windows, a
    path in WSL's form (/mnt/c/Users/a) is Windows' own (C:\\Users\\a), as WSL would reach it."""
    if os.environ.get("LIMONITER_FAKE_MNT") and len(path) >= 7 and path.startswith("/mnt/") and path[6:7] in ("/", ""):
        return path[5].upper() + ":" + path[6:].replace("/", "\\")
    return path
import time
import urllib.request

ARGS = sys.argv[1:]
LOG = ARGS[ARGS.index("--log") + 1] if "--log" in ARGS else None
REFUSE_CONTEXT = "--refuse-context" in ARGS
THREAD = "thread-fake-1"
def mcp_call(name, arguments):
    """Calls one of Limoniter's tools over MCP, with the server and headers the thread was started with."""
    config = state["mcp"]
    headers = dict(config["http_headers"])
    headers["Content-Type"] = "application/json"
    body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}
    request = urllib.request.Request(config["url"], data=json.dumps(body).encode(), headers=headers)
    return json.load(urllib.request.urlopen(request, timeout=10))["result"]["content"][0]["text"]


state = {"mcp": None, "turn": 0, "model": "gpt-5.6-luna", "answers": {}, "next_server_id": 0, "interrupted": False}


def out(message):
    sys.stdout.write((message if isinstance(message, str) else json.dumps(message, separators=(",", ":"))) + "\n")
    sys.stdout.flush()


def note(method, params):
    out({"method": method, "params": params})


def result(id, body):
    out({"id": id, "result": body})


def read():
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    if LOG:
        with open(LOG, "a", newline="") as log:
            log.write(line if line.endswith("\n") else line + "\n")
    return json.loads(line)


def model(name, display, default, hidden=False, efforts=("low", "medium", "high")):
    return {"id": name, "model": name, "upgrade": None, "displayName": display, "description": "", "hidden": hidden,
            "supportedReasoningEfforts": [{"reasoningEffort": e, "description": ""} for e in efforts],
            "defaultReasoningEffort": "low", "inputModalities": ["text"], "isDefault": default}


def item_started(turn, item):
    note("item/started", {"item": item, "threadId": THREAD, "turnId": turn, "startedAtMs": 1})


def item_completed(turn, item):
    note("item/completed", {"item": item, "threadId": THREAD, "turnId": turn, "completedAtMs": 2})


def usage(turn, thread=THREAD, total=1500):
    note("thread/tokenUsage/updated", {"threadId": thread, "turnId": turn, "tokenUsage": {
        "total": {"totalTokens": total, "inputTokens": total - 100, "cachedInputTokens": 400, "cacheWriteInputTokens": 0, "outputTokens": 100, "reasoningOutputTokens": 20},
        "last": {"totalTokens": 900, "inputTokens": 850, "cachedInputTokens": 400, "cacheWriteInputTokens": 0, "outputTokens": 50, "reasoningOutputTokens": 10},
        "modelContextWindow": 258400}})
    if thread == THREAD:
        note("account/rateLimits/updated", {"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 11, "windowDurationMins": 300, "resetsAt": 1795839145},
                                                         "secondary": {"usedPercent": 59, "windowDurationMins": 10080, "resetsAt": 1796252461}, "planType": "plus"}})


def ask(method, params):
    """A request from the server to the client, and what it answers (other messages wait)."""
    sid = state["next_server_id"]
    state["next_server_id"] += 1
    out({"method": method, "id": sid, "params": params})
    while True:
        message = read()
        if message.get("id") == sid and "method" not in message:
            return message.get("result")
        if message.get("method") == "turn/interrupt":
            state["interrupted"] = True
            result(message["id"], {})
            return None


def finish(turn, status="completed", error=None):
    note("turn/completed", {"threadId": THREAD, "turn": {"id": turn, "items": [], "status": status, "error": error}})


def text_message(turn, mid, pieces):
    item_started(turn, {"type": "agentMessage", "id": mid, "text": "", "phase": "final_answer"})
    for piece in pieces:
        note("item/agentMessage/delta", {"threadId": THREAD, "turnId": turn, "itemId": mid, "delta": piece})
    item_completed(turn, {"type": "agentMessage", "id": mid, "text": "".join(pieces), "phase": "final_answer"})


def command_item(turn, cid, command, status="inProgress", output=None, code=None):
    return {"type": "commandExecution", "id": cid, "command": "/bin/bash -lc '%s'" % command, "cwd": os.getcwd(), "source": "agent",
            "status": status, "commandActions": [{"type": "unknown", "command": command}], "aggregatedOutput": output, "exitCode": code}


def run_turn(turn, text):
    words = text.split()
    first = words[0] if words else ""
    if first == "say":
        item_started(turn, {"type": "reasoning", "id": "rs1", "summary": [], "content": []})
        note("item/reasoning/summaryPartAdded", {"threadId": THREAD, "turnId": turn, "itemId": "rs1", "summaryIndex": 0})
        note("item/reasoning/summaryTextDelta", {"threadId": THREAD, "turnId": turn, "itemId": "rs1", "delta": "**Thinking**", "summaryIndex": 0})
        note("item/reasoning/summaryTextDelta", {"threadId": THREAD, "turnId": turn, "itemId": "rs1", "delta": " it over", "summaryIndex": 0})
        item_completed(turn, {"type": "reasoning", "id": "rs1", "summary": ["**Thinking** it over"], "content": []})
        text_message(turn, "m1", ["Hel", "lo ", "there"])
        usage(turn)
    elif text.startswith("You are the planner"):
        for title in ("Plan step one", "Plan step two"):
            mcp_call("add_task", {"title": title, "details": "from the planner"})
        text_message(turn, "mp", ["Planned."])
        usage(turn)
    elif first == "board":
        payload = text.split(None, 2)[2] if len(words) > 2 else "{}"
        text_message(turn, "mb" + turn, [mcp_call(words[1], json.loads(payload))])
        usage(turn)
    elif first == "command":
        item_started(turn, command_item(turn, "c1", "touch b.txt"))
        reply = ask("item/commandExecution/requestApproval", {"kind": "command", "threadId": THREAD, "turnId": turn, "itemId": "c1",
                    "command": "/bin/bash -lc 'touch b.txt'", "cwd": os.getcwd(), "availableDecisions": ["accept", "acceptForSession", "cancel"]})
        decision = (reply or {}).get("decision")
        state["answers"]["c1"] = decision
        if decision in ("accept", "acceptForSession"):
            item_completed(turn, command_item(turn, "c1", "touch b.txt", "completed", "", 0))
            text_message(turn, "m2", ["done"])
        else:
            item_completed(turn, command_item(turn, "c1", "touch b.txt", "declined"))
            text_message(turn, "m2", ["not allowed"])
        usage(turn)
    elif first in ("edit", "multi", "add"):
        paths = words[1:]
        changes = []
        for path in paths:
            if first == "add":
                changes.append({"path": path, "kind": {"type": "add"}, "diff": "hello\n"})
            else:
                changes.append({"path": path, "kind": {"type": "update", "move_path": None}, "diff": "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"})
        item = {"type": "fileChange", "id": "f1", "changes": changes, "status": "inProgress"}
        item_started(turn, item)
        reply = ask("item/fileChange/requestApproval", {"threadId": THREAD, "turnId": turn, "itemId": "f1", "reason": None, "grantRoot": None})
        decision = (reply or {}).get("decision")
        state["answers"]["f1"] = decision
        if decision in ("accept", "acceptForSession"):
            for change in changes:
                if change["kind"]["type"] == "add":
                    with open(real(change["path"]), "w", newline="") as f:
                        f.write("hello\n")
                else:
                    with open(real(change["path"]), newline="") as f:
                        old = f.read()
                    with open(real(change["path"]), "w", newline="") as f:
                        f.write(old.replace("two", "TWO", 1))
            item_completed(turn, dict(item, status="completed"))
            text_message(turn, "m3", ["changed"])
        else:
            item_completed(turn, dict(item, status="declined"))
            text_message(turn, "m3", ["refused"])
        usage(turn)
    elif first == "agents":
        spawn = {"type": "collabAgentToolCall", "id": "k1", "tool": "spawnAgent", "status": "inProgress", "senderThreadId": THREAD,
                 "receiverThreadIds": ["child-1"], "prompt": "Look through the tests\nand report", "agentsStates": {"child-1": {"status": "running", "message": None}}}
        item_started(turn, spawn)
        note("turn/started", {"threadId": "child-1", "turn": {"id": "child-turn", "items": [], "status": "inProgress"}})
        note("item/started", {"item": command_item(turn, "cc1", "ls"), "threadId": "child-1", "turnId": "child-turn"})
        usage(turn, thread="child-1", total=700)
        item_completed(turn, dict(spawn, status="completed"))
        done = dict(spawn, tool="wait", id="k2", status="completed", agentsStates={"child-1": {"status": "completed", "message": "ok"}})
        item_completed(turn, done)
        text_message(turn, "m4", ["all reported"])
        usage(turn)
    elif first == "slow":
        text_message(turn, "m5", ["working"])
        for _ in range(200):
            if state["interrupted"]:
                break
            time.sleep(0.05)
            # Only the interrupt matters; anything else waits.
    elif first == "bad":
        sys.stdout.write("this is not json\n")
        sys.stdout.flush()
        text_message(turn, "m6", ["still here"])
    elif first == "fail":
        note("error", {"error": {"message": "the service said no"}, "willRetry": False, "threadId": THREAD, "turnId": turn})
        finish(turn, "failed", {"message": "the service said no"})
        return
    elif first == "permissions":
        reply = ask("item/permissions/requestApproval", {"threadId": THREAD, "turnId": turn, "itemId": "p1", "cwd": os.getcwd(), "startedAtMs": 1,
                    "reason": "needs the network", "permissions": {"network": {"enabled": True}}})
        state["answers"]["p1"] = reply
        text_message(turn, "m7", ["asked"])
    else:
        text_message(turn, "m0", ["ok"])
    if state["interrupted"]:
        state["interrupted"] = False
        finish(turn, "interrupted")
    else:
        finish(turn)


def main():
    while True:
        message = read()
        method = message.get("method")
        id = message.get("id")
        params = message.get("params") or {}
        if method == "initialize":
            result(id, {"userAgent": "fake/0.172.0", "codexHome": "/tmp/fake", "platformFamily": "unix", "platformOs": "linux"})
        elif method == "model/list":
            result(id, {"data": [model("gpt-6.2-sol", "GPT-6.2-Sol", True, efforts=("low", "medium", "high", "xhigh", "max", "ultra")),
                                 model("gpt-5.6-luna", "GPT-5.6-Luna", False), model("gpt-reserve", "GPT-Reserve", False, hidden=True)]})
        elif method == "account/rateLimits/read":
            result(id, {"rateLimits": {"limitId": "codex", "primary": {"usedPercent": 11, "windowDurationMins": 300, "resetsAt": 1795839145},
                                       "secondary": {"usedPercent": 59, "windowDurationMins": 10080, "resetsAt": 1796252461}}})
        elif method in ("thread/start", "thread/resume"):
            state["model"] = params.get("model") or "gpt-6.2-sol"
            state["mcp"] = ((params.get("config") or {}).get("mcp_servers") or {}).get("limoniter")
            thread = {"id": params.get("threadId", THREAD), "preview": "", "ephemeral": False}
            result(id, {"thread": thread, "model": state["model"], "approvalPolicy": params.get("approvalPolicy", "on-request"),
                        "sandbox": {"type": {"read-only": "readOnly", "workspace-write": "workspaceWrite", "danger-full-access": "dangerFullAccess"}[params.get("sandbox", "workspace-write")]},
                        "reasoningEffort": None, "cwd": params.get("cwd")})
            note("thread/started", {"thread": thread})
        elif method == "turn/start":
            if REFUSE_CONTEXT and "additionalContext" in params:
                out({"id": id, "error": {"code": -32602, "message": "Invalid request: unknown field `additionalContext`"}})
                continue
            state["turn"] += 1
            turn = "turn-%d" % state["turn"]
            result(id, {"turn": {"id": turn, "items": [], "status": "inProgress", "error": None}})
            note("turn/started", {"threadId": THREAD, "turn": {"id": turn, "items": [], "status": "inProgress"}})
            text = " ".join(part.get("text", "") for part in params["input"] if part.get("type") == "text")
            # The person's own message is the last text; notes and context come before it.
            last = [part.get("text", "") for part in params["input"] if part.get("type") == "text"][-1]
            run_turn(turn, last)
        elif method == "turn/interrupt":
            state["interrupted"] = True
            result(id, {})
        elif id is not None and method:
            out({"id": id, "error": {"code": -32601, "message": "unknown " + method}})


main()
