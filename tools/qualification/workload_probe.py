"""Read only the cgroup and process pinned by an authenticated managed record."""
from dataclasses import dataclass
from pathlib import Path
import subprocess

@dataclass(frozen=True)
class Workload:
    reservation: str
    generation: str
    unit: str
    invocation: str
    pid: int
    start: str
    boot: str

    @classmethod
    def from_record(cls, demand):
        process = demand['process']
        generation = demand['generation']
        if (process['activation'] != generation or not generation.isalnum()
                or process['unit'] != f'rack-runtime-{generation}.service'
                or not process['invocation'] or process['pid'] <= 0):
            raise ValueError('ambiguous managed workload identity')
        return cls(demand['id'], generation, process['unit'], process['invocation'],
                   process['pid'], process['start'], process['boot'].strip())

@dataclass(frozen=True)
class ProbeRoots:
    cgroup: Path = Path('/sys/fs/cgroup')
    proc: Path = Path('/proc')


@dataclass(frozen=True)
class UnitObservation:
    properties: dict
    retiring: bool


def unit_properties(unit):
    text = subprocess.check_output(['systemctl','--user','show',unit,
        '--property=Id,InvocationID,MainPID,ControlGroup,ActiveState,Job'],
        text=True, timeout=2)
    return dict(line.split('=',1) for line in text.splitlines() if '=' in line)


def counters(path):
    values = dict(line.split() for line in path.read_text().splitlines())
    result = {key:int(value) for key,value in values.items()}
    if any(value < 0 for value in result.values()):
        raise ValueError('negative memory counter')
    return result

class CgroupProbe:
    def __init__(self, roots=ProbeRoots()):
        self.roots = roots

    def read(self, target, retiring=False):
        unit = checked_unit(target)
        if retiring and ending(unit) and not unit['ControlGroup']:
            return dict(status='tearing_down', unit=unit)
        if (unit['InvocationID'] != target.invocation
                or (unit['MainPID'] != str(target.pid) and not (retiring and ending(unit)))):
            raise ValueError('managed workload process unavailable or changed')
        try:
            return self.active(target, UnitObservation(unit, retiring))
        except FileNotFoundError:
            # Only explicit cancellation plus a verified ending unit explains
            # disappearing files. Permission failures and live-unit gaps fail.
            after = checked_unit(target)
            if retiring and ending(after) and not after['ControlGroup']:
                return dict(status='tearing_down', unit=after)
            raise

    def active(self, target, observation):
        unit, retiring = observation.properties, observation.retiring
        relative = Path(unit['ControlGroup'])
        if not relative.is_absolute() or '..' in relative.parts or relative == Path('/'):
            raise ValueError('ambiguous managed cgroup path')
        directory = (self.roots.cgroup/str(relative).lstrip('/')).resolve()
        directory.relative_to(self.roots.cgroup.resolve())
        identity = directory.stat()
        values = {name:int((directory/file).read_text()) for name,file in (
            ('memory_current','memory.current'),('memory_max','memory.max'),
            ('swap_current','memory.swap.current'),('swap_max','memory.swap.max'))}
        values['events'] = counters(directory/'memory.events')
        if not {'low','high','max','oom','oom_kill'} <= values['events'].keys():
            raise ValueError('incomplete managed memory events')
        values['swap_events'] = counters(directory/'memory.swap.events')
        if not {'high','max','fail'} <= values['swap_events'].keys():
            raise ValueError('incomplete managed swap events')
        try:
            values['pressure'] = (directory/'memory.pressure').read_text()
        except FileNotFoundError:
            values['pressure'] = None  # PSI may be disabled; required counters are not optional.
        if not (retiring and ending(unit)):
            self.verify_process(target, str(relative))
        after = directory.stat()
        observed = checked_unit(target)
        if ((identity.st_dev,identity.st_ino) != (after.st_dev,after.st_ino)
                or (observed != unit and not (retiring and ending(observed)))):
            raise ValueError('managed cgroup changed during observation')
        values.update(status='retiring' if retiring and ending(unit) else 'active', cgroup=str(relative),
                      cgroup_identity=[identity.st_dev,identity.st_ino],
                      invocation=target.invocation,pid=target.pid)
        return values

    def verify_process(self, target, cgroup):
        root = self.roots.proc/str(target.pid)
        fields = (root/'stat').read_text().rsplit(') ',1)[1].split()
        if fields[0]=='Z' or fields[19]!=target.start:
            raise ValueError('managed process generation changed')
        if (self.roots.proc/'sys/kernel/random/boot_id').read_text().strip()!=target.boot:
            raise ValueError('managed process boot changed')
        if f'0::{cgroup}' not in (root/'cgroup').read_text().splitlines():
            raise ValueError('managed process cgroup membership changed')
        expected = f'RACK_RUNTIME_ACTIVATION={target.generation}'.encode()
        if expected not in (root/'environ').read_bytes().split(b'\0'):
            raise ValueError('managed process activation changed')


def checked_unit(target):
    unit = unit_properties(target.unit)
    required = ('Id','InvocationID','MainPID','ControlGroup','ActiveState','Job')
    if any(key not in unit for key in required):
        raise ValueError('incomplete managed unit observation')
    if unit['Id'] != target.unit or unit['InvocationID'] not in ('',target.invocation):
        raise ValueError('managed unit identity changed')
    if unit['MainPID'] not in ('0',str(target.pid)):
        raise ValueError('managed process identity changed')
    if not unit['InvocationID'] and not (
            unit['MainPID']=='0' and unit['ActiveState'] in ('inactive','failed')):
        raise ValueError('managed unit ownership uncertain')
    return unit


def ending(unit):
    return unit['MainPID']=='0' and unit['ActiveState'] in ('deactivating','inactive','failed')
