#!/usr/bin/env python3
"""Offline OMP RPC peer; all state stays under an isolated agent directory."""
import json
import os
from pathlib import Path
import sys
import time
import uuid

args = sys.argv[1:]
btw_records = []
goal_state = None
cwd = os.getcwd()
root = Path(os.environ['PI_CODING_AGENT_DIR'])
root.mkdir(parents=True, exist_ok=True)
log = root / 'calls.jsonl'
roles_file = root / 'model-roles.json'
roles = json.loads(roles_file.read_text()) if roles_file.exists() else {
    'default': 'test/model', 'smol': 'other/model', 'advisor': 'other/model:high'}
if args[:1] in (['config'], ['models']):
    with (root / 'metadata-calls.jsonl').open('a') as output:
        output.write(json.dumps(args) + '\n')
if args[:2] == ['config', 'list']:
    def presets(scope):
        file = root / (scope + '-presets.json')
        return json.loads(file.read_text()) if file.exists() else {}
    saved = {**presets('global'), **presets('project')}
    if '--scope' in args:
        saved = presets(args[args.index('--scope') + 1])
    values = {'modelRoles': roles, 'modelTags': {}, 'cycleOrder': ['smol', 'default', 'slow'],
              'modelPresets': saved, 'defaultThinkingLevel': 'off'}
    for key, name in [('modelTags', 'model-tags.json'), ('cycleOrder', 'cycle-order.json')]:
        file = root / name
        if file.exists():
            values[key] = json.loads(file.read_text())
    print(json.dumps({key: {'value': value} for key, value in values.items()}))
    sys.exit(0)
if args[:1] == ['models']:
    print(json.dumps({'models': [
        {'selector': 'test/model', 'name': 'Fixture model', 'kind': 'chat', 'input': ['text', 'image'], 'thinking': ['low', 'high']},
        {'selector': 'other/model', 'name': 'Other', 'kind': 'chat', 'input': ['text'], 'thinking': ['low', 'high', 'max']},
        *(json.loads((root / 'extra-models.json').read_text()) if (root / 'extra-models.json').exists() else []),
        *[{'selector': 'test/' + kind, 'name': kind, 'kind': kind} for kind in ['tiny', 'image', 'search', 'tts', 'stt', 'judge']]
    ]}))
    sys.exit(0)
overlay = json.loads(Path(args[args.index('--config') + 1]).read_text()) if '--config' in args else {}
roles.update(overlay.get('modelRoles', {}))
with log.open('a') as output:
    output.write(json.dumps({'args': args, 'modelRoles': roles, 'defaultThinkingLevel': overlay.get('defaultThinkingLevel')}) + '\n')
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
    {'provider': 'other', 'id': 'model', 'name': 'Other', 'reasoning': True, 'webSearch': {},
     'thinking': {'mode': 'effort', 'efforts': ['low', 'high', 'max']}, 'contextWindow': 32768},
]
model = models[0]
default = roles.get('default')
for _ in range(128):
    if not isinstance(default, str) or not default.startswith('@'):
        break
    default = roles.get(default[1:])
default = default or 'test/model'
selector, separator, effort = default.rpartition(':')
if not separator:
    selector, effort = default, overlay.get('defaultThinkingLevel', 'off')
model = next((item for item in models if item['provider'] + '/' + item['id'] == selector), model)
if '--provider' in args and '--model' in args:
    provider = args[args.index('--provider') + 1]
    model_id = args[args.index('--model') + 1]
    model = next((item for item in models if item['provider'] == provider and item['id'] == model_id), model)
level = effort
active = False
prompt_id = None
pending_plugin = None
pending_control = None
pending_async = False
current_prompt = None
pending_dialog = None
dialog_commands = []

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

def response(command, data=None, success=True):
    emit({'type': 'response', 'id': command.get('id'), 'command': command['type'],
          'success': success, 'data': data, 'error': None if success else 'Rejected'})

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

if (root / 'ready-delay').exists():
    time.sleep(float((root / 'ready-delay').read_text()))

emit({'type':'ready','protocolVersion':1,'supportedProtocolVersions':[1,2],'maxFrameBytes':1048576,'maxReassembledFrameBytes':67108864,
      'capabilities': {'commandCompletion': {'version': 1 if (root / 'completion.json').exists() else 0}}})

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
    if kind == 'fixture_slow_control':
        time.sleep(0.8)
        response(command)
    elif kind == 'negotiate_protocol':
        if (root / 'negotiate-delay').exists():
            time.sleep(float((root / 'negotiate-delay').read_text()))
        response(command, {'protocolVersion':2})
    elif kind == 'complete':
        response(command, json.loads((root / 'completion.json').read_text()))
    elif kind == 'goal':
        op = command.get('op')
        if op == 'create' and goal_state is None:
            goal_state = {'enabled': True, 'goal': {'id': str(uuid.uuid4()), 'status': 'active', 'objective': command['objective'], 'tokensUsed': 0}}
        elif op in ('pause', 'resume') and goal_state:
            goal_state['enabled'] = op == 'resume'
            goal_state['goal']['status'] = 'active' if op == 'resume' else 'paused'
        elif op == 'drop':
            goal_state = None
        elif op != 'get':
            response(command, success=False)
            continue
        response(command, {'goal': goal_state['goal'] if goal_state else None, 'state': goal_state})
    elif kind == 'set_host_tools':
        response(command, {'toolNames': [tool['name'] for tool in command.get('tools', [])]})
    elif kind == 'get_state':
        if active and (root/'finish-wait').exists():
            (root/'finish-wait').unlink()
            finish()
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
                          'slowModeSupported': model['provider'] == 'test', 'goal': goal_state}
        if current_prompt == 'missing-settlement-fact':
            state.pop('isSettled')
        if (root / 'idle-busy').exists():
            state.update({'hasPendingAsyncWork': True, 'isSettled': False})
        if (root / 'idle-unknown').exists():
            state.pop('isSettled', None)
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
        response(command, {'subagents': []})
    elif kind == 'get_btw_history':
        saved = root/'btw-history-fixture.json'
        if saved.exists():
            btw_records = json.loads(saved.read_text())
        response(command, {'records':btw_records})
    elif kind == 'btw':
        previous = next((entry for entry in btw_records if entry['id'] == command.get('recordId')),None)
        turn = {'question':command['question'],'answer':'','status':'running','createdAt':int(time.time()*1000),'updatedAt':int(time.time()*1000)}
        if previous:
            previous.setdefault('followUps',[]).append(turn)
            record = previous
        else:
            record = {'id':str(uuid.uuid4()),**turn}
            btw_records.insert(0,record)
            turn = record
        emit({'type':'btw_record','record':record})
        response(command,{'record':record})
        if not command['question'].startswith('hold'):
            emit({'type':'btw_delta','recordId':record['id'],'delta':'Native side answer'})
            turn['answer'] = 'Native side answer'
            turn['status'] = 'complete'
            turn['updatedAt'] += 1
            emit({'type':'btw_record','record':record})
        (root/'btw-history-fixture.json').write_text(json.dumps(btw_records))
    elif kind == 'btw_cancel':
        cancelled = False
        for record in btw_records:
            turn = record.get('followUps',[record])[-1]
            if turn['status'] == 'running' and (not command.get('recordId') or command['recordId'] == record['id']):
                turn['status'] = 'cancelled'
                turn['updatedAt'] += 1
                emit({'type':'btw_record','record':record})
                cancelled = True
        (root/'btw-history-fixture.json').write_text(json.dumps(btw_records))
        response(command,{'cancelled':cancelled})
    elif kind == 'get_available_commands':
        changed = root/'project-presets-on-probe.json'
        if changed.exists():
            (root/'project-presets.json').write_text(changed.read_text())
            changed.unlink()
        commands = [{'name': 'review', 'description': 'Review', 'source': 'skill'}]
        if (root/'side-commands').exists():
            commands.extend([{'name':'usage','source':'builtin'},{'name':'status','source':'extension'}])
        if not (root/'missing-preset-builtin').exists():
            commands.append({'name':'modelpreset','description':'Model presets','source':'builtin'})
        if (root/'plugins.json').exists():
            commands.extend(json.loads((root/'plugins.json').read_text()))
        if '--extension' in args and not (root/'missing-preset-helper').exists():
            commands.append({'name':'ait-read-model-preset-default','description':'Preset reader','source':'extension'})
        response(command, {'commands':commands})
    elif kind in ('interactive_control', 'login'):
        pending_control=command
        if kind == 'login':
            emit({'type':'extension_ui_request','id':'login-url','method':'open_url','url':'https://example.com/fixture-login'})
        emit({'type':'extension_ui_request','id':'control-question','method':'input','title':'Native control input'})
    elif kind == 'host_tool_update':
        pass
    elif kind == 'host_tool_result':
        finish()
    elif kind == 'prompt':
        text = command['message']
        if text == '/usage' and (root/'side-commands').exists():
            emit({'type':'command_output','text':'Native usage: 123 tokens'})
            response(command,{'agentInvoked':False})
            continue
        current_prompt = text
        prompt_id = command.get('id')
        if text == '/ait-read-model-preset-default':
            response(command)
            resolved = next(([item['provider'],item['id']] for item in models if item['provider']+'/'+item['id'] == selector),None)
            emit({'type':'extension_ui_request','id':'preset-default','method':'notify','message':json.dumps({'aitPresetDefault':resolved})})
            emit({'type':'prompt_result','id':prompt_id,'agentInvoked':False,'status':'completed'})
            continue
        if text.startswith('/modelpreset ') and not any(entry.get('name') == 'modelpreset' and entry.get('source') != 'builtin' for entry in (json.loads((root/'plugins.json').read_text()) if (root/'plugins.json').exists() else [])):
            _, operation, name = text.split(' ',2)
            saved_file = root/'global-presets.json'
            saved = json.loads(saved_file.read_text()) if saved_file.exists() else {}
            if operation == 'save':
                saved[name] = {'modelRoles':{key:value for key,value in roles.items() if value},'defaultThinkingLevel':overlay.get('defaultThinkingLevel','off')}
            elif operation == 'delete':
                saved.pop(name,None)
            if operation == 'save' and (root/'preset-write-delay').exists():
                import threading
                writer = threading.Timer(0.2,lambda: saved_file.write_text(json.dumps(saved)))
                writer.daemon = True
                writer.start()
            else:
                saved_file.write_text(json.dumps(saved))
            emit({'type':'command_output','text':operation+' preset '+name})
            response(command,{'agentInvoked':False})
            continue
        if text in ('/fixture-ui', '/fixture-ui-clear'):
            clear = text.endswith('-clear')
            emit({'type':'extension_ui_request','id':'status','method':'setStatus','statusKey':'fixture',**({} if clear else {'statusText':'Ready'})})
            emit({'type':'extension_ui_request','id':'widget','method':'setWidget','widgetKey':'fixture',**({} if clear else {'widgetLines':['Native widget']})})
            emit({'type':'extension_ui_request','id':text,'method':'set_editor_text','text':'' if clear else 'Native draft'})
            response(command)
            emit({'type':'prompt_result','id':prompt_id,'agentInvoked':False,'status':'completed','sessionSettled':True})
            continue
        if text == '/fixture-command-update':
            emit({'type':'available_commands_update','commands':json.loads((root/'catalog-update.json').read_text())})
            emit({'type':'command_output','text':'Command catalog updated'})
            response(command, {'agentInvoked': False})
            continue
        if text == 'interactive-fixture':
            pending_dialog = command
            dialog = json.loads((root / 'dialog.json').read_text())
            emit({'type':'extension_ui_request','id':'dialog-1',**dialog})
            continue
        plugins = json.loads((root/'plugins.json').read_text()) if (root/'plugins.json').exists() else []
        plugin = next((entry for entry in plugins if text == '/'+entry['name'] or text.startswith('/'+entry['name']+' ')), None)
        if plugin:
            if 'directOutput' in plugin:
                emit({'type':'command_output','text':plugin['directOutput']})
                response(command, {'agentInvoked':False})
                continue
            pending_plugin = (command, plugin)
            emit({'type':'extension_ui_request','id':'plugin-question','method':'select',
                  'title':plugin['description'],'options':['A, B','C']})
            continue
        if text == 'reject':
            response(command, success=False)
            continue
        if text == '/ait-vibe-mode-v1 off':
            response(command, {'agentInvoked': False})
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
