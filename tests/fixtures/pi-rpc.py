#!/usr/bin/env python3
"""Deterministic Pi stand-in; never calls a provider or executes tools."""
import json
import sys


def emit(record):
    sys.stdout.write(json.dumps(record, ensure_ascii=False) + "\n")
    sys.stdout.flush()


for line in sys.stdin.buffer:
    command = json.loads(line)
    kind = command["type"]
    data = {}
    if kind == "get_state":
        data = {"model": {"id": "fixture-model"}, "thinkingLevel": "high", "isStreaming": False}
    if kind == "get_messages":
        data = {"messages": [{"role": "user", "content": "saved history"}]}
    if kind == "prompt":
        emit({"type": "agent_start"})
        content = ([{"type": "text", "text": command["message"]}] + command["images"]) if command.get("images") else command["message"]
        emit({"type": "message_start", "message": {"role": "user", "content": content}})
        emit({"type": "message_end", "message": {"role": "user", "content": content}})
        emit({"type": "message_start", "message": {"role": "assistant", "content": []}})
        emit({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": "one\u2028two\u2029three"}})
        emit({"type": "message_end", "message": {"role": "assistant", "content": [{"type": "text", "text": "one\u2028two\u2029three"}]}})
        emit({"type": "agent_end"})
        emit({"type": "agent_settled"})
        data = {"disposition": "started"}
    emit({"type": "response", "id": command.get("id"), "command": kind, "success": True, "data": data})
