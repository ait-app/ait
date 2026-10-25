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

emit({'type':'ready','protocolVersion':1,'supportedProtocolVersions':[1,2],'maxFrameBytes':1048576,'maxReassembledFrameBytes':67108864})

for line in sys.stdin:
    command = json.loads(line)
    with log.open('a') as output:
        output.write(json.dumps(command) + '\n')
    kind = command['type']
    if kind == 'negotiate_protocol':
        response(command, {'protocolVersion':2})
    elif kind == 'get_state':
        response(command, {'sessionId': session_id, 'sessionFile': str(file), 'model': model,
                          'thinkingLevel': level, 'isStreaming': active, 'isCompacting': False,
                          'slowModeSupported': model['provider'] == 'test'})
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
    elif kind == 'get_available_commands':
        response(command, {'commands': [{'name': 'review', 'description': 'Review', 'source': 'skill'}]})
    elif kind == 'prompt':
        text = command['message']
        prompt_id = command.get('id')
        if text == 'reject':
            response(command, success=False)
            continue
        if text == '/local':
            response(command, {'agentInvoked': False})
            continue
        response(command, {'agentInvoked': True})
        active = True
        emit({'type': 'agent_start'})
        message('user', [{'type': 'text', 'text': text}, *command.get('images', [])])
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
        if text == 'wait':
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
        finish(aborted=True)
        response(command)
    elif kind == 'extension_ui_response':
        finish()
    else:
        response(command, success=False)
