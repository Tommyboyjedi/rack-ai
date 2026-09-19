#!/usr/bin/env python3
"""Synthetic JCode entrypoint; run only by the real RackAI sandbox/harness."""
import json,os,sys,time,tomllib,urllib.request
from pathlib import Path
config=tomllib.loads((Path.home()/'.jcode/config.toml').read_text())
provider=config['providers'][config['provider']['default_provider']]
body=dict(model=provider['default_model'],messages=[dict(role='user',content='identical bounded edit')],max_tokens=16,stream=True,stream_options=dict(include_usage=True))
control=Path('src/harness-control.json')
settings=json.loads(control.read_text()) if control.exists() else {}
if settings:time.sleep(settings.get('pre_submit_delay',0))
def chat(body):
    request=urllib.request.Request(provider['base_url']+'/chat/completions',data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(request,timeout=40) as response:
        raw=response.read().decode()
        if not body.get('stream'):
            return json.loads(raw)
    chunks=[]
    for line in raw.splitlines():
        if not line.startswith('data: '):
            continue
        payload=line.removeprefix('data: ')
        if payload=='[DONE]':
            continue
        event=json.loads(payload)
        for choice in event.get('choices',[]):
            chunks.append((choice.get('delta') or {}).get('content',''))
            chunks.append(((choice.get('message') or {}).get('content','')))
    return dict(model=provider['default_model'],choices=[dict(message=dict(content=''.join(chunks)))])

result=chat(body)
assert result['model']==provider['default_model']
if settings.get('three_turns') and provider['default_model']=='local-primary':
    Path('src/first-turn').write_text('ready')
    deadline=time.monotonic()+30
    while not Path('src/continue-turn').exists():
        if time.monotonic()>deadline:raise AssertionError('test continuation missing')
        time.sleep(.04)
    for turn in [2,3]:
        body['messages']=[dict(role='user',content=f'reserved turn {turn}')]
        result=chat(body)

# The real sandbox must prohibit writes outside the declared src/ bind.
try:Path('forbidden.txt').write_text('bypass')
except OSError:print('PATH_BOUNDARY_ENFORCED',flush=True)
else:raise AssertionError('workspace sandbox allowed forbidden write')
Path('src/lib.rs').write_text(result['choices'][0]['message']['content'])
print('[write] src/lib.rs\n[Tokens] upload: 1 download: 2\nCOMPLETE',flush=True)
