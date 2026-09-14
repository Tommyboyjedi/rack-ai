"""Read-only hardware evidence with explicit conservative abort signals."""
from dataclasses import dataclass
from pathlib import Path
import json
import subprocess
import threading
import time

@dataclass(frozen=True)
class MonitorLimits:
    minimum_available_mib: int = 6144
    maximum_swap_growth_mib: int = 256
    maximum_gpu_celsius: int = 85
    maximum_cpu_celsius: int = 68
    maximum_read_mib_per_second: int = 256
    thrash_samples: int = 10


def snapshot():
    memory = {line.split(':')[0]: int(line.split()[1])
              for line in Path('/proc/meminfo').read_text().splitlines()}
    vm = {line.split()[0]: int(line.split()[1])
          for line in Path('/proc/vmstat').read_text().splitlines()}
    gpus = subprocess.check_output(['nvidia-smi', '--query-gpu=uuid,memory.used,'
        'memory.free,temperature.gpu,utilization.gpu,power.draw',
        '--format=csv,noheader,nounits'], text=True, timeout=5)
    temperatures = {}
    for path in Path('/sys/class/hwmon').glob('hwmon*/temp*_input'):
        label = path.with_name(path.name.replace('_input', '_label'))
        name = (path.parent/'name').read_text().strip()
        temperatures[name + ':' + (label.read_text().strip() if label.exists() else path.name) + ':' + path.parent.name] = int(path.read_text()) / 1000
    return dict(time=time.time(), available_mib=memory['MemAvailable']//1024,
        used_mib=(memory['MemTotal']-memory['MemAvailable'])//1024,
        swap_mib=(memory['SwapTotal']-memory['SwapFree'])//1024,
        pgpgin=vm['pgpgin'], pgpgout=vm['pgpgout'], pswpin=vm['pswpin'],
        pswpout=vm['pswpout'], major_faults=vm['pgmajfault'], gpus=gpus,
        temperatures=temperatures, diskstats=Path('/proc/diskstats').read_text())

class Monitor:
    def __init__(self, directory, limits=MonitorLimits()):
        self.directory = directory
        self.limits = limits
        self.finished = threading.Event()
        self.failure = None
        self.phase = 'loading'
        self.initial = snapshot()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        previous = self.initial
        pressure_samples = 0
        with (self.directory / 'hardware.jsonl').open('x') as output:
            while not self.finished.is_set():
                try:
                    value = snapshot()
                    read_rate = (value['pgpgin'] - previous['pgpgin']) / 1024 / max(.001, value['time'] - previous['time'])
                    pressure_samples = pressure_samples + 1 if self.phase == 'inference' and read_rate > self.limits.maximum_read_mib_per_second else 0
                    value['phase'] = self.phase
                    value['read_mib_per_second'] = read_rate
                    previous = value
                    if pressure_samples >= self.limits.thrash_samples:
                        self.failure = 'sustained inference page-in pressure'
                    output.write(json.dumps(value) + '\n')
                    output.flush()
                    if value['available_mib'] < self.limits.minimum_available_mib:
                        self.failure = 'host memory reserve below 6 GiB'
                    if value['swap_mib'] - self.initial['swap_mib'] > self.limits.maximum_swap_growth_mib:
                        self.failure = 'swap growth exceeds 256 MiB'
                    if any(float(line.split(',')[3]) >= self.limits.maximum_gpu_celsius
                           for line in value['gpus'].splitlines()):
                        self.failure = 'GPU temperature threshold'
                    if any(t >= self.limits.maximum_cpu_celsius for name,t in value['temperatures'].items()
                           if name.startswith('k10temp:Tdie:')):
                        self.failure = 'CPU or board temperature threshold'
                    if self.failure:
                        (self.directory / 'ABORT').write_text(self.failure)
                        return
                except Exception as error:
                    self.failure = 'monitor failed: ' + str(error)
                    (self.directory / 'ABORT').write_text(self.failure)
                    return
                self.finished.wait(2)

    def check(self):
        if self.failure:
            raise RuntimeError(self.failure)

    def close(self):
        self.finished.set()
        self.thread.join(timeout=10)
