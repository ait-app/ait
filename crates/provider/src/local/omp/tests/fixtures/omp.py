#!/usr/bin/env python3
"""Offline OMP RPC peer; all state stays under an isolated agent directory."""
import json
import os
from pathlib import Path
import sys
import time
import uuid

args = sys.argv[1:]
cwd = os.getcwd()
root = Path(os.environ['PI_CODING_AGENT_DIR'])
root.mkdir(parents=True, exist_ok=True)
log = root / 'calls.jsonl'
with log.open('a') as output:
    output.write(json.dumps({'args': args}) + '\n')
file = Path(args[args.index('--resume') + 1]) if '--resume' in args else None
records = []
if file and file.exists():
    records = [json.loads(line) for line in file.read_text().splitlines() if json.loads(line)['type'] != 'title']
    session_id = next(record['id'] for record in records if record['type'] == 'session')
else:
    session_id = str(uuid.uuid4())
    file = root / 'sessions' / 'workspace' / ('2026-10-21_' + session_id + '.jsonl')
    records = [{'type': 'session', 'version': 3, 'id': session_id, 'cwd': cwd,
                'timestamp': '2026-10-21T00:00:00Z'}]
clock = 1792540800000 + len(records) * 10
leaf = records[-1].get('id') if len(records) > 1 else None
models = [
    {'provider': 'test', 'id': 'model', 'name': 'Fixture model', 'reasoning': True,
     'thinking': {'mode': 'effort', 'efforts': ['low', 'high']}, 'contextWindow': 32768},
    {'provider': 'other', 'id': 'model', 'name': 'Other', 'reasoning': True,
     'thinking': {'mode': 'effort', 'efforts': ['low', 'high', 'max']}, 'contextWindow': 32768},
]
model = models[0]
if '--provider' in args and '--model' in args:
    provider = args[args.index('--provider') + 1]
    model_id = args[args.index('--model') + 1]
    model = next((item for item in models if item['provider'] == provider and item['id'] == model_id), model)
level = 'off'
active = False
prompt_id = None
pending_plugin = None
pending_control = None
pending_async = False
current_prompt = None
pending_dialog = None
dialog_commands = []
held_control = None

def children():
    path = root / 'subagents.json'
    return json.loads(path.read_text()) if path.exists() else []

def child(subagent_id):
    return next((item for item in children() if item['id'] == subagent_id), None)

def child_controllable(item):
    return (item is not None and item.get('status') in ('running', 'pending')
            and bool(item.get('sessionFile')))

def emit(value):
    data = json.dumps(value, ensure_ascii=False).encode()
    if len(data) < 900000:
        print(data.decode(), flush=True)
    else:
        import base64
        chunks = [data[index:index+524288] for index in range(0, len(data), 524288)]
        chunk_id = str(uuid.uuid4())
        for index, chunk in enumerate(chunks):
            print(json.dumps({'type':'rpc_chunk','chunkId':chunk_id,'index':index,'count':len(chunks),'byteLength':len(data),'data':base64.b64encode(chunk).decode()}), flush=True)

def response(command, data=None, success=True, error=None):
    emit({'type': 'response', 'id': command.get('id'), 'command': command['type'],
          'success': success, 'data': data,
          'error': None if success else (error or 'Rejected')})

def message(role, content, **fields):
    global clock, leaf
    clock += 10
    value = {'role': role, 'content': content, 'timestamp': clock, **fields}
    emit({'type': 'message_start', 'message': value})
    if role == 'assistant':
        for index, block in enumerate(content):
            if block['type'] in ('text', 'thinking'):
                field = 'text' if block['type'] == 'text' else 'thinking'
                emit({'type': 'message_update', 'message': value, 'assistantMessageEvent': {
                    'type': field + '_delta', 'contentIndex': index, 'delta': block[field]}})
    emit({'type': 'message_end', 'message': value})
    entry_id = str(uuid.uuid4())
    records.append({'type': 'message', 'id': entry_id, 'parentId': leaf,
                    'timestamp': '2026-10-21T00:00:01Z', 'message': value})
    leaf = entry_id
    if '--no-session' not in args:
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(''.join(json.dumps(entry) + '\n' for entry in records))

def finish(aborted=False, settled=True):
    global active
    message('assistant', [{'type': 'text', 'text': 'Stopped' if aborted else 'Hello world'}],
            provider='test', model='model', stopReason='aborted' if aborted else 'stop',
            usage={'input': 10, 'output': 5, 'cacheRead': 2, 'cacheWrite': 0, 'cost': {'total': 0.001}})
    emit({'type': 'agent_end', 'messages': []})
    active = False
    emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True, 'status': 'aborted' if aborted else 'completed', 'sessionSettled': settled})
    if not settled:
        emit({'type': 'agent_start'})
        message('assistant', [{'type': 'text', 'text': 'Background finished'}],
                provider='test', model='model', stopReason='stop')
        emit({'type': 'agent_end', 'messages': []})
    emit({'type': 'session_settled'})

emit({'type':'ready','protocolVersion':1,'supportedProtocolVersions':[1,2],'maxFrameBytes':1048576,'maxReassembledFrameBytes':67108864})

startup_pending = (root/'startup-dialog').exists() and '--no-ui' not in args
if startup_pending:
    emit({'type':'extension_ui_request','id':'startup-choice','method':'input','title':'Startup input'})

def commands_from_input():
    global startup_pending
    held = []
    for line in sys.stdin:
        command = json.loads(line)
        if startup_pending:
            if command.get('type') == 'extension_ui_response' and command.get('id') == 'startup-choice':
                with log.open('a') as output:
                    output.write(json.dumps(command) + '\n')
                startup_pending = False
                yield from held
                held.clear()
            else:
                held.append(command)
            continue
        yield command

for command in commands_from_input():
    with log.open('a') as output:
        output.write(json.dumps(command) + '\n')
    kind = command['type']
    if pending_dialog and kind in ('get_state', 'steer'):
        dialog_commands.append(command)
        continue
    if kind == 'negotiate_protocol':
        response(command, {'protocolVersion':2})
    elif kind == 'get_state':
        if pending_async and (root / 'release-settlement').exists():
            pending_async = False
            emit({'type': 'agent_start'})
            message('assistant', [{'type': 'text', 'text': 'Background finished'}],
                    provider='test', model='model', stopReason='stop')
            emit({'type': 'agent_end', 'messages': [], 'isTerminal': True, 'yielded': True})
        state = {'sessionId': session_id, 'sessionFile': str(file), 'model': model,
                          'thinkingLevel': level, 'isStreaming': active, 'isCompacting': False,
                          'isSettled': not active and not pending_async,
                          'hasPendingAsyncWork': pending_async, 'queuedMessageCount': 0,
                          'slowModeSupported': model['provider'] == 'test'}
        if current_prompt == 'missing-settlement-fact':
            state.pop('isSettled')
        response(command, state)
    elif kind == 'fixture_new_context':
        session_id=str(uuid.uuid4())
        file=root/'sessions'/'workspace'/('2026-10-25_'+session_id+'.jsonl')
        records=[{'type':'session','version':3,'id':session_id,'cwd':cwd,'timestamp':'2026-10-25T00:00:00Z'}]
        response(command, {'sessionId':session_id})
    elif kind == 'get_available_models':
        response(command, {'models': models})
    elif kind == 'get_available_thinking_levels':
        response(command, {'levels': ['off', *model['thinking']['efforts']]})
    elif kind == 'set_model':
        selected = next((item for item in models if item['provider'] == command['provider'] and item['id'] == command['modelId']), None)
        if selected:
            model = selected
            response(command, model)
        else:
            response(command, success=False)
    elif kind == 'set_thinking_level':
        level = command['level']
        response(command)
    elif kind in ('set_fast_mode', 'set_slow_mode'):
        response(command, success=not command['enabled'] or model['provider'] == 'test')
    elif kind in ('set_auto_compaction', 'set_auto_retry', 'set_ask_dialog', 'set_subagent_subscription'):
        response(command)
    elif kind == 'get_subagents':
        response(command, {'subagents': children()})
    elif kind == 'steer_subagent':
        item = child(command.get('subagentId'))
        if not child_controllable(item):
            response(command, success=False,
                     error='Subagent not running: ' + str(command.get('subagentId')))
        elif command.get('message', '').strip() == 'refuse':
            response(command, success=False, error='Subagent refused the message: fixture')
        else:
            response(command)
    elif kind == 'cancel_subagent' and (root / 'hold-control-answer').exists():
        # A native answer withheld until the host resolves this question.
        held_control = command
        emit({'type': 'extension_ui_request', 'id': 'held-question', 'method': 'select',
              'title': 'Held question', 'options': ['A']})
    elif kind == 'cancel_subagent':
        item = child(command.get('subagentId'))
        if child_controllable(item) and (root / 'race-control').exists():
            # The child stops between the membership read and this write: publish a question that
            # must still reach the host, then report the native not-running fact.
            emit({'type': 'extension_ui_request', 'id': 'race-question', 'method': 'select',
                  'title': 'Race question', 'options': ['A', 'B']})
            response(command, {'cancelled': False})
            continue
        cancelled = child_controllable(item)
        if cancelled:
            # A cancelled child leaves the native running membership and reports a terminal
            # lifecycle status, as the native runtime does.
            items = children()
            for entry in items:
                if entry['id'] == item['id']:
                    entry['status'] = 'aborted'
            (root / 'subagents.json').write_text(json.dumps(items))
            emit({'type': 'subagent_lifecycle', 'payload': {
                'id': item['id'], 'index': item.get('index', 0), 'agent': item.get('agent'),
                'status': 'aborted', 'parentToolCallId': item.get('parentToolCallId')}})
        response(command, {'cancelled': cancelled})
    elif kind == 'get_available_commands':
        response(command, {'commands': [{'name': 'review', 'description': 'Review', 'source': 'skill'}, *json.loads((root/'plugins.json').read_text())] if (root/'plugins.json').exists() else [{'name': 'review', 'description': 'Review', 'source': 'skill'}]})
    elif kind == 'interactive_control':
        pending_control=command
        emit({'type':'extension_ui_request','id':'control-question','method':'input','title':'Native control input'})
    elif kind == 'host_tool_update':
        pass
    elif kind == 'host_tool_result':
        finish()
    elif kind == 'prompt':
        text = command['message']
        current_prompt = text
        prompt_id = command.get('id')
        if text == 'interactive-fixture':
            pending_dialog = command
            dialog = json.loads((root / 'dialog.json').read_text())
            emit({'type':'extension_ui_request','id':'dialog-1',**dialog})
            continue
        plugins = json.loads((root/'plugins.json').read_text()) if (root/'plugins.json').exists() else []
        plugin = next((entry for entry in plugins if text == '/'+entry['name']), None)
        if plugin:
            pending_plugin = (command, plugin)
            emit({'type':'extension_ui_request','id':'plugin-question','method':'select',
                  'title':plugin['description'],'options':['A, B','C']})
            continue
        if text == 'reject':
            response(command, success=False)
            continue
        if text == '/notify-only':
            emit({'type':'extension_ui_request','method':'notify','message':'Notify only output','notifyType':'info'})
            response(command)
            emit({'type':'prompt_result','id':prompt_id,'agentInvoked':False,'status':'completed','sessionSettled':True})
            continue
        if text == '/local':
            response(command, {'agentInvoked': False})
            continue
        response(command, {'agentInvoked': True})
        active = True
        emit({'type': 'agent_start'})
        message('user', [{'type': 'text', 'text': text}, *command.get('images', [])])
        if text == 'subagent-controls':
            # One running child of this parent plus a terminal sibling, published through the
            # native lifecycle frames this process emits for its own children.
            (root / 'subagents.json').write_text(json.dumps([
                {'id': 'worker', 'index': 0, 'agent': 'Task', 'status': 'running',
                 'sessionFile': str(root / 'sessions' / 'worker.jsonl'),
                 'parentToolCallId': 'task-1'},
                {'id': 'done', 'index': 1, 'agent': 'Task', 'status': 'completed',
                 'sessionFile': str(root / 'sessions' / 'done.jsonl'),
                 'parentToolCallId': 'task-2'},
            ]))
            emit({'type': 'subagent_lifecycle', 'payload': {
                'id': 'worker', 'index': 0, 'agent': 'Task', 'status': 'started',
                'parentToolCallId': 'task-1'}})
            emit({'type': 'subagent_lifecycle', 'payload': {
                'id': 'done', 'index': 1, 'agent': 'Task', 'status': 'started',
                'parentToolCallId': 'task-2'}})
            emit({'type': 'subagent_lifecycle', 'payload': {
                'id': 'done', 'index': 1, 'agent': 'Task', 'status': 'completed',
                'parentToolCallId': 'task-2'}})
            emit({'type': 'agent_end', 'messages': [], 'isTerminal': True, 'yielded': True})
            active = False
            emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True,
                  'status': 'completed', 'sessionSettled': True})
            continue
        if text == 'native-host':
            emit({'type':'future_native_event','opaque':{'unmodified':[1,2,3]}})
            emit({'type':'host_tool_call','id':'host-task','toolCallId':'external-tool','toolName':'unknown-vendor-tool','arguments':{'value':42}})
            continue
        if text == 'frame-error':
            emit({'type': 'rpc_frame_error', 'error': {
                'message': 'Upstream shard unavailable api_key=fixture-secret', 'statusCode': 503}})
            continue
        if text == 'unknown-native-error':
            error = 'Upstream shard unavailable api_key=fixture-secret'
            message('assistant', [], stopReason='error', errorMessage=error)
            emit({'type': 'prompt_result', 'id': prompt_id, 'status': 'error',
                  'sessionSettled': True, 'error': error})
            continue
        if text == 'invalid-timestamp':
            emit({'type': 'message_start', 'message': {'role': 'assistant', 'content': []}})
            continue
        if text == 'unknown-tool-result':
            message('toolResult', [{'type': 'text', 'text': 'api_key=fixture-secret'}],
                    toolCallId='unknown', toolName='bash', isError=True)
            continue
        if text == 'invalid-usage':
            message('assistant', [], usage={'input': -1, 'output': 0})
            continue
        if text == 'closed':
            break
        if text == 'incomplete-frame':
            sys.stdout.write('{"type":')
            sys.stdout.flush()
            break
        if text == 'invalid-chunks':
            emit({'type': 'rpc_chunk', 'chunkId': 'bad', 'index': 1,
                  'count': 2, 'byteLength': 4, 'data': 'e30='})
            break
        if text == 'missing-frame-type':
            emit({'content': 'api_key=fixture-secret'})
            break
        if text in ('native-error', 'native-error-recovered'):
            message('assistant', [], provider='test', model='model', stopReason='error',
                    errorMessage='Payment required: insufficient balance api_key=fixture-secret')
            if text == 'native-error':
                active = False
                emit({'type':'prompt_result','id':prompt_id,'status':'error','sessionSettled':True,
                      'error':'Payment required: insufficient balance api_key=fixture-secret'})
                continue
        if text == 'malformed':
            print('{broken', flush=True)
            break
        if text in ('lost-settlement', 'lost-result', 'busy-after-yield', 'missing-settlement-fact'):
            message('assistant', [{'type': 'text', 'text': 'Hello world'}],
                    provider='test', model='model', stopReason='stop')
            emit({'type': 'agent_end', 'messages': [], 'isTerminal': True, 'yielded': True})
            active = False
            pending_async = text == 'busy-after-yield'
            if text != 'lost-result':
                emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True,
                      'status': 'completed', 'sessionSettled': False})
            continue
        if text in ('wait', 'wait-slow-abort', 'wait-lost-abort', 'wait-stuck-abort'):
            emit({'type': 'agent_end', 'messages': []})
            emit({'type': 'auto_retry_start', 'attempt': 1})
            continue
        if text == 'ask':
            emit({'type':'extension_ui_request','id':'ask-1','method':'ask','questions':[{'id':'db','question':'Which database?','options':[{'label':'A,B'},{'label':'C'}]},{'id':'features','question':'Which features?','multi':True,'options':[{'label':'Auth'},{'label':'Search'}]}]})
            continue
        if text == 'subagents':
            emit({'type':'subagent_lifecycle','payload':{'id':'worker','index':0,'agent':'Task','status':'started','parentToolCallId':'task-1'}})
            emit({'type':'subagent_event','payload':{'id':'worker','event':{'type':'message_end','message':{'role':'assistant','timestamp':1792540800300,'content':[{'type':'text','text':'Child output'}]}}}})
            emit({'type':'subagent_lifecycle','payload':{'id':'worker','index':0,'agent':'Task','status':'completed','parentToolCallId':'task-1'}})
        if text == 'chunked':
            emit({'type':'available_commands_update','commands':[{'name':'large','description':'x'*1200000}]})
        if text == 'question':
            emit({'type': 'extension_ui_request', 'id': 'ui-1', 'method': 'select',
                  'title': 'Choose', 'options': ['A', 'B']})
            continue
        message('assistant', [{'type': 'thinking', 'thinking': 'Plan'},
                              {'type': 'text', 'text': 'Inspect'},
                              {'type': 'toolCall', 'id': 'call-' + str(clock), 'name': 'read', 'arguments': {'path': 'file.rs'}}],
                provider='test', model='model', stopReason='toolUse',
                usage={'input': 10, 'output': 5, 'cacheRead': 2, 'cacheWrite': 0, 'cost': {'total': 0.001}})
        call = records[-1]['message']['content'][-1]['id']
        emit({'type': 'tool_execution_start', 'toolCallId': call, 'toolName': 'read', 'args': {'path': 'file.rs'}})
        message('toolResult', [{'type': 'text', 'text': 'contents'}], toolCallId=call, toolName='read', isError=False)
        if text in ('background-tool-error', 'background-busy'):
            if text == 'background-busy':
                message('assistant', [{'type': 'toolCall', 'id': 'foreground', 'name': 'bash',
                                       'arguments': {'command': 'offline fixture'}}], stopReason='toolUse')
                emit({'type': 'tool_execution_start', 'toolCallId': 'foreground',
                      'toolName': 'bash', 'args': {'command': 'offline fixture'}})
            else:
                emit({'type': 'agent_end', 'messages': []})
                emit({'type': 'prompt_result', 'id': prompt_id, 'status': 'completed',
                      'sessionSettled': False})
            emit({'type': 'tool_execution_update', 'toolCallId': call, 'toolName': 'read',
                  'args': {'path': 'file.rs'}, 'partialResult': {
                      'content': [{'type': 'text', 'text': 'curl: (28) Connection timed out'}],
                      'details': {'async': {'state': 'completed', 'jobId': 'bg_15'}}}})
            message('custom', 'Background job bg_15 completed: curl: (28) Connection timed out',
                    customType='async-result', display=True, attribution='agent')
            if text == 'background-busy':
                message('toolResult', [{'type': 'text', 'text': 'Foreground finished'}],
                        toolCallId='foreground', toolName='bash', isError=False)
            else:
                emit({'type': 'agent_start'})
        finish(settled=text != 'background')
    elif kind == 'steer':
        response(command, {'disposition': 'queued'})
        message('user', command['message'])
        finish()
    elif kind == 'abort_and_restore_queue':
        if current_prompt == 'wait-slow-abort':
            (root / 'abort-entered').touch()
            while not (root / 'release-abort').exists():
                time.sleep(0.01)
        if current_prompt == 'wait-stuck-abort':
            continue
        if current_prompt == 'wait-lost-abort':
            message('assistant', [{'type': 'text', 'text': 'Stopped'}],
                    provider='test', model='model', stopReason='aborted')
            active = False
            emit({'type': 'agent_end', 'messages': [], 'isTerminal': True, 'yielded': True})
            emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True,
                  'status': 'aborted', 'sessionSettled': False})
            continue
        finish(aborted=True)
        response(command)
    elif kind == 'extension_ui_response':
        if command.get('id') == 'race-question':
            continue
        if command.get('id') == 'held-question' and held_control:
            pending_control_answer = held_control
            held_control = None
            item = child(pending_control_answer.get('subagentId'))
            cancelled = child_controllable(item)
            if cancelled:
                items = children()
                for entry in items:
                    if entry['id'] == item['id']:
                        entry['status'] = 'aborted'
                (root / 'subagents.json').write_text(json.dumps(items))
                emit({'type': 'subagent_lifecycle', 'payload': {
                    'id': item['id'], 'index': item.get('index', 0), 'agent': item.get('agent'),
                    'status': 'aborted', 'parentToolCallId': item.get('parentToolCallId')}})
            response(pending_control_answer, {'cancelled': cancelled})
            continue
        if pending_dialog:
            response(pending_dialog, {'agentInvoked':True})
            pending_dialog = None
            active = True
            emit({'type':'agent_start'})
            message('user', [{'type':'text','text':'interactive-fixture'}])
            for queued in dialog_commands:
                if queued['type'] == 'steer':
                    response(queued, {'disposition':'queued'})
                    message('user', queued['message'])
                else:
                    response(queued, {'sessionId':session_id,'isSettled':False,'isStreaming':True})
            dialog_commands = []
            finish()
            continue
        if pending_control:
            response(pending_control,{'value':command.get('value')})
            pending_control=None
            continue
        if pending_plugin:
            pending_command, plugin = pending_plugin
            pending_plugin = None
            emit({'type':'extension_ui_request','id':'plugin-note','method':'notify',
                  'message':'Plugin selected: '+str(command.get('value')), 'opaque':{'vendor':plugin['name']}})
            message('custom', [{'type':'text','text':plugin['name']+': '+str(command.get('value'))}],
                    customType=plugin['name'],display=True)
            response(pending_command, {'agentInvoked':False})
            continue
        finish()
    else:
        response(command, success=False)
