#!/usr/bin/env python3
"""Isolated ACP peer with OMP native journals and explicit turn settlement."""
import json
import base64
import hashlib
import os
from pathlib import Path
import sys
import uuid
import threading
import time

root = Path(os.environ['PI_CODING_AGENT_DIR'])
root.mkdir(parents=True, exist_ok=True)
args = sys.argv[1:]
records = []
session_id = None
file = None
active = None
pending = None
options = [
    {'id':'model','category':'model','type':'select','name':'Model','currentValue':'test/model',
     'options':[{'value':'test/model','name':'Test'},{'value':'other/model','name':'Other'}]},
    {'id':'thinking','category':'thought_level','type':'select','name':'Thinking','currentValue':'off',
     'options':[{'value':level,'name':level} for level in ['off','low','high','max']]},
    {'id':'mode','category':'mode','type':'select','name':'Mode','currentValue':'default',
     'options':[{'value':mode,'name':mode} for mode in ['default','plan','full','write','ask','ask-cached']]},
]

for choice in options[0]['options']:
    choice['_meta']={'model':{'id':'model','provider':choice['value'].split('/')[0], 'reasoning':True,
                             'contextWindow':64000, 'thinking':{'efforts':['low','high','max']}}}
for key in ('autoCompaction','autoRetry','fastMode','slowMode'):
    options.append({'id':key,'type':'boolean','name':key,'currentValue':key in ('autoCompaction','autoRetry')})
if '--provider' in args:
    options[0]['currentValue']=args[args.index('--provider')+1]+'/'+args[args.index('--model')+1]
if options[0]['currentValue']=='other/model':
    options[:]=[item for item in options if item['id'] not in ('fastMode','slowMode')]

def emit(value):
    print(json.dumps({'jsonrpc':'2.0', **value}), flush=True)

def response(command, result):
    emit({'id':command['id'],'result':result})

def update(value):
    emit({'method':'session/update','params':{'sessionId':session_id,'update':value}})

def native(event):
    emit({'method':'_omp/event','params':{'sessionId':session_id,'event':event}})

def text(value, identity='answer'):
    message={'role':'assistant','timestamp':1792886400000+len(records)*10,'content':[]}
    native({'type':'message_update','message':message,'assistantMessageEvent':{'type':'text_delta','contentIndex':0,'delta':value}})
    update({'sessionUpdate':'agent_message_chunk','messageId':identity,
            'content':{'type':'text','text':value}})

def record(role, content, **fields):
    if role == 'user' and isinstance(content, list):
        content=json.loads(json.dumps(content))
        for block in content:
            if block.get('type')=='image':
                data=base64.b64decode(block['data'])
                digest=hashlib.sha256(data).hexdigest()
                (root/'blobs').mkdir(exist_ok=True)
                (root/'blobs'/digest).write_bytes(data)
                block['data']='blob:sha256:'+digest
    entry_id = str(uuid.uuid4())
    records.append({'type':'message','id':entry_id,'parentId':records[-1]['id'] if len(records)>1 else None,
                    'timestamp':'2026-10-25T00:00:01Z','message':{
                        'role':role,'content':content,'timestamp':1792886400000+len(records)*10, **fields}})
    native({'type':'message_end','message':records[-1]['message']})
    if '--no-session' not in args:
        file.parent.mkdir(parents=True,exist_ok=True)
        file.write_text(''.join(json.dumps(row)+'\n' for row in records))

def settle(turn_id=None):
    emit({'method':'_session/settled','params':{'sessionId':session_id,
         'turnId':turn_id or active['params']['_meta']['turnId']}})

def finish(command=None, cancelled=False, reply='Hello ACP'):
    global active
    command = command or active
    if not cancelled:
        text(reply)
        record('assistant',[{'type':'text','text':reply}],provider='test',model='model',stopReason='stop')
        settle()
    response(command,{'stopReason':'cancelled' if cancelled else 'end_turn'})
    active = None

def permission():
    emit({'id':'approval','method':'session/request_permission','params':{'sessionId':session_id,
        'toolCall':{'toolCallId':'call','title':'printf hello','rawInput':{'command':'printf hello'}},
        'options':[{'optionId':'once','name':'Allow once','kind':'allow_once'},
                   {'optionId':'deny','name':'Deny','kind':'reject_once'}]}})

with (root/'calls.jsonl').open('a') as log:
    log.write(json.dumps({'args':args})+'\n')
for line in sys.stdin:
    command=json.loads(line)
    with (root/'calls.jsonl').open('a') as log:
        log.write(json.dumps(command)+'\n')
    method=command.get('method')
    if method=='initialize':
        response(command,{'protocolVersion':1,'agentCapabilities':{
            'promptCapabilities':{'image':True},'sessionCapabilities':{'resume':{},'close':{}}},
            '_meta':{'omp':{'version':0 if (root/'missing-features').exists() else 1},'steering':{'supported':True},'sessionSettled':{'version':0 if (root/'old-acp').exists() else 1}}})
    elif method in ('session/new','session/resume'):
        session_id=command['params'].get('sessionId',str(uuid.uuid4()))
        file=root/'sessions'/'workspace'/('2026-10-25_'+session_id+'.jsonl')
        records=[json.loads(row) for row in file.read_text().splitlines()] if file.exists() else [
            {'type':'session','version':3,'id':session_id,'cwd':os.getcwd(),'timestamp':'2026-10-25T00:00:00Z'}]
        response(command,{'sessionId':session_id,'configOptions':options})
        update({'sessionUpdate':'available_commands_update','availableCommands':[{'name':'review','description':'Review'}]})
    elif method=='session/set_config_option':
        option=next(item for item in options if item['id']==command['params']['configId'])
        option['currentValue']=command['params']['value']
        if option['id']=='model':
            options[:]=[item for item in options if item['id'] not in ('fastMode','slowMode')]
            if option['currentValue']=='test/model':
                for key in ('fastMode','slowMode'):
                    options.append({'id':key,'type':'boolean','name':key,'currentValue':False})
        update({'sessionUpdate':'config_option_update','configOptions':options})
        response(command,{'configOptions':options})
    elif method=='session/prompt':
        active=command
        value=''.join(block.get('text','') for block in command['params']['prompt'])
        if value != '/local':
            record('user',command['params']['prompt'])
        if value=='/local':
            settle()
            response(command,{'stopReason':'end_turn'})
            active=None
        elif value=='large':
            emit({'method':'_omp/event','params':{'sessionId':session_id,'event':{'type':'notice','text':'x'*(9*1024*1024)}}})
            finish()
        elif value=='subagents':
            native({'type':'subagent_lifecycle','payload':{'id':'child','status':'started','agent':'worker','description':'Task'}})
            native({'type':'subagent_event','payload':{'id':'child','event':{'type':'message_end','message':{'role':'assistant','timestamp':1792886400123,'content':[{'type':'text','text':'Child answer'}]}}}})
            native({'type':'subagent_lifecycle','payload':{'id':'child','status':'completed'}})
            finish()
        elif value=='usage':
            text('Usage answer')
            record('assistant',[{'type':'text','text':'Usage answer'}],provider='test',model='model',stopReason='stop',usage={'input':10,'output':4,'cacheRead':2,'totalTokens':16,'cost':{'total':0.25}})
            settle()
            response(command,{'stopReason':'end_turn'})
            active=None
        elif value=='malformed':
            print('{broken',flush=True)
            break
        elif value=='error':
            error='Upstream shard unavailable api_key=fixture-secret'
            update({'sessionUpdate':'agent_message_chunk','_meta':{'providerError':True},'content':{'type':'text','text':error}})
            record('assistant',[],provider='test',model='model',stopReason='error',errorMessage=error)
            response(command,{'stopReason':'end_turn','_meta':{'error':error}})
            active=None
        elif value=='question':
            emit({'method':'elicitation/create','id':'question','params':{'mode':'form','message':'Choose',
                'requestedSchema':{'type':'object','properties':{'values':{'type':'array','uniqueItems':True,
                    'items':{'type':'string','enum':['A, B','C']}}},'required':['values']}}})
        elif value=='permission':
            permission()
        elif value=='background':
            update({'sessionUpdate':'tool_call','toolCallId':'bg','title':'Bash','status':'in_progress','rawInput':{'command':'sleep'}})
            update({'sessionUpdate':'tool_call_update','toolCallId':'bg','status':'completed','rawOutput':'Backgrounded'})
            response(command,{'stopReason':'end_turn'})
            settle('previous-turn')
            text('Waiting for background result','initial')
            record('assistant',[{'type':'text','text':'Waiting for background result'}],provider='test',model='model',stopReason='stop')
            def wake(command=command):
                trigger=root/('wake-'+command['params']['_meta']['turnId'])
                while not trigger.exists():
                    time.sleep(0.002)
                update({'sessionUpdate':'tool_call_update','toolCallId':'bg','status':'in_progress','rawOutput':'late progress'})
                text('Background continued','continuation')
                record('assistant',[{'type':'text','text':'Background continued'}],provider='test',model='model',stopReason='stop')
                settle(command['params']['_meta']['turnId'])
            threading.Thread(target=wake,daemon=True).start()
        elif value=='wait':
            pass
        else:
            finish()
    elif method=='_session/steering':
        value=''.join(block.get('text','') for block in command['params']['prompt'])
        if value=='race':
            permission()
        record('user',command['params']['prompt'])
        response(command,{'outcome':'injected'})
        if value!='race':
            update({'sessionUpdate':'tool_call_update','toolCallId':'bg','status':'in_progress','rawOutput':'late progress'})
            finish(reply='Background continued')
    elif method=='session/cancel':
        finish(cancelled=True)
    elif method=='session/close':
        response(command,{})
    elif method is None and 'id' in command:
        finish()
