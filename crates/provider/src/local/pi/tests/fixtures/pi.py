#!/usr/bin/env python3
"""Deterministic Pi RPC peer; no model API or user credentials."""
import json
import os
import sys
import time
import uuid
from pathlib import Path

scenario = os.environ.get('PI_FIXTURE_SCENARIO', '')
if '--version' in sys.argv:
    with Path(__file__).with_name('versions.log').open('a') as log:
        log.write('probe\n')
    if scenario == 'version-timeout':
        time.sleep(10)
    if scenario == 'version-exit':
        sys.exit(3)
    print({'old': '0.73.1', 'version-malformed': 'not-a-version',
           'version-prerelease': '1.3.0-beta.2', 'version-build': '1.3.0+build',
           'version-extended': '1.3.0.1', 'version-short': '1.2', 'version-oversized': '1' * 4097}.get(scenario, '1.2.0'))
    sys.exit(0)
root = Path.cwd()
path = root / 'session.jsonl'
args = sys.argv[1:]
(root / 'arguments.json').write_text(json.dumps(args))
if '--session' in args:
    path = Path(args[args.index('--session') + 1])
    identity = json.loads(path.read_text().splitlines()[0])['id']
elif '--session-id' in args:
    identity = args[args.index('--session-id') + 1]
else:
    identity = str(uuid.uuid4())
running = False
finish_after_state = False
compacting = False
invalid_settlement = False
state_requests = 0
model = {'provider': 'fixture', 'id': 'model', 'name': 'Fixture Model', 'input': ['text', 'image'],
         'reasoning': scenario != 'no-reasoning'}
thinking = 'medium'
native_thinking = {
    'bad-thinking-state': 'future',
    'none-thinking-state': 'none',
    'ultra-thinking-state': 'ultra',
    'null-thinking-state': None,
    'malformed-thinking-state': 42,
}
if scenario == 'mapped-thinking':
    model['thinkingLevelMap'] = {'medium': None, 'high': 'high', 'xhigh': 'xhigh', 'max': None}
    thinking = 'high'
available_models = [model]
if scenario == 'multiple-models':
    available_models.append(dict(model, id='other', name='Other Model'))
    available_models.append(dict(model, id='clamped', name='Clamped Model'))

def emit(value):
    print(json.dumps(value), flush=True)

def response(command, data=None, success=True):
    emit({'id': command['id'], 'type': 'response', 'command': command['type'], 'success': success, 'data': data})

for line in sys.stdin:
    command = json.loads(line)
    with (root / 'requests.jsonl').open('a') as log:
        log.write(json.dumps(command) + '\n')
    kind = command['type']
    if scenario == 'hang':
        continue
    if scenario == 'wrong-response':
        command['id'] = 'foreign'
    if kind == 'get_state':
        if '--no-session' in args:
            if scenario == 'discovery-state-slow':
                time.sleep(0.4)
            if scenario == 'discovery-state-rejected':
                response(command, success=False)
                continue
            if scenario == 'discovery-state-timeout':
                continue
            if scenario == 'discovery-state-exit':
                sys.exit(2)
            if scenario == 'discovery-state-malformed':
                print('not json', flush=True)
                continue
            if scenario == 'discovery-state-missing-model':
                response(command, {})
                continue
        state_requests += 1
        if invalid_settlement:
            invalid_settlement = False
            emit({'type': 'compaction_start', 'reason': 'unknown'})
        if scenario == 'idle':
            for event in ['entry_appended', 'session_info_changed', 'queue_update', 'extension_error']:
                emit({'type': event})
            emit({'type': 'thinking_level_changed', 'level': thinking})
            emit({'type': 'compaction_start', 'reason': 'manual'})
            emit({'type': 'compaction_end', 'reason': 'manual', 'result': {}, 'aborted': False})
            emit({'type': 'agent_settled', 'aborted': False})
        swapped_at = {'session-swap-open': 1, 'session-swap-configure': 2,
                      'session-swap-synchronize': 2}.get(scenario)
        response_identity = 'foreign' if state_requests == swapped_at else identity
        state = {'model': model, 'sessionId': response_identity, 'sessionFile': str(path),
                 'thinkingLevel': native_thinking.get(scenario, thinking),
                 'isStreaming': running, 'isCompacting': compacting}
        if scenario == 'missing-thinking-state' or (scenario == 'thinking-state-disappears' and state_requests > 1):
            del state['thinkingLevel']
        if scenario.startswith('idle-thinking-') and state_requests == 1:
            thinking = 'future' if scenario == 'idle-thinking-future' else 'low'
            event = {'type': 'thinking_level_changed', 'level': thinking}
            if scenario == 'idle-thinking-missing':
                del event['level']
            elif scenario == 'idle-thinking-malformed':
                event['level'] = 42
            emit(event)
        response(command, state)
        if compacting and scenario != 'compaction-hang':
            compacting = False
            emit({'type': 'compaction_end', 'reason': 'manual', 'result': {}})
        if finish_after_state:
            finish_after_state = False
            running = False
            emit({'type': 'message_start', 'message': {'role': 'assistant'}})
            emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type': 'text', 'text': 'Extension'}], 'stopReason': 'stop'}})
            emit({'type': 'agent_settled', 'aborted': False})
    elif kind == 'get_available_models':
        if scenario == 'discovery-models-rejected':
            response(command, success=False)
            continue
        models = {'bad-models': None, 'bad-model-label': [dict(model, name='bad\nlabel')]}.get(scenario, available_models)
        response(command, {'models': models})
    elif kind == 'set_model':
        selected = next((candidate for candidate in available_models
                         if candidate['provider'] == command['provider'] and candidate['id'] == command['modelId']), None)
        if selected is not None:
            model = selected
            thinking = 'low' if model['id'] == 'other' else ('high' if scenario == 'mapped-thinking' else 'medium')
        response(command, model, selected is not None)
    elif kind == 'set_thinking_level':
        if scenario == 'reject-thinking':
            response(command, success=False)
            continue
        thinking = 'high' if scenario == 'clamp-thinking' else ('low' if model['id'] == 'clamped' else command['level'])
        emit({'type': 'thinking_level_changed', 'level': thinking})
        response(command)
    elif kind == 'prompt':
        text = command['message']
        if text in ['thinking-before', 'thinking-after', 'thinking-handled', 'thinking-invalid']:
            if text == 'thinking-after':
                response(command, {'disposition': 'started'})
            level = {'thinking-event-future': 'future', 'thinking-event-none': 'none',
                     'thinking-event-ultra': 'ultra', 'thinking-event-null': None,
                     'thinking-event-malformed': 42}.get(scenario, 'low')
            if text == 'thinking-invalid':
                level = 'future'
            event = {'type': 'thinking_level_changed', 'level': level}
            if scenario == 'thinking-event-missing':
                del event['level']
            elif isinstance(level, str):
                thinking = level
            emit(event)
            if text != 'thinking-after':
                response(command, {'disposition': 'handled' if text == 'thinking-handled' else 'started'})
            emit({'type': 'agent_settled', 'aborted': False})
            continue
        if text == 'reject':
            response(command, success=False)
            continue
        if text == 'exit':
            sys.exit(2)
        if text == 'malformed':
            print('not json', flush=True)
            continue
        if text == 'slow':
            time.sleep(0.6)
        if text == 'never-ack':
            continue
        if text.startswith('invalid-admission-'):
            events = {
                'content': {'type': 'message_end', 'message': {'role': 'assistant', 'content': None}},
                'index': {'type': 'message_update', 'assistantMessageEvent': {'type': 'text_delta', 'delta': 'x'}},
                'usage': {'type': 'message_end', 'message': {'role': 'assistant', 'content': [], 'usage': {'cost': {'total': -1}}}},
                'compaction': {'type': 'compaction_start', 'reason': 'unknown'},
            }
            suffix = text[len('invalid-admission-'):]
            if suffix == 'messages':
                for _ in range(4097):
                    emit({'type': 'message_start', 'message': {'role': 'assistant'}})
            else:
                emit(events[suffix])
            response(command, {'disposition': 'handled'})
            continue
        if text.startswith('dialog-'):
            method = text[len('dialog-'):]
            emit({'type': 'extension_ui_request', 'id': 'dialog', 'method': method})
            response(command, {'disposition': 'handled'})
            continue
        if text == 'ui-notifications':
            for method in ['notify', 'setStatus', 'setWidget', 'setTitle', 'set_editor_text']:
                emit({'type': 'extension_ui_request', 'id': method, 'method': method})
            response(command, {'disposition': 'handled'})
            continue
        if text == 'handled-compaction':
            compacting = True
            emit({'type': 'compaction_start', 'reason': 'manual'})
            response(command, {'disposition': 'handled'})
            continue
        if text == 'handled-invalid-settlement':
            running = True
            invalid_settlement = True
            response(command, {'disposition': 'handled'})
            continue
        if text in ['handled-many', 'handled-limit', 'handled-deltas']:
            emit({'type': 'agent_start'})
            emit({'type': 'message_start', 'message': {'role': 'assistant'}})
            for i in range(5000 if text in ['handled-limit', 'handled-deltas'] else 300):
                emit({'type': 'message_update', 'assistantMessageEvent': {'type': 'text_delta', 'contentIndex': i if text == 'handled-limit' else 0, 'delta': 'x' * (32768 if text == 'handled-many' else 1)}})
            if text == 'handled-many':
                for i in range(300):
                    emit({'type': 'tool_execution_start', 'toolCallId': str(i), 'toolName': 'bash', 'args': {}})
                    emit({'type': 'tool_execution_end', 'toolCallId': str(i), 'toolName': 'bash', 'result': {}, 'isError': False})
            emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type': 'text', 'text': 'Extension'}], 'stopReason': 'stop'}})
            emit({'type': 'agent_settled', 'aborted': False})
            response(command, {'disposition': 'handled'})
            continue
        if text == 'handled-backlog':
            running = True
            emit({'type': 'agent_start'})
            for i in range(300):
                emit({'type': 'tool_execution_start', 'toolCallId': str(i), 'toolName': 'bash', 'args': {}})
                emit({'type': 'tool_execution_end', 'toolCallId': str(i), 'toolName': 'bash', 'result': {}, 'isError': False})
            emit({'type': 'tool_execution_start', 'toolCallId': 'pending', 'toolName': 'bash', 'args': {}})
            response(command, {'disposition': 'handled'})
            continue
        if text == 'handled-running':
            path.write_text(json.dumps({'id': identity}))
            running = True
            finish_after_state = True
            emit({'type': 'agent_start'})
            response(command, {'disposition': 'handled'})
            continue
        if text in ['handled-before', 'handled-after']:
            path.write_text(json.dumps({'id': identity}))
            if text == 'handled-after':
                response(command, {'disposition': 'handled'})
            emit({'type': 'agent_start'})
            emit({'type': 'message_start', 'message': {'role': 'assistant'}})
            emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type': 'text', 'text': 'Extension'}], 'stopReason': 'stop'}})
            emit({'type': 'agent_settled', 'aborted': False})
            if text == 'handled-before':
                response(command, {'disposition': 'handled'})
            continue
        if text == 'handled':
            response(command, {'disposition': 'handled'})
            continue
        path.write_text(json.dumps({'id': identity}))
        # Fast native events may precede the admission response.
        emit({'type': 'agent_start'})
        response(command, {'disposition': 'started'})
        if text == 'frame-boundary':
            record = json.dumps({'type': 'entry_appended'})
            sys.stdout.write(record + ' ' * (2 * 1024 * 1024 - len(record) - 1) + '\n')
            sys.stdout.flush()
        if text == 'oversized':
            emit({'type': 'tool_execution_end', 'secret': 'x' * (2 * 1024 * 1024)})
            continue
        if text == 'stderr-auth':
            print('invalid api key: secret-value', file=sys.stderr, flush=True)
            sys.exit(2)
        if text == 'die-after-admission':
            sys.exit(2)
        if text == 'wait':
            continue
        if text == 'dialog':
            emit({'type': 'extension_ui_request', 'id': 'dialog', 'method': 'confirm', 'title': 'Approve?'})
            continue
        emit({'type': 'tool_execution_start', 'toolCallId': 'tool', 'toolName': 'bash', 'args': {'command': 'echo test'}})
        emit({'type': 'tool_execution_end', 'toolCallId': 'tool', 'toolName': 'bash', 'result': {'content': [{'type': 'text', 'text': 'test'}]}, 'isError': False})
        emit({'type': 'message_start', 'message': {'role': 'assistant'}})
        emit({'type': 'message_update', 'assistantMessageEvent': {'type': 'text_delta', 'contentIndex': 0, 'delta': 'Hello'}})
        emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type': 'text', 'text': 'Hello'}],
             'stopReason': 'error' if text == 'fail' else 'stop', 'errorMessage': 'quota exhausted: secret-value' if text == 'fail' else '', 'usage': {'input': 12, 'output': 3, 'cacheRead': 2, 'cost': {'total': 0}}}})
        emit({'type': 'agent_end', 'messages': []})
        emit({'type': 'agent_settled', 'aborted': False})
    elif kind == 'abort':
        if running:
            running = False
            emit({'type': 'tool_execution_end', 'toolCallId': 'pending', 'toolName': 'bash', 'result': {}, 'isError': True})
        emit({'type': 'agent_end', 'messages': []})
        emit({'type': 'agent_settled', 'aborted': True})
        response(command)
    elif kind == 'extension_ui_response':
        assert command['cancelled'] is True
        emit({'type': 'agent_settled', 'aborted': False})
    else:
        response(command, success=False)
