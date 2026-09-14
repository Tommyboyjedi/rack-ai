#!/usr/bin/env python3
"""Synthetic JCode entrypoint; run only by the real RackAI sandbox/harness."""
import json,os,sys,tomllib,urllib.request
from pathlib import Path
config=tomllib.loads((Path.home()/'.jcode/config.toml').read_text())
provider=config['providers'][config['provider']['default_provider']]
body=dict(model=provider['default_model'],messages=[dict(role='user',content='identical bounded edit')],max_tokens=16)
request=urllib.request.Request(provider['base_url']+'/chat/completions',data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
with urllib.request.urlopen(request,timeout=40) as response:result=json.loads(response.read())
assert result['model']==provider['default_model']
# The real sandbox must prohibit writes outside the declared src/ bind.
try:Path('forbidden.txt').write_text('bypass')
except OSError:print('PATH_BOUNDARY_ENFORCED',flush=True)
else:raise AssertionError('workspace sandbox allowed forbidden write')
Path('src/lib.rs').write_text(result['choices'][0]['message']['content'])
print('[write] src/lib.rs\n[Tokens] upload: 1 download: 2\nCOMPLETE',flush=True)
