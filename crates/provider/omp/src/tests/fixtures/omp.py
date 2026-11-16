#!/usr/bin/env python3
"""Offline OMP RPC peer; all state stays under an isolated agent directory."""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import signal
import sys
import threading
import time
import uuid

args = sys.argv[1:]
if args == ['--version']:
    # OMP 18.18.8 prints `omp/<version>` and exits 0 without touching its directories; an
    # invalid profile makes it exit 1 with the reason on stderr.
    if os.environ.get('OMP_FIXTURE_VERSION_FAILS'):
        sys.stderr.write('Error: Invalid OMP profile\n')
        sys.exit(1)
    print('omp/' + os.environ.get('OMP_FIXTURE_VERSION', '18.18.8'))
    sys.exit(0)
cwd = os.getcwd()
root = Path(os.environ['PI_CODING_AGENT_DIR'])
root.mkdir(parents=True, exist_ok=True)
log = root / 'calls.jsonl'
with log.open('a') as output:
    output.write(json.dumps({'args': args}) + '\n')
# Read-only metadata reads OMP exposes outside a conversation. Ait's role catalog calls these
# before any process starts, so they must answer with real JSON rather than RPC frames.
if args[:2] == ['config', 'set'] and len(args) == 5 and args[2] == 'modelPresets':
    if os.environ.get('OMP_FIXTURE_PRESET_WRITE_FAILS'):
        sys.stderr.write('Error: config write unavailable\n')
        sys.exit(1)
    value = json.loads(args[3])
    if not os.environ.get('OMP_FIXTURE_PRESET_WRITE_IGNORED'):
        staging = root / 'native-presets.tmp'
        staging.write_text(json.dumps(value))
        staging.replace(root / 'native-presets.json')
    print(json.dumps({'key': 'modelPresets', 'value': value}))
    sys.exit(0)
if args[:2] in (['config', 'list'], ['config', 'get']):
    if os.environ.get('OMP_FIXTURE_METADATA_FAILS'):
        sys.stderr.write('Error: config unavailable\n')
        sys.exit(1)
    overrides = json.loads(os.environ.get('OMP_FIXTURE_CONFIG_OVERRIDES', '{}'))
    native_file = root / 'native-presets.json'
    global_presets = json.loads(native_file.read_text()) if native_file.exists() else \
        json.loads(os.environ.get('OMP_FIXTURE_PRESETS', '{}'))
    project_presets = json.loads(os.environ.get('OMP_FIXTURE_PROJECT_PRESETS', '{}'))
    if args[:2] == ['config', 'get']:
        print(json.dumps({'key': args[2], 'value': {**global_presets, **project_presets}}))
        sys.exit(0)
    if '--scope' in args:
        scope = args[args.index('--scope') + 1]
        print(json.dumps({'modelPresets': {'value': global_presets if scope == 'global' else project_presets}}))
        sys.exit(0)
    settings = {
        'modelRoles': {'value': overrides.get('modelRoles',
            {'default': 'test/model', 'task': 'test/model:high'})},
        'modelTags': {'value': overrides.get('modelTags',
            {'default': {'name': 'Director'}, 'task': {'name': 'Worker'}})},
        'cycleOrder': {'value': overrides.get('cycleOrder', ['default', 'task'])},
        'modelPresets': {'value': {**global_presets, **project_presets}},
    }
    print(json.dumps(settings))
    sys.exit(0)
if args == ['models', '--kind', 'all', '--json']:
    if os.environ.get('OMP_FIXTURE_METADATA_FAILS'):
        sys.stderr.write('Error: catalog unavailable\n')
        sys.exit(1)
    print(json.dumps({'models': [
        {'selector': 'test/model', 'name': 'Fixture model', 'kind': 'chat',
         'input': ['text', 'image'], 'thinking': ['low', 'high']},
        {'selector': 'test/image', 'name': 'Fixture image', 'kind': 'image',
         'input': ['text'], 'thinking': []},
    ]}))
    sys.exit(0)
# Ait-only test hook: report whether this process may write outside its sandbox.
outside = os.environ.get('OMP_FIXTURE_WRITE_PROBE')
if outside:
    try:
        Path(outside).write_text('written')
        Path(outside).unlink()
        outcome = 'written'
    except OSError:
        outcome = 'denied'
    with log.open('a') as output:
        output.write(json.dumps({'writeProbe': outcome}) + '\n')
# Ait-only test hook: report which agent-directory entries this process may write.
if os.environ.get('OMP_FIXTURE_STATE_PROBE'):
    outcomes = {}
    for name in ('sessions/probe', 'blobs/probe', 'agent.db', 'config.yml', 'models.yml',
                 'extensions/probe.ts'):
        try:
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            with (root / name).open('a') as output:
                output.write('probe\n')
            outcomes[name] = 'written'
        except OSError:
            outcomes[name] = 'denied'
    with log.open('a') as output:
        output.write(json.dumps({'stateProbe': outcomes}) + '\n')

def flag(name):
    return args[args.index(name) + 1] if name in args else None

def refuse(text, status):
    # OMP 18.18.8 validates launch flags before writing ready.
    sys.stderr.write(text + '\n')
    sys.stderr.flush()
    sys.exit(status)

# OMP 18.18.8 reports each model's `input` kinds; only models listing "image" see images.
models = [
    {'provider': 'test', 'id': 'model', 'name': 'Fixture model', 'reasoning': True,
     'input': ['text', 'image'],
     'thinking': {'mode': 'effort', 'efforts': ['low', 'high']}, 'contextWindow': 32768},
    {'provider': 'other', 'id': 'model', 'name': 'Other', 'reasoning': True, 'input': ['text'],
     'thinking': {'mode': 'effort', 'efforts': ['low', 'high', 'max'], 'defaultLevel': 'high'},
     'contextWindow': 32768},
    # Guessed: a reasoning model without per-model thinking config, as older OMP reports.
    {'provider': 'test', 'id': 'any-effort', 'name': 'vendor/Any_effort', 'reasoning': True,
     'input': ['text', 'image'], 'contextWindow': 32768},
    {'provider': 'test', 'id': 'plain', 'name': 'Vendor: Plain', 'reasoning': False,
     'input': ['text'], 'contextWindow': 32768},
]
# A model removed from the catalog since a session recorded it; OMP cannot select it.
removed = os.environ.get('OMP_FIXTURE_REMOVED_MODEL')
available = [item for item in models if item['provider'] + '/' + item['id'] != removed]
levels = ('off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'auto')
approval = flag('--approval-mode')
if approval is not None and approval not in ('always-ask', 'always-ask-cached', 'write', 'yolo'):
    refuse('Error: Invalid --approval-mode value: ' + json.dumps(approval), 2)
if flag('--thinking') is not None and flag('--thinking') not in levels:
    refuse('Error: Invalid --thinking value: ' + json.dumps(flag('--thinking')), 2)

def find_model(reference):
    return next((item for item in available if item['provider'] + '/' + item['id'] == reference), None)

# --session is an alias of --resume in OMP 18.18.8: an ID prefix or a path.
file = Path(flag('--session')) if '--session' in args else None
if file and not file.exists():
    refuse('Error: Session "' + str(file) + '" not found.', 1)
model = available[0]
fast_enabled = False
# Guessed: todo phases a resumed session's state already holds.
todo_phases = json.loads(os.environ.get('OMP_FIXTURE_TODO_PHASES', '[]'))
records = []
if file:
    records = [json.loads(line) for line in file.read_text().splitlines() if json.loads(line)['type'] != 'title']
    session_id = next(record['id'] for record in records if record['type'] == 'session')
    recorded = [record['model'] for record in records if record['type'] == 'model_change']
    model = find_model(recorded[-1]) if recorded else model
    # OMP 18.18.8 rpc-ui refuses to resume when it cannot restore the session's model.
    if model is None and flag('--model') is None:
        refuse('SessionModelRestoreError: Could not restore model ' + recorded[-1], 1)
    for record in records:
        if record['type'] == 'service_tier_change':
            fast_enabled = record['serviceTier'] is not None
else:
    session_id = str(uuid.uuid4())
    # OMP 18.18.8 names a session file `<ISO time with - for :>_<session id>.jsonl`.
    file = root / 'sessions' / 'workspace' / ('2026-10-21T00-00-00-000Z_' + session_id + '.jsonl')
    records = [{'type': 'session', 'version': 3, 'id': session_id, 'cwd': cwd,
                'timestamp': '2026-10-21T00:00:00Z'}]
if flag('--model') is not None:
    model = find_model(flag('--model'))
    if model is None:
        refuse('Model "' + flag('--model') + '" not found', 1)
clock = 1792540800000 + len(records) * 10
leaf = records[-1].get('id') if len(records) > 1 else None
level = flag('--thinking') or 'off'
active = False
prompt_id = None
pending_plugin = None
pending_control = None
pending_async = False
current_prompt = None
pending_dialog = None
dialog_commands = []
pending_approval = None
ask_flow = None
ui_sequence = 0
queued_steers = []
# Ids of `prompt`s queued with a streamingBehavior during the active run.
queued_prompts = []
auto_compaction = True
protocol = 1
# A delayed compaction answers from a timer thread while stdin keeps being served.
output_lock = threading.Lock()
slow_commands = set(filter(None, os.environ.get('OMP_FIXTURE_SLOW', '').split(',')))
# Commands logged but never answered, as a stalled catalog refresh is.
unanswered_commands = set(filter(None, os.environ.get('OMP_FIXTURE_UNANSWERED', '').split(',')))
shutdown = os.environ.get('OMP_FIXTURE_SHUTDOWN')

def write_line(text):
    # The garbage-line prompt also proves CRLF line endings are accepted.
    ending = '\r\n' if current_prompt == 'garbage-line' else '\n'
    with output_lock:
        sys.stdout.write(text + ending)
        sys.stdout.flush()

def emit(value):
    data = json.dumps(value, ensure_ascii=False).encode()
    # OMP 18.18.8 chunks only after negotiating v2, and only frames whose line (with its
    # newline) exceeds 1 MiB, using 256 KiB chunks. In v1 it elides large frames instead;
    # this fixture never sends those in v1, so it writes them whole.
    if protocol != 2 or len(data) + 1 <= 1048576:
        write_line(data.decode())
        return
    import base64
    chunks = [data[index:index+262144] for index in range(0, len(data), 262144)]
    chunk_id = 'rpc-' + str(uuid.uuid4())
    for index, chunk in enumerate(chunks):
        write_line(json.dumps({'type':'rpc_chunk','chunkId':chunk_id,'index':index,'count':len(chunks),'byteLength':len(data),'data':base64.b64encode(chunk).decode()}))

def response(command, data=None, success=True):
    emit({'type': 'response', 'id': command.get('id'), 'command': command['type'],
          'success': success, 'data': data, 'error': None if success else 'Rejected'})

def persist():
    if '--no-session' not in args:
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(''.join(json.dumps(record) + '\n' for record in records))

def entry(kind, **fields):
    # Control entries reach disk with the next message, or at once when the file exists.
    global leaf
    entry_id = str(uuid.uuid4())
    records.append({'type': kind, 'id': entry_id, 'parentId': leaf,
                    'timestamp': '2026-10-21T00:00:01Z', **fields})
    leaf = entry_id
    if file.exists():
        persist()

def fast_active():
    # Guessed: only the test provider applies Fast; real OMP decides per model.
    return fast_enabled and model['provider'] == 'test'

stream_messages = 0

def announce(value, entry_id, kind):
    # OMP 18.18.8 numbers each streamed message (`messageId: msg-N`, not an entry id) and puts
    # no entry id on the message itself. It records an entry before announcing its end, so
    # `get_branch_messages` lists a user message as soon as its message_end arrives.
    global stream_messages
    stream_messages += 1
    frame = {'message': value, 'messageId': 'msg-' + str(stream_messages)}
    if kind == 'start':
        emit({'type': 'message_start', **frame})
        return
    if os.environ.get('OMP_FIXTURE_MESSAGE_IDS') and value['role'] == 'user':
        # Guessed: an OMP that names the entry on the message, as Paseo reads `id`.
        frame['message'] = {**value, 'id': entry_id}
    emit({'type': 'message_end', **frame})

def message(role, content, **fields):
    global clock, leaf
    clock += 10
    value = {'role': role, 'content': content, 'timestamp': clock, **fields}
    entry_id = str(uuid.uuid4())
    announce(value, entry_id, 'start')
    if role == 'assistant':
        for index, block in enumerate(content):
            if block['type'] in ('text', 'thinking'):
                field = 'text' if block['type'] == 'text' else 'thinking'
                emit({'type': 'message_update', 'message': value, 'assistantMessageEvent': {
                    'type': field + '_delta', 'contentIndex': index, 'delta': block[field]}})
    records.append({'type': 'message', 'id': entry_id, 'parentId': leaf,
                    'timestamp': '2026-10-21T00:00:01Z', 'message': value})
    leaf = entry_id
    persist()
    announce(value, entry_id, 'end')
    return value, entry_id

def custom_message(custom_type, content, details=None, attribution='agent', display=True):
    # OMP 18.18.8 records a custom message as a `custom_message` entry stamped with the
    # message's own time.
    global clock, leaf
    clock += 10
    fields = {'customType': custom_type, 'content': content, 'display': display,
              'details': details, 'attribution': attribution}
    value = {'role': 'custom', **fields, 'timestamp': clock}
    entry_id = str(uuid.uuid4())[:8]
    announce(value, entry_id, 'start')
    stamp = datetime.fromtimestamp(clock / 1000, timezone.utc).isoformat(timespec='milliseconds')
    records.append({'type': 'custom_message', **fields, 'id': entry_id, 'parentId': leaf,
                    'timestamp': stamp.replace('+00:00', 'Z')})
    leaf = entry_id
    persist()
    announce(value, entry_id, 'end')

def skill_prompt(text):
    # OMP 18.18.8 expands `/skill:<name> <args>` into a user-attributed custom message.
    name, _, args = text[len('/skill:'):].partition(' ')
    details = {'name': name, 'path': cwd + '/.omp/skills/' + name + '/SKILL.md', 'args': args,
               'prompt': text, 'lineCount': 1}
    content = ('[IMPORTANT: User invoked the "' + name + '" skill; follow its instructions. '
               'Full skill below.]\n\nReview the code carefully.\nUser: ' + args)
    custom_message('skill-prompt', content, details, attribution='user')

def branch_text(content):
    # OMP 18.18.8 `getUserMessagesForBranching`: text blocks joined without a separator.
    if isinstance(content, str):
        return content
    return ''.join(block['text'] for block in content if block.get('type') == 'text')

def finish(settled=True):
    global active, queued_prompts
    message('assistant', [{'type': 'text', 'text': 'Hello world'}],
            provider='test', model='model', stopReason='stop',
            usage={'input': 10, 'output': 5, 'cacheRead': 2, 'cacheWrite': 0, 'totalTokens': 17,
                   'cost': {'total': 0.001}})
    emit({'type': 'agent_end', 'messages': []})
    active = False
    # OMP 18.18.8 reports each prompt queued during the run before the run's own prompt.
    for queued in queued_prompts:
        emit({'type': 'prompt_result', 'id': queued, 'agentInvoked': True, 'status': 'completed',
              'sessionSettled': True})
    queued_prompts = []
    emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True, 'status': 'completed', 'sessionSettled': settled})
    if not settled:
        emit({'type': 'agent_start'})
        message('assistant', [{'type': 'text', 'text': 'Background finished'}],
                provider='test', model='model', stopReason='stop')
        emit({'type': 'agent_end', 'messages': []})
    emit({'type': 'session_settled'})

def ui_request(method, **fields):
    # OMP 18.18.8 numbers its own extension UI requests.
    global ui_sequence
    ui_sequence += 1
    emit({'type': 'extension_ui_request', 'id': 'ui-' + str(ui_sequence), 'method': method,
          **fields})

def tool_call(call, name, arguments):
    message('assistant', [{'type': 'toolCall', 'id': call, 'name': name, 'arguments': arguments}],
            provider='test', model='model', stopReason='toolUse')

def tool_end(call, name, text, error=False, details=None):
    # OMP 18.18.8 ends a tool's execution, then records its result with the same content and
    # details.
    result = {'content': [{'type': 'text', 'text': text}], 'details': details or {}}
    emit({'type': 'tool_execution_end', 'toolCallId': call, 'toolName': name,
          'result': result, 'isError': error})
    message('toolResult', result['content'], toolCallId=call, toolName=name,
            details=result['details'], isError=error)

def timeline_mapping():
    # Shapes recorded from OMP 18.18.8 (B6); `compaction_*`, `goal_updated` and the retry
    # events were not triggered locally and follow OMP's RPC serializer and Paseo.
    tools = [
        ('edit-1', 'edit', {'i': 'e', 'input': '[probe.txt#0097]\nPUT 1.=1:\n+line 1\n'},
         '[probe.txt#5D8B]\n1:line 1\n2:line two', False,
         {'diff': '-1|line one\n+1|line 1\n 2|line two', 'firstChangedLine': 1, 'op': 'update',
          'path': cwd + '/probe.txt', 'oldText': 'line one\nline two\n',
          'newText': 'line 1\nline two\n'}),
        ('read-1', 'read', {'i': 'r', 'path': 'probe.txt'}, '[probe.txt#5D8B]\n1:line 1', False,
         {'totalLines': 1, 'rowPrefixes': True,
          'displayContent': {'text': 'line 1', 'startLine': 1, 'lineNumbers': [1]}}),
        # Guessed: web search failures report `details.error` without `isError`, as Paseo reads.
        ('search-1', 'web_search', {'i': 's', 'query': 'rust serde'}, '', False,
         {'error': 'All web search providers failed'}),
        ('todo-1', 'todo', {'i': 't', 'op': 'init', 'list': [{'phase': 'Build',
                                                               'items': ['one', 'two']}]},
         'Remaining items (2)', False,
         {'op': 'init', 'storage': 'session', 'phases': [{'name': 'Build', 'tasks': [
             {'content': 'one', 'status': 'in_progress'},
             {'content': 'two', 'status': 'blocked', 'blocker': 'waiting on api'},
             {'content': 'three', 'status': 'abandoned'}]}]}),
        ('task-1', 'task', {'i': 'delegate', 'context': '# Goal', 'tasks': [
            {'task': '# Target\nReply', 'solutionSpace': 'one fix', 'name': 'Prober'}]},
         'Spawned agent `Prober` (job `Prober`).', False,
         {'results': [], 'async': {'state': 'running', 'jobId': 'Prober', 'type': 'task'}}),
    ]
    message('assistant', [{'type': 'toolCall', 'id': call, 'name': name, 'arguments': arguments}
                          for call, name, arguments, *_ in tools],
            provider='test', model='model', stopReason='toolUse')
    for call, name, arguments, *_ in tools:
        emit({'type': 'tool_execution_start', 'toolCallId': call, 'toolName': name,
              'args': {key: value for key, value in arguments.items() if key != 'i'},
              'intent': arguments['i']})
    for call, name, _, text, error, details in tools:
        tool_end(call, name, text, error, details)
    emit({'type': 'notice', 'level': 'warning', 'source': 'cost-gate',
          'message': 'Cost warning: this session has spent $1.00, past its $0.50 warning threshold.'})
    emit({'type': 'goal_updated', 'goal': {'id': 'goal-1', 'objective': 'Ship', 'status': 'active'},
          'state': {'enabled': True, 'mode': 'goal'}})
    emit({'type': 'auto_retry_start', 'attempt': 1, 'maxAttempts': 3, 'delayMs': 2000,
          'errorMessage': 'overloaded', 'errorId': 529})
    emit({'type': 'auto_retry_end', 'success': True, 'attempt': 1})
    emit({'type': 'retry_fallback_applied', 'from': 'test/model', 'to': 'other/model',
          'role': 'default', 'reason': 'rate limit'})
    emit({'type': 'retry_fallback_succeeded', 'model': 'other/model', 'role': 'default'})
    emit({'type': 'auto_compaction_start', 'reason': 'threshold', 'action': 'handoff'})
    emit({'type': 'auto_compaction_end', 'action': 'handoff', 'aborted': False,
          'willRetry': False, 'result': {'summary': 'Summary', 'tokensBefore': 100010}})
    emit({'type': 'manual_compaction_start'})
    emit({'type': 'manual_compaction_end', 'aborted': False, 'skipped': True,
          'errorMessage': 'Nothing to compact (session too small)'})
    emit({'type': 'compaction_start', 'reason': 'manual'})
    emit({'type': 'compaction_end', 'reason': 'manual'})
    custom_message('async-result',
                   '<system-notice>\nBackground job Prober has completed. Resume your work using '
                   'the result below.\n<task-result id="Prober" agent="task" status="completed" '
                   'duration="50ms">\n\n<output>\nchild done\n</output>\n</task-result>\n\n'
                   'Prober is now idle\n</system-notice>',
                   {'jobs': [{'jobId': 'Prober', 'type': 'task', 'status': 'completed',
                              'agentId': 'Prober', 'label': 'Prober', 'durationMs': 49}]})
    # Guessed advisor note shape (Paseo's advisor mapper); OMP sends `details.notes`.
    custom_message('advisor', 'Advice', {'notes': [
        {'note': 'Handle the error', 'severity': 'blocker', 'advisor': 'rust'}]})
    custom_message('vendor-note', 'Plain extension output', {'k': 1})
    custom_message('hidden-context', 'Private context', display=False)
    emit({'type': 'todo_reminder', 'todos': [{'content': 'one', 'status': 'in_progress'}],
          'attempt': 1, 'maxAttempts': 3})

def subagent_task():
    # Shapes from OMP 18.18.8 (B6/B7 probes): a child session file at
    # `<parent file without .jsonl>/<job>.jsonl` whose header names `parentSession`, and
    # lifecycle and progress frames carrying `sessionFile`. Guessed: a task that waits for its
    # child (18.18.8 runs tasks in the background) and reports `details.results`, as Paseo
    # reads them, and a lifecycle `description` (18.18.8 sends only the progress `assignment`).
    child_file = file.with_suffix('') / 'worker.jsonl'
    tool_call('task-1', 'task', {'i': 'Delegate review', 'context': '# Goal', 'tasks': [
        {'task': '# Target\nReview', 'name': 'worker'}]})
    emit({'type': 'tool_execution_start', 'toolCallId': 'task-1', 'toolName': 'task',
          'args': {'context': '# Goal', 'tasks': [{'task': '# Target\nReview', 'name': 'worker'}]},
          'intent': 'Delegate review'})
    lifecycle = {'id': 'worker', 'index': 0, 'agent': 'Task', 'agentSource': 'bundled',
                 'parentToolCallId': 'task-1', 'sessionFile': str(child_file),
                 'description': 'Review the change'}
    emit({'type': 'subagent_lifecycle', 'payload': {**lifecycle, 'status': 'started'}})
    emit({'type': 'subagent_progress', 'payload': {
        'index': 0, 'agent': 'Task', 'agentSource': 'bundled', 'task': 'Complete assignment',
        'assignment': '# Target\nReview', 'parentToolCallId': 'task-1',
        'sessionFile': str(child_file), 'progress': {
            'index': 0, 'id': 'worker', 'agent': 'Task', 'status': 'running',
            'task': 'Complete assignment', 'assignment': '# Target\nReview',
            'resolvedModel': 'test/model'}}})
    child_message = {'role': 'assistant', 'timestamp': 1792540800300, 'provider': 'test',
                     'model': 'model', 'stopReason': 'stop',
                     'content': [{'type': 'text', 'text': 'Child output'}]}
    emit({'type': 'subagent_event', 'payload': {'id': 'worker', 'event': {
        'type': 'message_end', 'message': child_message}}})
    child_file.parent.mkdir(parents=True, exist_ok=True)
    child_records = [
        {'type': 'title', 'v': 1, 'title': '', 'updatedAt': '2026-10-21T00:00:02.000Z'},
        {'type': 'session', 'version': 3, 'id': str(uuid.uuid4()), 'cwd': cwd,
         'timestamp': '2026-10-21T00:00:02.000Z', 'parentSession': str(file)},
        {'type': 'model_change', 'id': 'c1', 'parentId': None,
         'timestamp': '2026-10-21T00:00:02.000Z', 'model': 'test/model'},
        {'type': 'session_init', 'id': 'c2', 'parentId': 'c1',
         'timestamp': '2026-10-21T00:00:02.000Z', 'systemPrompt': ['child system prompt']},
        {'type': 'message', 'id': 'c3', 'parentId': 'c2', 'timestamp': '2026-10-21T00:00:02.100Z',
         'message': {'role': 'user', 'content': [{'type': 'text', 'text': 'Review'}],
                     'attribution': 'agent', 'timestamp': 1792540800200}},
        {'type': 'message', 'id': 'c4', 'parentId': 'c3', 'timestamp': '2026-10-21T00:00:02.300Z',
         'message': child_message},
    ]
    child_file.write_text(''.join(json.dumps(record) + '\n' for record in child_records))
    emit({'type': 'subagent_lifecycle', 'payload': {**lifecycle, 'status': 'completed'}})
    tool_end('task-1', 'task', 'worker finished', details={
        'results': [{'id': 'worker', 'agent': 'Task', 'exitCode': 0}],
        'progress': [{'index': 0, 'id': 'worker', 'agent': 'Task', 'status': 'completed',
                      'assignment': '# Target\nReview'}]})

# OMP 18.18.8's `ask` takes one HTML form and asks it field by field (tools/ask.ts): a radio
# group is a select with "Other (type your own)", whose typed text follows as an `input`; a
# checkbox group re-prompts "(n selected) <title>", toggling choices until "✔ Done selecting",
# which it offers only while the group is optional or something is checked.
ASK_OTHER = 'Other (type your own)'
ASK_DONE = '\u2714 Done selecting'
ASK_FIELDS = [
    {'name': 'db', 'title': 'Which database?', 'labels': ['A,B', 'C'], 'multi': False},
    {'name': 'features', 'title': 'Which features?', 'labels': ['Auth', 'Search'], 'multi': True,
     'required': os.environ.get('OMP_FIXTURE_ASK_REQUIRED') == '1'},
]

def ask_prompt():
    field = ASK_FIELDS[ask_flow['field']]
    if ask_flow['typing']:
        ui_request('input', title=field['title'])
        return
    count = len(ask_flow['checked']) + (1 if ask_flow['other'] is not None else 0)
    options = field['labels'] + [ASK_OTHER]
    title = field['title']
    if field['multi']:
        if not field.get('required') or count:
            options = options + [ASK_DONE]
        if count:
            title = '(' + str(count) + ' selected) ' + title
    ui_request('select', title=title, options=options)

def ask_next(value):
    global ask_flow
    field = ASK_FIELDS[ask_flow['field']]
    ask_flow['answers'][field['name']] = value
    ask_flow.update(field=ask_flow['field'] + 1, checked=[], other=None, typing=False)
    if ask_flow['field'] < len(ASK_FIELDS):
        ask_prompt()
        return
    answers, ask_flow = ask_flow['answers'], None
    tool_end('ask-call', 'ask', 'User answers:\n' + json.dumps(answers, indent=2),
             details={'answers': answers})
    finish()

def ask_answer(command):
    global ask_flow
    field = ASK_FIELDS[ask_flow['field']]
    value = command.get('value')
    if command.get('cancelled') or value is None:
        ask_flow = None
        tool_end('ask-call', 'ask', 'Ask tool was cancelled by the user', error=True)
        end_aborted_run()
        emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True,
              'status': 'aborted', 'sessionSettled': True})
        emit({'type': 'session_settled'})
    elif ask_flow['typing']:
        ask_flow['typing'] = False
        if not field['multi']:
            ask_next(value)
            return
        ask_flow['other'] = value
        ask_prompt()
    elif value == ASK_OTHER:
        ask_flow['typing'] = True
        ask_prompt()
    elif field['multi'] and value == ASK_DONE:
        ask_next(ask_flow['checked'] + ([ask_flow['other']] if ask_flow['other'] else []))
    elif value in field['labels'] and field['multi']:
        checked = ask_flow['checked']
        checked.remove(value) if value in checked else checked.append(value)
        ask_prompt()
    elif value in field['labels']:
        ask_next(value)
    else:
        ask_prompt()

def end_aborted_run():
    # OMP 18.18.8 ends the aborted run, then answers abort, then reports the prompt.
    global active
    message('assistant', [{'type': 'text', 'text': 'Stopped'}], provider='test', model='model',
            stopReason='aborted', errorMessage='Interrupted by user')
    emit({'type': 'agent_end', 'messages': []})
    active = False

def run_queued(command):
    # OMP 18.18.8 keeps steering through a plain abort and runs it at once as a new run;
    # the aborted prompt's result arrives during that run, settlement after it.
    global active, queued_steers
    emit({'type': 'queue_update', 'steering': [], 'followUp': [], 'liveSteered': 0})
    active = True
    emit({'type': 'agent_start'})
    for steer in queued_steers:
        message('user', steer)
    queued_steers = []
    emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True, 'status': 'aborted',
          'sessionSettled': False})
    message('assistant', [{'type': 'text', 'text': 'Steered reply'}], provider='test',
            model='model', stopReason='stop')
    emit({'type': 'agent_end', 'messages': []})
    active = False
    emit({'type': 'session_settled'})

def current_messages():
    # OMP 18.18.8 counts the messages kept since the last compaction (or handoff).
    kept = []
    for record in records:
        if record['type'] == 'compaction':
            kept = []
        elif record['type'] == 'message':
            kept.append(record['message'])
    return kept

def context_usage():
    # OMP 18.18.8 reports the last response's prompt size, else an estimate of the system
    # prompt and tools (8586 tokens for the probe's empty session).
    answered = [item['usage'] for item in current_messages() if item.get('usage')]
    tokens = answered[-1].get('totalTokens', 0) if answered else 8586
    return {'tokens': tokens, 'contextWindow': model['contextWindow'],
            'percent': tokens * 100 / model['contextWindow']}

def session_stats():
    # OMP 18.18.8 `getSessionStats`: sums over the kept messages, with context usage.
    kept = current_messages()
    usages = [item['usage'] for item in kept if item.get('usage')]
    tokens = {field: sum(usage.get(field, 0) for usage in usages)
              for field in ('input', 'output', 'cacheRead', 'cacheWrite')}
    tokens['reasoning'] = 0
    tokens['total'] = sum(usage.get('totalTokens', 0) for usage in usages)
    return {'sessionFile': str(file), 'sessionId': session_id,
            'userMessages': sum(item['role'] == 'user' for item in kept),
            'assistantMessages': sum(item['role'] == 'assistant' for item in kept),
            'toolCalls': 0, 'toolResults': 0, 'totalMessages': len(kept), 'tokens': tokens,
            'cost': sum(usage.get('cost', {}).get('total', 0) for usage in usages),
            'premiumRequests': 0, 'contextUsage': context_usage()}

def compact(command):
    # OMP 18.18.8 answers `compact` with its result after manual_compaction_start/_end; it
    # refuses a session with nothing to compact the same way.
    emit({'type': 'manual_compaction_start'})
    if not current_messages():
        error = 'Nothing to compact (session too small)'
        emit({'type': 'manual_compaction_end', 'aborted': False, 'skipped': True,
              'errorMessage': error})
        emit({'type': 'response', 'id': command.get('id'), 'command': 'compact',
              'success': False, 'error': error})
        return
    def answer():
        hold = root / 'release-compact'
        while os.environ.get('OMP_FIXTURE_HOLD_COMPACT') and not hold.exists():
            time.sleep(0.01)
        result = {'summary': 'Summary', 'shortSummary': 'Summary', 'firstKeptEntryId': leaf,
                  'tokensBefore': context_usage()['tokens'],
                  'details': {'readFiles': [], 'modifiedFiles': []}}
        entry('compaction', summary='Summary', firstKeptEntryId=leaf,
              tokensBefore=result['tokensBefore'], tokensAfter=100, method='summary')
        persist()
        emit({'type': 'manual_compaction_end', 'result': result, 'aborted': False})
        response(command, result)
    # Guessed delay: a summary model call takes a while.
    timer = threading.Timer(0.1, answer)
    timer.daemon = True
    timer.start()

def stop(signum, frame):
    (root / 'sigterm').touch()
    sys.exit(0)

if len(records) == 1 or flag('--model') is not None:
    entry('model_change', model=model['provider'] + '/' + model['id'])

if shutdown == 'term':
    signal.signal(signal.SIGTERM, stop)
elif shutdown == 'stubborn':
    signal.signal(signal.SIGTERM, signal.SIG_IGN)

ready = {'type':'ready','protocolVersion':1,'supportedProtocolVersions':[1,2],'maxFrameBytes':1048576,
         'maxReassembledFrameBytes':67108864,'supportsBashExcludeFromContext':True}
mode = os.environ.get('OMP_FIXTURE_READY')
if mode == 'v1':
    ready['supportedProtocolVersions'] = [1]
elif mode == 'sizes':
    ready['maxFrameBytes'] = 2 * 1048576
elif mode == 'late':
    # Guessed: real OMP writes ready first; these only prove Ait waits for ready.
    print('{not json', flush=True)
    emit({'type':'notice','level':'info','message':'before ready'})
if mode != 'never':
    emit(ready)
    # OMP 18.18.8 announces its command list right after ready.
    emit({'type': 'available_commands_update', 'commands': [
        {'name': 'compact', 'description': 'Compact the session context', 'source': 'builtin'},
        {'name': 'handoff', 'description': 'Hand off to a fresh context', 'source': 'builtin'},
        {'name': 'review', 'description': 'Review', 'source': 'skill'}]})
if os.environ.get('OMP_FIXTURE_REPLAY') and '--session' in args:
    # Guessed: OMP 18.18.8 replays nothing on --session; Paseo suppresses replay defensively.
    for record in records:
        if record['type'] == 'message':
            emit({'type': 'message_start', 'message': record['message']})
            emit({'type': 'message_end', 'message': record['message']})

for line in sys.stdin:
    command = json.loads(line)
    with log.open('a') as output:
        output.write(json.dumps(command) + '\n')
    kind = command['type']
    if kind in slow_commands:
        time.sleep(0.3)
    if kind in unanswered_commands:
        continue
    if pending_dialog and kind in ('get_state', 'steer'):
        dialog_commands.append(command)
        continue
    if kind == 'negotiate_protocol':
        response(command, {'protocolVersion':2})
        protocol = 2
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
                          'slowModeSupported': model['provider'] == 'test',
                          'autoCompactionEnabled': auto_compaction,
                          'contextUsage': context_usage()}
        if not os.environ.get('OMP_FIXTURE_NO_FAST_STATE'):
            # OMP 18.18.8 always reports both facts; older releases omit them.
            state['fastModeEnabled'] = fast_enabled
            state['fastModeActive'] = fast_active()
        if current_prompt == 'missing-settlement-fact':
            state.pop('isSettled')
        # OMP 18.18.8 reports the session's todo phases in its state.
        state['todoPhases'] = todo_phases
        response(command, state)
    elif kind == 'fixture_new_context':
        session_id=str(uuid.uuid4())
        file=root/'sessions'/'workspace'/('2026-10-25T00-00-00-000Z_'+session_id+'.jsonl')
        records=[{'type':'session','version':3,'id':session_id,'cwd':cwd,'timestamp':'2026-10-25T00:00:00Z'}]
        response(command, {'sessionId':session_id})
    elif kind == 'get_available_models':
        response(command, {'models': available})
    elif kind == 'get_available_thinking_levels':
        response(command, {'levels': ['off', *model['thinking']['efforts']]})
    elif kind == 'set_model':
        selected = find_model(command['provider'] + '/' + command['modelId'])
        if selected and not os.environ.get('OMP_FIXTURE_SET_MODEL_FAILS'):
            model = selected
            entry('model_change', model=model['provider'] + '/' + model['id'])
            # OMP 18.18.8 announces a field-less model_changed before answering set_model.
            emit({'type': 'model_changed'})
            response(command, model)
        else:
            response(command, success=False)
    elif kind == 'set_thinking_level':
        level = command['level']
        response(command)
    elif kind == 'set_fast_mode':
        if command['enabled'] and model['provider'] != 'test':
            emit({'type': 'response', 'id': command.get('id'), 'command': kind, 'success': False,
                  'error': 'Fast mode is unavailable for the current model.'})
            continue
        fast_enabled = command['enabled']
        # Guessed shape of the persisted tier; OMP 18.18.8 records it per provider.
        entry('service_tier_change', serviceTier={'test': 'priority'} if fast_enabled else None)
        response(command, {'enabled': fast_enabled, 'active': fast_active()})
    elif kind == 'set_slow_mode':
        response(command, success=not command['enabled'] or model['provider'] == 'test')
    elif kind == 'set_ask_dialog' and os.environ.get('OMP_FIXTURE_REJECT_ASK_DIALOG'):
        # Guessed: an OMP without the structured ask dialog refuses the unknown command.
        response(command, success=False)
    elif kind == 'set_subagent_subscription' and command['level'] in os.environ.get('OMP_FIXTURE_REJECT_SUBSCRIPTION', '').split(','):
        # Guessed: an OMP without that subscription level refuses it.
        response(command, success=False)
    elif kind == 'set_auto_compaction':
        auto_compaction = command['enabled']
        response(command)
    elif kind == 'get_session_stats':
        if os.environ.get('OMP_FIXTURE_REJECT_STATS'):
            # Guessed: an OMP before get_session_stats refuses the unknown command.
            emit({'type': 'response', 'id': command.get('id'), 'command': kind, 'success': False,
                  'error': 'Unknown command: get_session_stats'})
        else:
            response(command, session_stats())
    elif kind == 'compact':
        compact(command)
    elif kind == 'handoff':
        # OMP 18.18.8 refuses a handoff while busy or with nothing to hand off, and otherwise
        # records it as a compaction entry without any event.
        if active:
            emit({'type': 'response', 'id': command.get('id'), 'command': kind, 'success': False,
                  'error': 'Cannot hand off while a response is in progress'})
        elif not current_messages():
            emit({'type': 'response', 'id': command.get('id'), 'command': kind, 'success': False,
                  'error': 'Nothing to hand off (already compacted)'})
        else:
            entry('compaction', summary='Handoff', firstKeptEntryId=leaf, tokensBefore=17,
                  tokensAfter=100, method='handoff')
            persist()
            response(command, {})
    elif kind in ('set_auto_retry', 'set_ask_dialog', 'set_subagent_subscription'):
        response(command)
    elif kind == 'get_branch_messages':
        # OMP 18.18.8 lists every recorded user message entry with text, in file order.
        response(command, {'messages': [
            {'entryId': record['id'], 'text': branch_text(record['message']['content'])}
            for record in records
            if record['type'] == 'message' and record['message']['role'] == 'user'
            and branch_text(record['message']['content'])]})
    elif kind == 'get_subagents':
        response(command, {'subagents': []})
    elif kind == 'get_available_commands':
        response(command, {'commands': [{'name': 'review', 'description': 'Review', 'source': 'skill'}, *json.loads((root/'plugins.json').read_text())] if (root/'plugins.json').exists() else [{'name': 'review', 'description': 'Review', 'source': 'skill'}]})
    elif kind == 'interactive_control':
        pending_control=command
        emit({'type':'extension_ui_request','id':'control-question','method':'input','title':'Native control input'})
    elif kind == 'host_tool_update':
        pass
    elif kind == 'host_tool_result':
        finish()
    elif kind == 'prompt' and active and command.get('streamingBehavior'):
        # OMP 18.18.8 queues a prompt with a streamingBehavior during a run, delivers it in
        # that run (a steer marked `steering`) and reports it before the run's prompt.
        if current_prompt == 'steer-reject':
            # Guessed refusal text, as for `steer`; the turn then completes normally.
            emit({'type': 'response', 'id': command.get('id'), 'command': 'prompt',
                  'success': False, 'error': 'Steering is unavailable api_key=fixture-secret'})
            finish()
            continue
        steering = command['streamingBehavior'] == 'steer'
        key = 'steering' if steering else 'followUp'
        emit({'type': 'queue_update', 'steering': [], 'followUp': [], key: [command['message']],
              'liveSteered': 0})
        response(command)
        queued_prompts.append(command.get('id'))
        if steering:
            message('user', [{'type': 'text', 'text': command['message']}], steering=True)
        else:
            message('user', [{'type': 'text', 'text': command['message']}])
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
        if text in ('/local', '/local-result'):
            # OMP 18.18.8 writes a builtin command's output before acknowledging it locally.
            emit({'type': 'command_output', 'text': '\x1b[1mSession\x1b[0m: \x1b]8;;file:///x\x07local\x1b]8;;\x07 \n'})
            response(command, {'agentInvoked': False})
            if text == '/local-result':
                # Paseo's OMP 17.0.5 compatibility shape; 18.18.8 sends no result for builtins.
                emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': False,
                      'status': 'completed', 'sessionSettled': True})
            continue
        if text in ('late-reject', 'late-reject-result'):
            # A prompt that fails after admission: OMP 18.3 answers it a second time; 18.18.8
            # also sends its failed prompt_result (rpc-prompt-results.ts `fail`).
            response(command)
            emit({'type': 'response', 'id': prompt_id, 'command': 'prompt', 'success': False,
                  'error': 'No API key found for test.'})
            if text == 'late-reject-result':
                emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': False,
                      'status': 'error', 'sessionSettled': True,
                      'error': {'message': 'No API key found for test.', 'retryable': False}})
            continue
        response(command, {'agentInvoked': True})
        active = True
        emit({'type': 'agent_start'})
        if text.startswith('/skill:'):
            skill_prompt(text)
            if text.endswith(' wait'):
                # Await a steer, which the steer branch answers and finishes.
                continue
            finish()
            continue
        images = command.get('images', [])
        if images and 'image' not in model['input']:
            # OMP 18.18.8 saves the images for a text-only model and describes them, one text
            # block each, in a hidden user-attributed message before the user message, which
            # keeps the images.
            custom_message('image-attachment-description', [{'type': 'text', 'text':
                '<image path="local://image-' + str(index) + '.webp">\n[No vision-capable model '
                'is configured, so this image could not be described automatically.]\n</image>'}
                for index, _ in enumerate(images)], attribution='user', display=False)
        user, entry_id = message('user', [{'type': 'text', 'text': text}, *images])
        if text == 'duplicate-user':
            # Guessed: Paseo dedupes OMP announcing a user message again (after a steer or a
            # resumed process); OMP 18.18.8 was not seen doing so.
            announce(user, entry_id, 'start')
            announce(user, entry_id, 'end')
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
            message('toolResult', [{'type': 'text', 'text': 'unmatched result'}],
                    toolCallId='unknown', toolName='bash', isError=True)
        if text == 'uncorrelated-failure':
            emit({'type': 'response', 'id': 'not-a-request', 'command': 'get_state',
                  'success': False, 'error': 'Unknown request'})
        if text == 'invalid-usage':
            message('assistant', [], usage={'input': -1, 'output': 0})
            continue
        if text == 'closed':
            break
        if text == 'crash':
            sys.stderr.write('filler diagnostic line\n' * 400)
            sys.stderr.write('fatal: worker crashed api_key=fixture-secret see '
                             'https://example.com/private/path?token=fixture-secret\n')
            sys.stderr.flush()
            os._exit(3)
        if text == 'killed':
            os.kill(os.getpid(), signal.SIGKILL)
        if text == 'incomplete-frame':
            sys.stdout.write('{"type":')
            sys.stdout.flush()
            break
        if text == 'invalid-chunks':
            large = 1048576
            for chunk in (
                {'chunkId': 'stray', 'index': 1, 'count': 5, 'byteLength': large, 'data': 'e30='},
                {'chunkId': 'single', 'index': 0, 'count': 1, 'byteLength': large, 'data': 'e30='},
                {'chunkId': 'small', 'index': 0, 'count': 2, 'byteLength': 4, 'data': 'e30='},
                {'chunkId': 'padding', 'index': 0, 'count': 5, 'byteLength': large, 'data': 'e31='},
                {'chunkId': 'x' * 129, 'index': 0, 'count': 5, 'byteLength': large, 'data': 'e30='},
                {'chunkId': 'short', 'index': 0, 'count': 2, 'byteLength': large, 'data': 'e30='},
                {'chunkId': 'short', 'index': 1, 'count': 2, 'byteLength': large, 'data': 'e30='},
            ):
                write_line(json.dumps({'type': 'rpc_chunk', **chunk}))
        if text == 'non-object-line':
            for line in ('[1,2]', '"text"', '42', 'null'):
                write_line(line)
            emit({'content': 'api_key=fixture-secret'})
            emit({'type': 7})
        if text in ('native-error', 'native-error-recovered'):
            message('assistant', [{'type': 'text', 'text': 'Partial ' + 'x' * 600}],
                    provider='test', model='model', stopReason='error',
                    responseModel='test-model-2026', responseId='resp_1',
                    errorMessage='Payment required: insufficient balance api_key=fixture-secret')
            if text == 'native-error':
                active = False
                emit({'type':'prompt_result','id':prompt_id,'status':'error','sessionSettled':True,
                      'error':'Payment required: insufficient balance api_key=fixture-secret'})
                continue
        if text == 'garbage-line':
            write_line('{broken api_key=fixture-secret')
            write_line('')
            write_line('   ')
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
        if text in ('wait', 'wait-slow-abort', 'wait-lost-abort', 'wait-stuck-abort', 'wait-queue',
                    'steer-reject'):
            emit({'type': 'agent_end', 'messages': []})
            emit({'type': 'auto_retry_start', 'attempt': 1})
            continue
        if text == 'tool-abort':
            # Guessed: a tool that ignores abort and never reports an end. OMP 18.18.8's bash
            # instead ends with `Command aborted` before the aborted assistant message.
            message('assistant', [{'type': 'toolCall', 'id': 'stuck-tool', 'name': 'bash',
                                   'arguments': {'command': 'sleep 30'}}],
                    provider='test', model='model', stopReason='toolUse')
            emit({'type': 'tool_execution_start', 'toolCallId': 'stuck-tool', 'toolName': 'bash',
                  'args': {'command': 'sleep 30'}})
            emit({'type':'subagent_lifecycle','payload':{'id':'worker','index':0,'agent':'Task','status':'started','parentToolCallId':'stuck-tool'}})
            continue
        if text == 'approval':
            command_text = "printf 'ok'\necho done"
            tool_call('approval-call', 'bash', {'command': command_text})
            emit({'type': 'tool_execution_start', 'toolCallId': 'approval-call', 'toolName': 'bash',
                  'args': {'command': command_text}})
            pending_approval = command_text
            ui_request('select', title='Allow tool: bash\nCommand: ' + command_text,
                       options=['Approve', 'Approve bash for this session', 'Deny'])
            continue
        if text == 'open-url':
            # OMP 18.18.8 sends open_url only during its `login` command; Ait never logs in.
            ui_request('open_url', url='https://example.com/device',
                       launchUrl='https://example.com/device?code=ABCD',
                       instructions='Enter code ABCD')
            finish()
            continue
        if text == 'html-ask':
            # OMP 18.18.8 asks the first field before announcing the tool's execution.
            html = '<form>…</form>'
            tool_call('ask-call', 'ask', {'i': 'Pick a stack', 'html': html})
            ask_flow = {'field': 0, 'checked': [], 'other': None, 'typing': False, 'answers': {}}
            ask_prompt()
            emit({'type': 'tool_execution_start', 'toolCallId': 'ask-call', 'toolName': 'ask',
                  'args': {'html': html}, 'intent': 'Pick a stack'})
            continue
        if text == 'ask':
            emit({'type':'extension_ui_request','id':'ask-1','method':'ask','questions':[{'id':'db','question':'Which database?','options':[{'label':'A,B'},{'label':'C'}]},{'id':'features','question':'Which features?','multi':True,'options':[{'label':'Auth'},{'label':'Search'}]}]})
            continue
        if text == 'subagents':
            subagent_task()
        if text == 'todo-state':
            # Guessed: OMP's todo state changing without a todo tool result in this turn.
            todo_phases = [{'name': 'Plan', 'tasks': [{'content': 'Check state', 'status': 'pending'}]}]
        if text == 'chunked':
            emit({'type':'available_commands_update','commands':[{'name':'large','description':'x'*1200000}]})
        if text == 'timeline-mapping':
            timeline_mapping()
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
        tool_end(call, 'read', 'contents', details={'totalLines': 1, 'rowPrefixes': True,
                 'displayContent': {'text': 'contents', 'startLine': 1, 'lineNumbers': [1]}})
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
            custom_message('async-result',
                           '<system-notice>\nBackground job bg_15 (curl) has completed. Resume '
                           'your work using the result below.\ncurl: (28) Connection timed out\n'
                           '</system-notice>',
                           {'jobs': [{'jobId': 'bg_15', 'type': 'bash', 'status': 'completed',
                                      'label': 'curl'}]})
            if text == 'background-busy':
                message('toolResult', [{'type': 'text', 'text': 'Foreground finished'}],
                        toolCallId='foreground', toolName='bash', isError=False)
            else:
                emit({'type': 'agent_start'})
        finish(settled=text != 'background')
    elif kind == 'steer':
        if current_prompt == 'steer-reject':
            # Guessed refusal text; the turn then continues and completes normally.
            emit({'type': 'response', 'id': command.get('id'), 'command': 'steer',
                  'success': False, 'error': 'Steering is unavailable api_key=fixture-secret'})
            finish()
            continue
        response(command)
        if current_prompt == 'wait-queue':
            # OMP 18.18.8 holds steering in its queue until the current message ends.
            queued_steers.append(command['message'])
            emit({'type': 'queue_update', 'steering': queued_steers, 'followUp': [], 'liveSteered': 0})
            continue
        message('user', command['message'])
        finish()
    elif kind == 'abort':
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
        end_aborted_run()
        response(command)
        if queued_steers:
            run_queued(command)
            continue
        emit({'type': 'prompt_result', 'id': prompt_id, 'agentInvoked': True, 'status': 'aborted',
              'sessionSettled': True})
        emit({'type': 'session_settled'})
    elif kind == 'extension_ui_response':
        if pending_approval is not None:
            value = command.get('value')
            approved = value in ('Approve', 'Approve bash for this session')
            text = 'ok\ndone' if approved else 'Tool call denied by user: bash'
            tool_end('approval-call', 'bash', text, error=not approved)
            pending_approval = None
            finish()
            continue
        if ask_flow is not None:
            ask_answer(command)
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

if shutdown in ('term', 'stubborn'):
    # Outlive stdin EOF so only a signal from Ait ends this process.
    while True:
        time.sleep(0.05)
