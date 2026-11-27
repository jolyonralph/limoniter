# A stand-in for the `claude` program, for the tests of the AI agents panel: it reads the JSON lines Limoniter
# sends and answers with lines recorded from Claude Code 2.1.332, one scenario for each first word of a message.
# Every line it's sent is kept in the file named by --log, so a test can see what was asked of it.
import json
import os
import sys

# Options come before the program's own arguments: --log FILE, --no-limits, --no-bypass.
ARGS = sys.argv[1:]
LOG = ARGS[ARGS.index("--log") + 1] if "--log" in ARGS else None
SESSION = "fake-session-1"
MODEL = "claude-haiku-5-5"
mode = "default"
pending = {}
slow = False
totals = {"inputTokens": 0, "outputTokens": 0, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0}
counter = [0]


def out(message):
    sys.stdout.write((message if isinstance(message, str) else json.dumps(message)) + "\n")
    sys.stdout.flush()


def reply(request_id, response=None, error=None):
    if error:
        out({"type": "control_response", "response": {"subtype": "error", "request_id": request_id, "error": error}})
    else:
        out({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response or {}}})


def models():
    levels = ["low", "medium", "high", "xhigh", "max"]
    return [
        {"value": "default", "displayName": "Default (recommended)", "supportedEffortLevels": levels},
        {"value": "haiku", "displayName": "Haiku 5.5", "supportedEffortLevels": ["low", "high"]},
    ]


def result(subtype="success", is_error=False, text="done"):
    totals["inputTokens"] += 10
    totals["outputTokens"] += 20
    totals["cacheReadInputTokens"] += 100
    out({
        "type": "result", "subtype": subtype, "is_error": is_error, "result": text, "session_id": SESSION,
        "total_cost_usd": 0.0125, "usage": {"input_tokens": 10, "output_tokens": 20},
        "modelUsage": {MODEL: dict(totals)}, "terminal_reason": "completed",
    })


def assistant(blocks, parent=None, message_id=None):
    counter[0] += 1
    out({"type": "assistant", "parent_tool_use_id": parent, "session_id": SESSION,
         "message": {"id": message_id or "msg_%d" % counter[0], "role": "assistant", "model": MODEL, "content": blocks}})


def tool_result(tool_id, text, is_error=False, parent=None):
    out({"type": "user", "parent_tool_use_id": parent, "session_id": SESSION,
         "message": {"role": "user", "content": [{"tool_use_id": tool_id, "type": "tool_result", "content": text, "is_error": is_error}]}})


def stream_text(message_id, pieces):
    def event(e):
        out({"type": "stream_event", "event": e, "session_id": SESSION, "parent_tool_use_id": None})
    event({"type": "message_start", "message": {"id": message_id, "role": "assistant"}})
    event({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    for piece in pieces:
        event({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}})
    assistant([{"type": "text", "text": "".join(pieces)}], message_id=message_id)
    event({"type": "content_block_stop", "index": 0})


def ask(tool, tool_input, then, suggestions=None):
    counter[0] += 1
    request_id = "perm-%d" % counter[0]
    tool_id = "toolu_%d" % counter[0]
    assistant([{"type": "tool_use", "id": tool_id, "name": tool, "input": tool_input}])
    request = {"subtype": "can_use_tool", "tool_name": tool, "input": tool_input, "tool_use_id": tool_id}
    if suggestions:
        request["permission_suggestions"] = suggestions
    out({"type": "control_request", "request_id": request_id, "request": request})
    pending[request_id] = lambda response: then(tool_id, response)


def finish(message_id, text):
    stream_text(message_id, [text])
    result()


def edit_scenario(path, kind):
    if kind == "write":
        tool, tool_input = "Write", {"file_path": path, "content": "written\n"}
    else:
        tool, tool_input = "Edit", {"file_path": path, "old_string": "two", "new_string": "three", "replace_all": False}

    def then(tool_id, response):
        body = response.get("response", {})
        if body.get("behavior") == "allow":
            given = body.get("updatedInput", {})
            if tool == "Write":
                open(given["file_path"], "w", newline="").write(given["content"])
            else:
                text = open(given["file_path"], newline="").read().replace(given["old_string"], given["new_string"], 1)
                open(given["file_path"], "w", newline="").write(text)
            tool_result(tool_id, "The file has been updated successfully.")
            finish("msg_edit_done", "Edited.")
        else:
            tool_result(tool_id, "The user doesn't want to proceed: " + body.get("message", ""), True)
            finish("msg_edit_refused", "Not edited.")

    ask(tool, tool_input, then, [{"type": "setMode", "mode": "acceptEdits", "destination": "session"}])


def bash_scenario():
    command = "cargo test"

    def then(tool_id, response):
        body = response.get("response", {})
        if body.get("behavior") == "allow":
            tool_result(tool_id, "running 3 tests\ntest result: FAILED. 0 passed; 3 failed")
            finish("msg_bash_done", "Three failed.")
        else:
            tool_result(tool_id, "The user doesn't want to proceed with this tool use.", True)
            finish("msg_bash_refused", "Not run.")

    ask("Bash", {"command": command, "description": "Run the tests"}, then,
        [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": command}], "behavior": "allow", "destination": "localSettings"}])


def agent_scenario():
    tool_id = "toolu_agent"
    assistant([{"type": "tool_use", "id": tool_id, "name": "Agent", "input": {"description": "Read the files", "prompt": "read", "subagent_type": "general-purpose"}}])
    out({"type": "system", "subtype": "task_started", "task_id": "task1", "tool_use_id": tool_id, "description": "Read the files", "subagent_type": "general-purpose", "task_type": "local_agent", "session_id": SESSION})
    assistant([{"type": "tool_use", "id": "toolu_child", "name": "Read", "input": {"file_path": "/x/a.txt"}}], parent=tool_id)
    out({"type": "system", "subtype": "task_progress", "task_id": "task1", "tool_use_id": tool_id, "description": "Reading a.txt", "subagent_type": "general-purpose",
         "usage": {"total_tokens": 15026, "tool_uses": 1, "duration_ms": 1418}, "last_tool_name": "Read", "session_id": SESSION})
    out({"type": "system", "subtype": "task_progress", "task_id": "task1", "tool_use_id": tool_id, "description": "Reading b.txt", "subagent_type": "general-purpose",
         "usage": {"total_tokens": 16100, "tool_uses": 2, "duration_ms": 2418}, "last_tool_name": "Read", "session_id": SESSION})
    out({"type": "system", "subtype": "task_notification", "task_id": "task1", "tool_use_id": tool_id, "status": "completed", "summary": "Read the files", "output_file": "", "session_id": SESSION})
    tool_result(tool_id, "They hold alpha and beta.")
    finish("msg_agent_done", "The files hold alpha and beta.")


def user_message(content):
    global slow
    text = content if isinstance(content, str) else "".join(p.get("text", "") for p in content)
    word, _, rest = text.partition(" ")
    out({"type": "system", "subtype": "init", "session_id": SESSION, "model": MODEL, "permissionMode": mode, "cwd": os.getcwd(), "tools": [], "mcp_servers": []})
    if word == "hello":
        stream_text("msg_hello", ["Hel", "lo ", "there"])
        result()
    elif word in ("edit", "write"):
        edit_scenario(rest.strip(), word)
    elif word == "bash":
        bash_scenario()
    elif word == "agent":
        agent_scenario()
    elif word == "slow":
        slow = True
        stream_text("msg_slow", ["Working on it"])
    elif word == "garbage":
        out("this is not json at all")
        out('{"type":"assistant","message":{"id":"x","content":[')
        stream_text("msg_garbage", ["Still here"])
        result()
    elif word == "title":
        out({"type": "system", "subtype": "session_title_changed", "title": "Tidy the parser", "session_id": SESSION})
        finish("msg_title", "Titled.")
    elif word == "limit":
        out({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "unifiedWindows": {"five_hour": {"utilization": 0.5, "resetsAt": 1900000000, "status": "allowed"}}}, "session_id": SESSION})
        finish("msg_limit", "Limited.")
    elif word == "die":
        sys.stderr.write("something went badly wrong\n")
        sys.exit(3)
    else:
        finish("msg_echo", "You said: " + text)


def control(message):
    global slow, mode
    request_id = message["request_id"]
    request = message["request"]
    kind = request["subtype"]
    if kind == "initialize":
        reply(request_id, {"models": models(), "current_permission_mode": mode, "account": {}})
    elif kind == "get_context_usage":
        reply(request_id, {"categories": [], "totalTokens": 250000, "maxTokens": 1000000, "percentage": 25})
    elif kind == "get_usage":
        if "--no-limits" in ARGS:
            reply(request_id, {"rate_limits_available": False, "rate_limits": None})
        else:
            reply(request_id, {"rate_limits_available": True, "rate_limits": {
                "five_hour": {"utilization": 42, "resets_at": "2030-01-01T10:00:00+00:00"},
                "seven_day": {"utilization": 19, "resets_at": "2030-01-05T10:00:00+00:00"}, "seven_day_opus": None}})
    elif kind == "set_model":
        reply(request_id, {"model": "fake-" + str(request.get("model") or "default")})
    elif kind == "set_permission_mode":
        if request["mode"] == "bypassPermissions" and "--no-bypass" in ARGS:
            reply(request_id, error="Cannot set permission mode to bypassPermissions")
        else:
            mode = request["mode"]
            reply(request_id, {"mode": mode})
    elif kind == "interrupt":
        reply(request_id, {"still_queued": []})
        if slow:
            slow = False
            tool_result_note = {"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}}
            out(tool_result_note)
            result("error_during_execution", True, "")
    elif kind in ("apply_flag_settings", "stop_task"):
        reply(request_id, {})
    else:
        reply(request_id, error="unknown request " + kind)


def main():
    if LOG:
        with open(LOG, "a") as log:
            log.write(json.dumps({"argv": ARGS}) + "\n")
    for line in sys.stdin:
        if LOG:
            with open(LOG, "a") as log:
                log.write(line.rstrip("\n") + "\n")
        try:
            message = json.loads(line)
        except ValueError:
            continue
        kind = message.get("type")
        if kind == "control_request":
            control(message)
        elif kind == "control_response":
            handler = pending.pop(message["response"]["request_id"], None)
            if handler:
                handler(message["response"])
        elif kind == "user":
            user_message(message["message"]["content"])


main()
