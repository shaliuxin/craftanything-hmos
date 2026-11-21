"""Minimal stand-in for `claude -p`, used only to verify the OHOS exec bridge end to end.

It speaks the subset of Claude Code's JSON-lines protocol that craft-anything's
ClaudeCode adapter parses (system/init, assistant/text, result). Not a real agent.
"""
import json
import sys

if "--version" in sys.argv:
    print("claude 2.0.0 (mock bridge agent)")
    sys.exit(0)

try:
    prompt = sys.stdin.read().strip()
except Exception:
    prompt = ""

print(json.dumps({
    "type": "system",
    "subtype": "init",
    "session_id": "mock-bridge-session",
    "mcp_servers": [],
}, ensure_ascii=False))
print(json.dumps({
    "type": "assistant",
    "session_id": "mock-bridge-session",
    "message": {"content": [
        {"type": "text", "text": "外部进程桥已连通。收到：" + prompt[:80]},
    ]},
}, ensure_ascii=False))
print(json.dumps({
    "type": "result",
    "subtype": "success",
    "session_id": "mock-bridge-session",
    "result": "mock agent finished",
    "is_error": False,
    "total_cost_usd": 0.0,
    "usage": {"input_tokens": 1, "output_tokens": 1},
}, ensure_ascii=False))
