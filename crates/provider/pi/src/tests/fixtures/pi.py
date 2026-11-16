#!/usr/bin/env python3
"""Deterministic Pi RPC peer; no model API or user credentials."""
import json
import os
import select
import signal
import sys
import time
import uuid
from pathlib import Path

scenario = os.environ.get('PI_FIXTURE_SCENARIO', '')
if '--version' in sys.argv:
    with Path(__file__).with_name('versions.log').open('a') as log:
        log.write('probe\n')
    if scenario == 'version-slow':
        time.sleep(3)
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
if '--extension' in args:
    # Pi loads the module and registers each servers.json entry with pi.registerMcpServer.
    extension = Path(args[args.index('--extension') + 1])
    source = extension.read_text()
    assert 'registerMcpServer' in source and 'Bearer' not in source
    servers = extension.with_name('servers.json')
    with (root / 'mcp-openings.jsonl').open('a') as log:
        log.write(json.dumps({'directory': str(extension.parent),
                              'mode': oct(servers.stat().st_mode & 0o777),
                              'servers': json.loads(servers.read_text())}) + '\n')
if '--session' in args:
    path = Path(args[args.index('--session') + 1])
    identity = json.loads(path.read_text().splitlines()[0])['id']
elif '--session-id' in args:
    identity = args[args.index('--session-id') + 1]
else:
    identity = str(uuid.uuid4())
session_header = json.dumps({'type': 'session', 'version': 3, 'id': identity, 'cwd': str(root)}) + '\n'
running = False
finish_after_state = False
compacting = False
invalid_settlement = False
state_requests = 0
thinking_rejected = False
control_conflict = False
model = {'provider': 'fixture', 'id': 'model', 'name': 'Fixture Model', 'input': ['text', 'image'],
         'reasoning': scenario != 'no-reasoning'}
thinking = 'medium'
if scenario == 'low-thinking':
    thinking = 'low'
if '--session' in args:
    for entry in map(json.loads, path.read_text().splitlines()[1:]):
        if entry.get('type') == 'thinking_level_change':
            thinking = entry['thinkingLevel']
bounded_levels = {
    'boundary': 'a' * 512,
    'multibyte-boundary': '\u00e9' * 256,
    'oversized': 'a' * 513,
    'large': 'a' * (1024 * 1024),
    'multibyte-oversized': '\u00e9' * 257,
    'control': 'bad\nlevel',
    'nul': 'bad\x00level',
    'unicode-control': 'bad\u0085level',
    'empty': '',
}
native_thinking = {
    'bad-thinking-state': 'future',
    'none-thinking-state': 'none',
    'ultra-thinking-state': 'ultra',
    'null-thinking-state': None,
    'malformed-thinking-state': 42,
}
native_thinking.update({f'{name}-thinking-state': level for name, level in bounded_levels.items()})
if scenario == 'mapped-thinking':
    model['thinkingLevelMap'] = {'medium': None, 'high': 'high', 'xhigh': 'xhigh', 'max': None}
    thinking = 'high'
if scenario.startswith('completions-'):
    model['api'] = 'openai-completions'
    if scenario == 'completions-native-off':
        thinking = 'off'
    if scenario == 'completions-mapped-off':
        model['thinkingLevelMap'] = {'off': 'none'}
available_models = [model]
if scenario.startswith('completions-'):
    available_models.append(dict(model, id='other', thinkingLevelMap={}))
if (scenario in ['multiple-models', 'stale-thinking-event', 'newer-thinking-event', 'unstable-thinking-events',
                 'control-starts-work', 'busy-runtime-change', 'busy-runtime-compacting', 'worded-thinking-rejection',
                 'reject-implicit-thinking', 'reject-thinking', 'discovery-second-model']
        or scenario.endswith('-thinking-state') or scenario.startswith('summary-model-reply')):
    available_models.append(dict(model, id='other', name='Other Model'))
    available_models.append(dict(model, id='clamped', name='Clamped Model'))
if scenario in ['discovery-thinking-defaults', 'strict-thinking-models']:
    available_models.extend([
        dict(model, id='other', name='Other Model'),
        dict(model, id='deepseek-flash', name='High Model', thinkingLevelMap={'medium': None}),
        dict(model, id='low-ceiling', name='Low Model', thinkingLevelMap={'medium': None, 'high': None}),
        dict(model, id='no-reasoning', name='No Reasoning', reasoning=False),
    ])

def emit(value):
    print(json.dumps(value), flush=True)

def response(command, data=None, success=True, error=None):
    emit({'id': command['id'], 'type': 'response', 'command': command['type'], 'success': success, 'data': data, 'error': error})

deferred_history = None

def save_history(data):
    global deferred_history
    if scenario in ['history-on-shutdown', 'partial-history-on-shutdown']:
        deferred_history = data
        if scenario == 'partial-history-on-shutdown':
            path.write_text(session_header[:len(session_header) // 2])
    else:
        path.write_text(data)

def finish_history(_signal, _frame):
    if deferred_history is not None:
        path.write_text(deferred_history)
    sys.exit(0)

def interruptible_lines():
    """Yield stdin lines while letting a signal end the wait for the next one.

    Python defers a handler until the interpreter regains control, so SIGTERM landing just
    before a blocking read would wait for input that shutdown never sends.
    """
    wake, notify = os.pipe()
    os.set_blocking(notify, False)
    signal.set_wakeup_fd(notify)
    pending = b''
    while True:
        *lines, pending = pending.split(b'\n')
        yield from lines
        readable, _, _ = select.select([0, wake], [], [])
        if 0 in readable:
            chunk = os.read(0, 65536)
            if not chunk:
                return
            pending += chunk

stdin_lines = sys.stdin
if scenario in ['history-on-shutdown', 'partial-history-on-shutdown']:
    signal.signal(signal.SIGTERM, finish_history)
    stdin_lines = interruptible_lines()

for line in stdin_lines:
    command = json.loads(line)
    with (root / 'requests.jsonl').open('a') as log:
        log.write(json.dumps(command) + '\n')
    kind = command['type']
    if scenario == 'hang':
        continue
    if scenario == 'wrong-response':
        command['id'] = 'foreign'
    if kind == 'get_state':
        if (root / 'handled-notice').exists():
            (root / 'handled-notice').unlink()
            emit({'type': 'extension_ui_request', 'method': 'notify', 'message': 'handled notice'})
        rejection_readback = (root / 'rejection-readback').read_text() if thinking_rejected and (root / 'rejection-readback').exists() else ''
        if rejection_readback == 'timeout':
            continue
        if rejection_readback == 'exit':
            sys.exit(2)
        if (root / 'state-reply.json').exists():
            response(command, json.loads((root / 'state-reply.json').read_text()))
            continue
        if scenario in ['busy-runtime-change', 'busy-runtime-compacting'] and state_requests > 0:
            model = available_models[1]
            thinking = 'low'
            running = scenario == 'busy-runtime-change'
            compacting = scenario == 'busy-runtime-compacting'
        if scenario == 'control-starts-work' and (root / 'native-idle').exists():
            running = False
        if scenario == 'completions-mapped-off' and model['id'] == 'model':
            model['thinkingLevelMap'] = {} if (root / 'disable-off').exists() else {'off': 'none'}
        if scenario in ['native-streaming', 'native-compacting'] and state_requests > 0:
            running = scenario == 'native-streaming' and not (root / 'native-idle').exists()
            compacting = scenario == 'native-compacting' and not (root / 'native-idle').exists()
        if '--no-session' in args:
            if scenario == 'discovery-state-slow':
                time.sleep(0.4)
            if scenario == 'discovery-state-late':
                time.sleep(1.2)
            if scenario == 'discovery-second-model':
                model = available_models[1]
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
        if (scenario == 'stale-thinking-event' and state_requests == 2) or (scenario == 'stale-thinking-open' and state_requests == 1):
            emit({'type': 'thinking_level_changed', 'level': 'high'})
            thinking = 'low'
        if control_conflict and (root / 'control-readback-conflict').exists():
            emit({'type': 'thinking_level_changed', 'level': 'off'})
        if scenario == 'single-thinking-event' and state_requests >= 2:
            thinking = 'high' if (root / 'settle-thinking').exists() else 'medium'
            if state_requests == 2:
                emit({'type': 'thinking_level_changed', 'level': 'high'})
        if ((scenario == 'stale-thinking-event' and state_requests == 3)
                or (scenario == 'stale-thinking-open' and state_requests == 2)
                or (scenario == 'stale-thinking-admission' and state_requests == 4)):
            emit({'type': 'thinking_level_changed', 'level': thinking})
        if (rejection_readback == 'conflict'
                or scenario == 'unstable-thinking-open'
                or (scenario == 'unstable-thinking-events' and state_requests >= 2)
                or (scenario == 'unstable-thinking-handled' and state_requests >= 3)):
            emit({'type': 'thinking_level_changed', 'level': 'high'})
            thinking = 'low'
        if scenario == 'notified-optional-thinking-state':
            emit({'type': 'thinking_level_changed', 'level': 'high'})
        if scenario == 'model-metadata-change':
            model['name'] = f'Fixture Model {state_requests}'
            model['input'] = ['text'] if state_requests % 2 == 0 else ['text', 'image']
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
        if scenario == 'pending-messages':
            state['pendingMessageCount'] = 1
        if scenario == 'newer-thinking-event' and state_requests == 2:
            thinking = 'low'
            emit({'type': 'thinking_level_changed', 'level': thinking})
        if scenario == 'stale-thinking-admission' and state_requests == 3:
            emit({'type': 'thinking_level_changed', 'level': 'high'})
        if scenario == 'missing-thinking-state' or (scenario == 'thinking-state-disappears' and state_requests > 1):
            del state['thinkingLevel']
        if scenario == 'notified-optional-thinking-state':
            del state['thinkingLevel']
            state.update(json.loads((root / 'thinking-state.json').read_text()))
        if (root / 'state-overrides.json').exists():
            state.update(json.loads((root / 'state-overrides.json').read_text()))
        if scenario.startswith('idle-thinking-') and state_requests == 1:
            thinking = 'future' if scenario == 'idle-thinking-future' else 'low'
            event = {'type': 'thinking_level_changed', 'level': thinking}
            if scenario == 'idle-thinking-missing':
                del event['level']
            elif scenario == 'idle-thinking-malformed':
                event['level'] = 42
        response(command, state)
        if scenario == 'idle-notices' and state_requests == 1:
            for _ in range(2000):
                emit({'type': 'extension_ui_request', 'method': 'notify', 'message': 'x' * 1024})
            (root / 'notices-written').touch()
        if scenario.startswith('idle-thinking-') and state_requests == 1:
            emit(event)
        if compacting and scenario != 'compaction-hang':
            compacting = False
            emit({'type': 'compaction_end', 'reason': 'manual', 'result': {}})
        if finish_after_state:
            finish_after_state = False
            running = False
            emit({'type': 'message_start', 'message': {'role': 'assistant'}})
            emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type': 'text', 'text': 'Extension'}], 'stopReason': 'stop'}})
            emit({'type': 'agent_settled', 'aborted': False})
    elif kind == 'get_commands':
        if scenario == 'control-notices':
            for _ in range(300):
                emit({'type': 'extension_ui_request', 'method': 'notify', 'message': 'notice'})
        (root / 'commands-started').touch()
        while (root / 'commands-blocked').exists():
            time.sleep(0.01)
        if scenario == 'commands-rejected' or (root / 'commands-rejected').exists():
            response(command, success=False)
            continue
        commands = [{'name': 'review', 'source': 'extension'},
                    {'name': 'fix-tests', 'description': 'Fix tests', 'source': 'prompt'},
                    {'name': 'skill:rust', 'description': 'Rust guidance', 'source': 'skill'}]
        response(command, {'commands': None if scenario == 'commands-malformed' else commands})
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
            emit({'type': 'thinking_level_changed', 'level': thinking})
        reply = ({key: model[key] for key in ['provider', 'id']}
                 if scenario.startswith('summary-model-reply') else model)
        response(command, reply, selected is not None)
    elif kind == 'set_thinking_level':
        if scenario == 'worded-thinking-rejection' and command['level'] == 'high':
            message = (root / 'rejection-message').read_text()
            response(command, success=False, error=message)
            continue
        if scenario == 'reject-implicit-thinking' and command['level'] == 'medium':
            if (root / 'disconnect-thinking').exists():
                sys.exit(2)
            response(command, success=False, error='forbidden fallback thinking level')
            continue
        if (scenario == 'summary-model-reply-reject-thinking' and model['id'] == 'other'
                and not (root / 'accept-thinking').exists()):
            response(command, success=False)
            continue
        if (scenario == 'reject-thinking'
                or (scenario == 'reject-live-thinking' and command['level'] == 'future')
                or (scenario == 'completions-reject-off' and command['level'] == 'off')):
            thinking_rejected = True
            response(command, success=False)
            continue
        if scenario == 'strict-thinking-models':
            allowed = ['off'] if not model['reasoning'] else [
                level for level in ['off', 'minimal', 'low', 'medium', 'high']
                if model.get('thinkingLevelMap', {}).get(level, True) is not None]
            if command['level'] not in allowed:
                response(command, success=False)
                continue
        control_conflict = (root / 'control-readback-conflict').exists()
        thinking = 'high' if scenario == 'clamp-thinking' else ('low' if model['id'] == 'clamped' else command['level'])
        if scenario == 'control-starts-work':
            running = True
        emit({'type': 'thinking_level_changed', 'level': thinking})
        response(command)
    elif kind == 'prompt':
        text = command['message']
        if text.startswith('[BTW context snapshot v1 - frozen]'):
            if 'SIDE-DIE' in text and 'SIDE-DIE-AFTER' not in text:
                sys.exit(2)
            if 'SIDE-NEVER-ADMIT' in text:
                continue
            if 'SIDE-HANDLED' in text:
                response(command, {'disposition': 'handled'})
                continue
            running = 'SIDE-BLOCK' in text
            response(command, {'disposition': 'started'})
            if 'SIDE-DIE-AFTER' in text:
                sys.exit(2)
            if running:
                continue
            emit({'type': 'message_start', 'message': {'role': 'assistant'}})
            answer = f'SIDE-ANSWER::{text}'
            emit({'type': 'message_update',
                  'assistantMessageEvent': {'type': 'text_delta', 'contentIndex': 0, 'delta': answer}})
            if 'SIDE-ERROR' in text:
                emit({'type': 'message_end', 'message': {'role': 'assistant',
                     'content': [{'type': 'text', 'text': answer}], 'stopReason': 'error',
                     'errorMessage': 'quota exhausted: token=secret-value'}})
            else:
                emit({'type': 'message_end', 'message': {'role': 'assistant',
                     'content': [{'type': 'text', 'text': answer}], 'stopReason': 'stop'}})
            emit({'type': 'agent_settled', 'aborted': False})
            continue
        if text in ['thinking-before', 'thinking-after', 'thinking-handled', 'thinking-invalid']:
            if text == 'thinking-after':
                response(command, {'disposition': 'started'})
            level = {'thinking-event-future': 'future', 'thinking-event-none': 'none',
                     'thinking-event-ultra': 'ultra', 'thinking-event-null': None,
                     'thinking-event-malformed': 42}.get(
                         scenario, bounded_levels.get(scenario.removeprefix('thinking-event-'), 'low'))
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
        if text == 'unknown-disposition':
            response(command, {'disposition': 'future'})
            continue
        if text in ['reject-running', 'reject-settled']:
            save_history(session_header)
            running = text == 'reject-running'
            emit({'type': 'agent_start'})
            emit({'type': 'tool_execution_start', 'toolCallId': 'extension',
                  'toolName': 'bash', 'args': {'command': 'echo extension'}})
            if not running:
                emit({'type': 'tool_execution_end', 'toolCallId': 'extension',
                      'toolName': 'bash', 'result': {}, 'isError': False})
                emit({'type': 'agent_settled', 'aborted': False})
            response(command, success=False)
            continue
        if text == 'exit':
            sys.exit(2)
        if text == 'malformed':
            print('not json', flush=True)
            continue
        if text in ['gated-admission', 'never-ack']:
            (root / 'prompt-received').touch()
        if text == 'gated-admission':
            while not (root / 'admit-prompt').exists():
                time.sleep(0.01)
        if text == 'never-ack':
            continue
        if text in ['unacknowledged-running', 'unacknowledged-settled']:
            save_history(session_header + json.dumps(
                {'type': 'thinking_level_change', 'thinkingLevel': 'low'}) + '\n')
            emit({'type': 'agent_start'})
            emit({'type': 'tool_execution_start', 'toolCallId': 'pending',
                  'toolName': 'bash', 'args': {'command': 'echo saved'}})
            if text == 'unacknowledged-settled':
                emit({'type': 'tool_execution_end', 'toolCallId': 'pending',
                      'toolName': 'bash', 'result': {}, 'isError': False})
                emit({'type': 'agent_settled', 'aborted': False})
            # The host reader queues preceding run events before writing this response.
            emit({'type': 'extension_ui_request', 'id': 'admission-barrier',
                  'method': 'confirm', 'title': 'Admission barrier'})
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
        if text == 'handled-control-notice':
            (root / 'handled-notice').touch()
            response(command, {'disposition': 'handled'})
            continue
        if text == 'ui-notification-error':
            emit({'type': 'extension_ui_request', 'id': 'notice', 'method': 'notify',
                  'notifyType': 'error', 'message': '/btw requires interactive TUI mode'})
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
            save_history(session_header)
            running = True
            finish_after_state = True
            emit({'type': 'agent_start'})
            response(command, {'disposition': 'handled'})
            continue
        if text in ['handled-before', 'handled-after']:
            save_history(session_header)
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
        save_history(session_header)
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
             'stopReason': 'error' if text == 'fail' else 'stop', 'errorMessage': 'quota exhausted: token=secret-value' if text == 'fail' else '', 'usage': {'input': 12, 'output': 3, 'cacheRead': 2, 'cost': {'total': 0}}}})
        emit({'type': 'agent_end', 'messages': []})
        emit({'type': 'agent_settled', 'aborted': False})
    elif kind == 'abort':
        if scenario == 'control-notices':
            for _ in range(300):
                emit({'type': 'extension_ui_request', 'method': 'notify', 'message': 'notice'})
        if running:
            running = False
            emit({'type': 'tool_execution_end', 'toolCallId': 'pending', 'toolName': 'bash', 'result': {}, 'isError': True})
        emit({'type': 'agent_end', 'messages': []})
        emit({'type': 'agent_settled', 'aborted': True})
        response(command)
    elif kind == 'extension_ui_response':
        assert command['cancelled'] is True
        if command['id'] == 'admission-barrier':
            (root / 'prompt-received').touch()
            continue
        emit({'type': 'agent_settled', 'aborted': False})
    else:
        response(command, success=False)
