import hashlib
import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VERSION = 'rack-ai/runtime/v1'

_fixture_ports = set()
def port():
    while True:
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            selected = s.getsockname()[1]
        if selected not in _fixture_ports:
            _fixture_ports.add(selected)
            return selected

class Rack:
    def __init__(self, directory, configure=None, environment=None):
        self.root = Path(directory)
        self.root.mkdir(parents=True, exist_ok=True)
        self.address = f'127.0.0.1:{port()}'
        self.events = self.root/'events.jsonl'
        self.environment = environment
        self.config = self.make_config()
        if configure:
            configure(self.config)
        self.start()

    def make_config(self):
        profiles = []
        executable = str(Path(sys.executable).resolve())
        tags = [('local-primary',['gpu-4060ti']),('local-coder',['gpu-2060']),
                ('local-fun-chat',['gpu-4060ti']),('comfyui',['gpu-4080-super']),
                ('big-brain',['gpu-4060ti','gpu-2060','gpu-4080-super'])]
        for tag, resources in tags:
            listen = port()
            profiles.append(dict(tag=tag, version='fixture-v1', model=tag, backend='vllm', driver='fixture',
                qualified=True, evidence=['synthetic-process-only'], capabilities=['reasoning'], context_tokens=4096,
                max_input_tokens=3968, max_output_tokens=128, resources=resources, device_mib={r:16 for r in resources},host_mib=32,cpu_percent=100,
                endpoint=f'http://127.0.0.1:{listen}', executable=executable,
                executable_sha256=hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
                args=[str(ROOT/'tests/runtime/backend.py'), str(listen), tag, str(self.events),str(self.root/(tag+'.control.json'))],
                artifact=None, artifact_sha256=None, startup_seconds=3,drain_seconds=3,stop_seconds=2,inference_seconds=5))
        return dict(schema=VERSION,listen=self.address,authority_root=str(self.root/'authority'),fixture_mode=True,
            max_ttl_seconds=60,host_capacity_mib=1024,
            devices={r:dict(uuid=r,capacity_mib=1024) for r in tags[-1][1]}, profiles=profiles,
            sources=[dict(source=s,token_sha256=hashlib.sha256(s.encode()).hexdigest(),
                qualification=s=='other') for s in ['athba','comfy','cb','other']])

    def start(self):
        path = self.root/'config.json'
        path.write_text(json.dumps(self.config))
        os.chmod(path,0o600)
        self.log = open(self.root/'receiver.log','a')
        self.process = subprocess.Popen([str(ROOT/'target/debug/rack_ai_runtime'), str(path)],stdout=self.log,stderr=self.log,env=self.environment)
        for _ in range(100):
            if self.process.poll() is not None:
                raise AssertionError((self.root/'receiver.log').read_text())
            try:
                self.call('athba','discover')
                return
            except OSError:
                time.sleep(.03)
        raise AssertionError('receiver startup timeout')

    def call(self, source, operation, status=200, **fields):
        req = urllib.request.Request(f'http://{self.address}/runtime/v1',
            data=json.dumps(dict(operation=operation,**fields)).encode(),
            headers={'Authorization':f'Bearer {source}','Content-Type':'application/json'})
        try:
            response = urllib.request.urlopen(req,timeout=4)
        except urllib.error.HTTPError as error:
            response = error
        value = json.loads(response.read())
        assert response.status == status, (response.status,value)
        return value.get('result',value)

    def acquire(self, source, tag, priority, identity=None, **changes):
        request = dict(schema=VERSION,source_system=source,work_id='work',acquisition_id=identity or os.urandom(8).hex(),
            tag=tag,priority=priority,capabilities=next(p['capabilities'] for p in self.config['profiles'] if p['tag']==tag),context_tokens=4096,ttl_seconds=60,qualification=False)
        request.update(changes)
        return self.call(source,'acquire',request=request)

    def inspect(self, d):
        return self.call(d['owner'],'inspect',reservation_id=d['id'])

    def wait(self, d, state='ready', seconds=10):
        deadline = time.monotonic()+seconds
        while time.monotonic()<deadline:
            d = self.inspect(d)
            if d['state']==state:
                return d
            if d['state']=='recovery_required' and state!='recovery_required':
                if d.get('released') and not d.get('recovery_error'):
                    time.sleep(.03); continue
                raise AssertionError(d)
            time.sleep(.03)
        raise AssertionError(d)

    def infer(self, d, identity=None, **changes):
        request = dict(schema=VERSION,submission_id=identity or os.urandom(8).hex(),reservation_id=d['id'],
            generation=d['generation'],profile_hash=d['profile_hash'],prompt='hello',max_tokens=16,timeout_seconds=5)
        request.update(changes)
        return self.call(d['owner'],'infer',request=request)

    def result(self, i, state='completed'):
        deadline = time.monotonic()+8
        while time.monotonic()<deadline:
            i = self.call(i['owner'],'result',invocation_id=i['id'])
            if i['state']==state:
                return i
            time.sleep(.03)
        raise AssertionError(i)

    def release(self, d, kind='release'):
        d = self.inspect(d)
        return self.call(d['owner'],'control',reservation_id=d['id'],request=dict(generation=d['generation'],action=dict(kind=kind)))

    def counts(self, kind):
        records = [json.loads(l) for l in self.events.read_text().splitlines()] if self.events.exists() else []
        return {m:sum(r['kind']==kind and r['model']==m for r in records) for m in [p['model'] for p in self.config['profiles']]}

    def controls(self, tag, **fields):
        (self.root/(tag+'.control.json')).write_text(json.dumps(fields))

    def close(self):
        self.process.terminate()
        self.process.wait(timeout=5)
        self.log.close()
        # Test-owned fixture PIDs only, with activation identity checked against /proc.
        if self.events.exists():
            for record in [json.loads(l) for l in self.events.read_text().splitlines() if json.loads(l)['kind']=='start']:
                try:
                    env = Path(f'/proc/{record["pid"]}/environ').read_bytes()
                    if f'RACK_RUNTIME_ACTIVATION={record["activation"]}'.encode() in env.split(b'\0'):
                        os.kill(record['pid'],9)
                except (FileNotFoundError,ProcessLookupError):
                    pass
