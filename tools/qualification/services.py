"""Restore exact pre-existing RackAI services; never recreate model containers."""
from pathlib import Path
import json
import subprocess
import time
import urllib.request
from client import save

UNITS = ('rack-ai-campaign-supervisor.service', 'rack-ai-media-pr24.service')
MODELS = ('vllm-primary', 'vllm-coder')
COMFY = 'rack-ai-comfyui-pr24.service'
ROOT = Path('/srv/rack-ai/state/resources')


def command(argv):
    return subprocess.check_output(argv, text=True, timeout=180).strip()


def unit_state(name):
    return command(['systemctl', '--user', 'show', name, '--property=ActiveState', '--value'])


def metric_idle(port):
    with urllib.request.urlopen(f'http://127.0.0.1:{port}/metrics', timeout=5) as response:
        lines = response.read().decode().splitlines()
    samples = [float(line.rsplit(' ', 1)[1]) for line in lines
               if line.startswith(('vllm:num_requests_running{', 'vllm:num_requests_waiting{'))]
    if len(samples) != 2 or any(samples):
        raise RuntimeError('existing endpoint is not proven idle: ' + str(port))


def gpu_empty():
    value = command(['nvidia-smi', '--query-compute-apps=gpu_uuid,pid', '--format=csv,noheader'])
    if value:
        raise RuntimeError('GPU processes remain: ' + value)


def validate_mounts(containers):
    for name, container in containers.items():
        for mount in container['Mounts']:
            if mount['Type'] == 'bind' and not Path(mount['Source']).exists():
                raise RuntimeError('restoration blocked: missing bind source for '
                                   + name + ': ' + mount['Source'])


def mount_configuration(mounts):
    required={'Type','Source','Destination','Mode','RW','Propagation'}
    if not isinstance(mounts,list):
        raise ValueError('invalid Docker Mounts evidence')
    normalized=[]
    for mount in mounts:
        if (not isinstance(mount,dict) or not required<=mount.keys()
                or type(mount['RW']) is not bool
                or any(not isinstance(mount[key],str) for key in required-{'RW'})):
            raise ValueError('incomplete Docker Mounts evidence')
        # Keep every field and duplicate count; only list ordering is irrelevant.
        normalized.append(json.dumps(mount,sort_keys=True,separators=(',',':')))
    return sorted(normalized)

class Services:
    def __init__(self, directory):
        self.directory = directory
        self.containers = {name: json.loads(command(['docker','inspect',name]))[0] for name in MODELS}
        self.units = {name: unit_state(name) for name in UNITS}
        self.comfy = unit_state(COMFY)
        self.quiesced = False
        self.changed_units = []
        self.changed_models = []
        save(directory / 'original-containers.json', self.containers)
        save(directory / 'original-units.json', dict(units=self.units, comfy=self.comfy))

    def stop(self):
        validate_mounts(self.containers)
        if self.comfy != 'inactive':
            raise RuntimeError('ComfyUI state changed; replan required')
        if any(p.name != '.gitkeep' for p in (ROOT/'leases').iterdir()):
            raise RuntimeError('legacy leases must be reconciled by their owners')
        for port in (8017,8018):
            metric_idle(port)
        for name in UNITS:
            if self.units[name] == 'active':
                self.changed_units.append(name)
                command(['systemctl','--user','stop',name])
            elif self.units[name] != 'inactive':
                raise RuntimeError('unexpected unit state')
        for port in (8017,8018):
            metric_idle(port)
        for name, original in self.containers.items():
            if not original['State']['Running']:
                raise RuntimeError('model initial state changed; replan required')
            current = json.loads(command(['docker','inspect',name]))[0]
            if current['Id'] != original['Id']:
                raise RuntimeError('container identity changed')
            self.changed_models.append(name)
            command(['docker','stop','--time','60', original['Id']])
        gpu_empty()
        self.quiesced = True
        save(self.directory/'quiesced.json', dict(time=time.time(), gpu_processes=[]))

    def restore(self):
        if self.quiesced:
            gpu_empty()
        if (ROOT/'managed.json').exists():
            state = json.loads((ROOT/'managed.json').read_text())
            if state['claims'] or any(d['process'] for d in state['data']['demands'].values()):
                raise RuntimeError('managed cleanup must be proven before restoration')
        for name in self.changed_models:
            original = self.containers[name]
            current = json.loads(command(['docker','inspect',original['Id']]))[0]
            for field in ('Id','Image','Config','HostConfig','Mounts'):
                actual, expected = current[field], original[field]
                if field=='Mounts':
                    actual, expected = mount_configuration(actual), mount_configuration(expected)
                if actual != expected:
                    raise RuntimeError('container configuration changed: ' + name + ' ' + field)
            command(['docker','start',original['Id']])
        deadline = time.monotonic() + 600
        for name in self.changed_models:
            while time.monotonic() < deadline:
                state = json.loads(command(['docker','inspect',self.containers[name]['Id']]))[0]['State']
                if state.get('Health',{}).get('Status') == 'healthy':
                    break
                time.sleep(3)
            else:
                raise TimeoutError('original model failed health restoration: ' + name)
        for name in self.changed_units:
            command(['systemctl','--user','start',name])
        if unit_state(COMFY) != self.comfy or any(unit_state(n) != s for n,s in self.units.items()):
            raise RuntimeError('original RackAI unit state not restored')
        save(self.directory/'services-restored.json', dict(time=time.time(),
             containers={n:v['Id'] for n,v in self.containers.items()}, units=self.units, comfy=self.comfy))
