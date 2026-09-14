"""Acquire and release qualification work through the canonical RackAI receiver."""
from pathlib import Path
import json
import time
import subprocess
import urllib.request
import uuid
from client import save

class Lifecycle:
    def __init__(self, client, directory):
        self.client = client
        self.directory = directory
        self.demand = None
        self.monitor = None

    def acquire(self, monitor):
        request = dict(schema='rack-ai/runtime/v1', source_system='gptoss-qualification',
            work_id='single-model-benchmark', acquisition_id=str(uuid.uuid4()),
            tag='big-brain', priority='medium', capabilities=['reasoning','coding'],
            context_tokens=4096, ttl_seconds=1800, qualification=True)
        save(self.directory / 'acquisition-request.json', request)
        self.monitor = monitor
        monitor.phase = 'loading'
        start = time.monotonic()
        self.demand = self.client.call(dict(operation='acquire', request=request))
        save(self.directory / 'acquisition-response.json', self.demand)
        monitor.workload.observe(self.demand)
        deadline = start + 780
        phases = []
        while time.monotonic() < deadline:
            monitor.check()
            self.demand = self.client.call(dict(operation='inspect', reservation_id=self.demand['id']))
            monitor.workload.observe(self.demand)
            phase = (self.demand['state'], self.demand['preflight_done'], self.demand['process'] is not None)
            if not phases or phases[-1]['phase'] != phase:
                phases.append(dict(phase=phase, seconds=time.monotonic()-start))
                print('activation', phases[-1], flush=True)
            if self.demand['state'] == 'ready':
                save(self.directory / 'ready.json', self.demand)
                save(self.directory / 'startup.json', dict(seconds=time.monotonic()-start, phases=phases))
                monitor.phase = 'inference'
                return self.demand
            if self.demand['state'] in ('denied','recovery_required','released','expired'):
                raise RuntimeError('activation failed: ' + str(self.demand.get('reason')))
            time.sleep(1)
        raise TimeoutError('qualification startup deadline')

    def release(self):
        if not self.demand:
            request = json.loads((self.directory / 'acquisition-request.json').read_text())
            self.demand = self.client.call(dict(operation='acquire', request=request))
        current = self.client.call(dict(operation='inspect', reservation_id=self.demand['id']))
        process = current.get('process')
        if process and process.get('unit'):
            unit = process['unit']
            for filename, argv in (
                ('backend-journal.txt', ['journalctl','--user','-u',unit,'--no-pager','-o','short-iso']),
                ('runtime-limits.txt', ['systemctl','--user','show',unit,
                    '--property=MemoryCurrent,MemoryPeak,MemoryMax,MemorySwapMax,CPUUsageNSec,CPUQuotaPerSecUSec,ControlGroup,InvocationID'])):
                result = subprocess.run(argv, capture_output=True, text=True, timeout=10)
                (self.directory/filename).write_text(result.stdout + result.stderr)
            try:
                with urllib.request.urlopen('http://127.0.0.1:8096/metrics', timeout=5) as response:
                    (self.directory/'backend-metrics.txt').write_bytes(response.read())
            except OSError as error:
                (self.directory/'backend-metrics-error.txt').write_text(str(error))
        if self.monitor:
            self.monitor.workload.retire()
        self.client.call(dict(operation='control', reservation_id=current['id'],
            request=dict(generation=current['generation'], action=dict(kind='cancel'))))
        deadline = time.monotonic() + 330
        while time.monotonic() < deadline:
            current = self.client.call(dict(operation='inspect', reservation_id=current['id']))
            if current['state'] in ('released','cancelled','expired') and current['process'] is None:
                if self.monitor:
                    self.monitor.workload.clear(current)
                save(self.directory / 'released.json', current)
                return
            if current['state'] == 'recovery_required':
                raise RuntimeError('cleanup unproven: ' + str(current.get('reason')))
            time.sleep(1)
        raise TimeoutError('qualification cleanup deadline')
