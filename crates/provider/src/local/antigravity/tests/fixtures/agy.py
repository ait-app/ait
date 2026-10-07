#!/usr/bin/env python3
import json
import os
import signal
import sys
import time

args = sys.argv[1:]
scenario = os.environ.get('AGY_FIXTURE_SCENARIO', '')
conversation = '11111111-2222-4333-8444-555555555555'
if '--conversation' in args:
    conversation = args[args.index('--conversation') + 1]
model = args[args.index('--model') + 1] if '--model' in args else None

def emit(value):
    # SIGINT may interrupt another emission while Python's stdout buffer is locked.
    os.write(sys.stdout.fileno(), (json.dumps(value) + '\n').encode())

def log(value):
    path = os.environ.get('AGY_FIXTURE_LOG')
    if path:
        with open(path, 'a') as output:
            output.write(json.dumps(value) + '\n')

log({'args': args, 'hasEnvironment': os.environ.get('AGY_TEST_ENV') == 'test-only-value'})
if scenario == 'hung':
    time.sleep(600)
if scenario == 'malformed':
    print('{', flush=True)
    sys.exit(1)
if scenario == 'oversized':
    print('x' * (2 * 1024 * 1024), flush=True)
    sys.exit(1)
if args == ['models']:
    if scenario == 'model-failure':
        sys.exit(1)
    print('Fetching available models...')
    print('gemini-test-low\tGemini Test (Low)')
    print('claude-test-high\tClaude Test (High)')
    sys.exit(0)

init = {'cwd': os.getcwd(), 'permission_mode': 'request-review', 'tools': ['run_command']}
if model is not None:
    init['model'] = model
if scenario == 'wrong-model':
    init['model'] = 'foreign-model'
turns = 0
step_index = 0

def terminal(status, response):
    emit({'event': 'result', 'result': {'conversation_id': conversation, 'status': status,
        'response': response, 'num_turns': turns, 'usage': {'input_tokens': turns * 100,
            'output_tokens': turns * 10, 'cache_read_tokens': turns * 50}}})

def interrupted(signum, frame):
    terminal('INTERRUPTED', '')
    sys.exit(0)

signal.signal(signal.SIGINT, interrupted)
# Install cancellation handling before advertising that the process is ready.
emit({'event': 'init', 'conversation_id': conversation, 'init': init})
for line in sys.stdin:
    message = json.loads(line)
    log({'input': message})
    assert message['event'] == 'user'
    turns += 1
    if scenario == 'crash':
        sys.exit(1)
    content = message['message']['content']
    text = content if isinstance(content, str) else ''.join(block['text'] for block in content)
    if text == 'wait':
        time.sleep(600)
    if scenario == 'foreign':
        emit({'event': 'step_update', 'step_update': {'conversation_id': 'foreign',
            'state': 'DONE', 'step_type': 'agent_response', 'step_index': step_index}})
        continue
    if scenario == 'failure':
        terminal('ERROR', '')
        continue
    def step(kind, state, **fields):
        emit({'event': 'step_update', 'step_update': {'conversation_id': conversation,
            'step_index': step_index, 'state': state, 'step_type': kind, **fields}})
    step('user_input', 'DONE')
    step_index += 1
    step('tool', 'ACTIVE', tool_name='run_command', tool_info={
        'name': 'run_command', 'parameters': {'CommandLine': 'echo fixture'}})
    step('tool', 'DONE', tool_info={'output': 'fixture'})
    step_index += 1
    step('agent_response', 'ACTIVE', text_delta='Hello ')
    step('agent_response', 'DONE', text_delta='world')
    step_index += 1
    step('checkpoint', 'DONE')
    step_index += 1
    terminal('SUCCESS', 'Hello world')
