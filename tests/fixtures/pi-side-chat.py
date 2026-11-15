#!/usr/bin/env python3
"""Independent-process side-chat fixture; no model, tools, or saved history."""
import json
import sys

def emit(record):
    print(json.dumps(record), flush=True)

for line in sys.stdin:
    command = json.loads(line)
    kind = command['type']
    data = {}
    if kind == 'get_state':
        data = {'model': {'id': 'fixture-model'}, 'isStreaming': False}
    if kind == 'get_entries':
        # This fixture has no disk history. Keep in-memory messages authoritative.
        continue
    if kind == 'prompt':
        emit({'type': 'agent_start'})
        for role, text in [('user', command['message']), ('assistant', 'Fixture answer: ' + command['message'])]:
            message = {'role': role, 'content': [{'type': 'text', 'text': text}]}
            emit({'type': 'message_start', 'message': message})
            emit({'type': 'message_end', 'message': message})
        emit({'type': 'agent_end'})
        emit({'type': 'agent_settled'})
        data = {'disposition': 'started'}
    emit({'type': 'response', 'id': command.get('id'), 'command': kind, 'success': True, 'data': data})
