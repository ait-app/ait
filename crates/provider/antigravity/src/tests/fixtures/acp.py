#!/usr/bin/env python3
"""Synthetic ACP errors and races; native successful traffic lives in recorded fixtures."""
import json
import os
from pathlib import Path
import sys

args = sys.argv[1:]
scenario = os.environ.get('AGY_ACP_SCENARIO', '')
if args == ['--version']:
    print(os.environ.get('AGY_ACP_VERSION', '1.5.18'))
    sys.exit(0)
if args == ['acp', '--help']:
    if scenario == 'no-acp':
        print('unknown command acp', file=sys.stderr)
        sys.exit(1)
    print('Usage: agy acp')
    sys.exit(0)
if not args or args[0] != 'acp':
    os.execv(str(Path(__file__).with_name('agy.py')), ['agy.py', *args])

session = '11111111-2222-4333-8444-555555555555'
prompt_id = None
turns = 0
options = [
    {'id': 'model', 'category': 'model', 'type': 'select', 'currentValue': 'gemini-test',
     'options': [{'value': value, 'name': value} for value in ['gemini-test', 'claude-test', 'plain-test']]},
    {'id': 'thought_level', 'category': 'thought_level', 'type': 'select', 'currentValue': 'high',
     'options': [{'value': value, 'name': value} for value in ['low', 'high', 'xhigh']]},
    {'id': 'mode', 'category': 'mode', 'type': 'select', 'currentValue': 'default',
     'options': [{'value': value, 'name': value} for value in ['default', 'accept-edits', 'auto', 'plan']]},
]
if scenario == 'missing-option':
    options.pop()

def emit(frame):
    print(json.dumps({'jsonrpc': '2.0', **frame}), flush=True)

def log(frame):
    path = os.environ.get('AGY_FIXTURE_LOG')
    if path:
        with open(path, 'a') as out:
            out.write(json.dumps(frame) + '\n')

def reply(request, result):
    emit({'id': request['id'], 'result': result})

def update(value):
    emit({'method': 'session/update', 'params': {'sessionId': session, 'update': value}})

def text(value):
    update({'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': value}})

def done(reason='end_turn'):
    emit({'id': prompt_id, 'result': {'stopReason': reason, 'usage': {
        'inputTokens': 10, 'outputTokens': 3, 'cachedReadTokens': 5, 'cachedWriteTokens': 0,
        'thoughtTokens': 1, 'totalTokens': 18}}})

log({'args': args, 'hasEnvironment': os.environ.get('AGY_TEST_ENV') == 'test-only-value',
     'cwd': os.getcwd(), 'pid': os.getpid()})
for line in sys.stdin:
    request = json.loads(line)
    log({'rpc': request})
    method = request.get('method')
    params = request.get('params', {})
    if method == 'initialize':
        if scenario in ['method-missing', 'auth', 'quota', 'network', 'unknown-error']:
            errors = {'method-missing': (-32601, 'Method not found'), 'auth': (-32000, 'authentication required'),
                      'quota': (-32000, 'quota exhausted'), 'network': (-32000, 'network error'),
                      'unknown-error': (-32001, 'unrecognized fixture-secret')}
            code, message = errors[scenario]
            emit({'id': request['id'], 'error': {'code': code, 'message': message}})
        elif scenario == 'initialize-timeout':
            continue
        else:
            reply(request, {'protocolVersion': 1,
                'agentInfo': {'name': 'Antigravity ACP Server', 'version': os.environ.get('AGY_ACP_VERSION', '1.5.18')},
                'agentCapabilities': {
                'promptCapabilities': {'image': scenario != 'missing-capability'},
                'sessionCapabilities': ({'close': {}} if scenario == 'missing-resume' else {'resume': {}, 'close': {}})}})
    elif method in ['session/new', 'session/resume']:
        session = params.get('sessionId', session)
        reply(request, {'sessionId': 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee' if scenario == 'wrong-session' else session,
                        'configOptions': options})
    elif method == 'session/set_config_option':
        for option in options:
            if option['id'] == params['configId'] and scenario != 'unacknowledged-config':
                option['currentValue'] = params['value']
        if scenario == 'switch-unsupported' and params['value'] == 'plain-test':
            options = [option for option in options if option['category'] != 'mode']
        if scenario in ['drift-model', 'drift-effort'] and params['configId'] == 'mode':
            target, value = ('model', 'claude-test') if scenario == 'drift-model' else ('thought_level', 'high')
            next(option for option in options if option['id'] == target)['currentValue'] = value
        reply(request, {'configOptions': options})
    elif method == 'session/prompt':
        prompt_id = request['id']
        turns += 1
        content = ''.join(x.get('text', '') for x in params['prompt'])
        if content == 'sandbox-boundary':
            home = Path(os.environ['HOME'])
            cli = home / '.gemini' / 'antigravity-cli'
            (cli / 'conversations').mkdir(parents=True, exist_ok=True)
            (cli / 'conversations' / 'acp-state.txt').write_text('ok')
            Path('acp-worktree-ok.txt').write_text('ok')
            for blocked in [home / 'forbidden.txt', home / '.gemini/config/hooks.json',
                            cli / 'settings.json', cli / 'hooks.json',
                            cli / 'builtin/injected.md', cli / 'plugins/injected.json']:
                try:
                    blocked.write_text('tampered')
                except OSError:
                    continue
                emit({'id': prompt_id, 'error': {'code': -32000, 'message': 'write confinement was not enforced'}})
                sys.exit(20)
        if content == 'malformed':
            print('{', flush=True)
            sys.exit(0)
        if content == 'oversized':
            print('x' * (8 * 1024 * 1024 + 1), flush=True)
            sys.exit(0)
        if content == 'stale':
            emit({'id': 'old-prompt', 'result': {'stopReason': 'end_turn'}})
            continue
        if content == 'foreign':
            emit({'method': 'session/update', 'params': {'sessionId': 'foreign', 'update': {}}})
            continue
        if scenario == 'large-input':
            text('accepted')
        elif content != 'deny-empty':
            reply_text = (
                'partial'
                if content in ['disconnect', 'wait', 'race', 'deny', 'no-cancel']
                else (content or 'image received')
            )
            text(reply_text)
        update({'sessionUpdate': 'usage_update', 'used': 42, 'size': 1000})
        if content in ['disconnect', 'wait', 'race', 'deny', 'deny-empty', 'no-cancel', 'tool', 'unfinished']:
            update({'sessionUpdate': 'tool_call', 'toolCallId': 'file', 'name': 'write_file',
                    'title': 'File result.txt', 'kind': 'edit', 'status': 'in_progress',
                    'rawInput': {'Path': '/tmp/result.txt', 'Content': 'test'}})
        if content == 'disconnect':
            sys.exit(1)
        if content == 'crash-once':
            sys.exit(1)
        if content in ['wait', 'race', 'no-cancel']:
            scenario = content
            continue
        if content in ['deny', 'deny-empty']:
            scenario = content
            emit({'id': 'permission', 'method': 'session/request_permission', 'params': {
                'sessionId': session, 'toolCall': {'toolCallId': 'file', 'title': 'write_file', 'kind': 'edit'},
                'options': [{'optionId': 'yes', 'name': 'Allow once', 'kind': 'allow_once'},
                            {'optionId': 'no', 'name': 'Reject once', 'kind': 'reject_once'}]}})
            continue
        if content == 'tool':
            update({'sessionUpdate': 'tool_call_update', 'toolCallId': 'file', 'status': 'completed'})
        if content == 'usage-invalid':
            emit({'id': prompt_id, 'result': {'stopReason': 'end_turn', 'usage': {'inputTokens': -1}}})
        else:
            done()
    elif method == 'session/cancel':
        if scenario != 'no-cancel':
            if scenario == 'race':
                update({'sessionUpdate': 'tool_call_update', 'toolCallId': 'file', 'status': 'completed'})
            done('end_turn' if scenario == 'race' else 'cancelled')
    elif method == 'session/close':
        reply(request, {})
    elif request.get('id') == 'permission':
        assert request['result']['outcome'] == {'outcome': 'selected', 'optionId': 'no'}
        update({'sessionUpdate': 'tool_call_update', 'toolCallId': 'file', 'status': 'failed'})
        if scenario != 'deny-empty':
            text('permission denied')
        done()
    else:
        emit({'id': request['id'], 'error': {'code': -32601, 'message': 'Method not found'}})
