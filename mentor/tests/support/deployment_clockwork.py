#!/usr/bin/env python3
"""Clockwork protocol double. It never launches a worker or sends email."""
import json, os, sys
from pathlib import Path
home = Path(os.environ['HOME'])
path = home / 'clockwork-fixture.json'
state = json.loads(path.read_text()) if path.exists() else {'binding': None, 'definitions': {}}
args = sys.argv[2:]
def finish(data):
    path.write_text(json.dumps(state))
    print(json.dumps({'ok': True, 'data': data}))
    sys.exit(0)
def fail(code):
    print(json.dumps({'ok': False, 'error': {'code': code, 'message': 'fixture'}}), file=sys.stderr)
    sys.exit(1)
if args[:2] == ['binding','show']:
    if state['binding'] is None: fail('binding_not_found')
    finish(state['binding'])
if args[:2] == ['definition','register']:
    manifest, table = {}, None
    for line in Path(args[2]).read_text().splitlines():
        line = line.strip()
        if not line: continue
        if line.startswith('['):
            table = line[1:-1]
            manifest[table] = {}
        else:
            key, value = line.split('=',1)
            (manifest if table is None else manifest[table])[key.strip()] = json.loads(value.strip())
    digest = Path(args[2]).stem
    definition = {'digest':digest,'key':manifest['key'],'manifest':manifest,'registered_at':1}
    state['definitions'][digest] = definition
    finish(definition)
if args[:2] == ['definition','show']:
    finish(state['definitions'][args[2]])
if args[:2] in (['binding','disable'],['binding','switch']):
    if args[1] == 'switch' and (home/'fail-switch').exists():
        (home/'fail-switch').unlink()
        fail('fixture_transition_failure')
    binding = state['binding'] or {'key':args[2],'definition_digest':None,'enabled':False,'halted_incident':None,'failure_policy_active':True,'updated_at':1}
    if args[1] == 'switch':
        binding['definition_digest'] = args[3]
    elif len(args) > 3:
        binding['definition_digest'] = args[4]
    binding['enabled'] = args[1] == 'switch'
    binding['updated_at'] += 1
    state['binding'] = binding
    finish(binding)
if args[:2] == ['incident','feed']:
    finish({'items':[],'next_cursor':0,'has_more':False})
if args[:2] == ['notification','emt']:
    state['routing'] = args[2:]
    finish({'configured':True})
fail('unexpected_fixture_command')
