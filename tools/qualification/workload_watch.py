"""Bounded registration, pinned memory evidence and explicit retirement lifetime."""
from dataclasses import dataclass
import threading
import time
from workload_probe import CgroupProbe, Workload

@dataclass(frozen=True)
class WorkloadLimits:
    memory_max_bytes: int | None = None
    registration_seconds: float = 10


def memory_failure(sample, limits):
    if sample['status'] not in ('active','retiring'):
        return None
    if (sample['memory_max'] <= 0 or limits.memory_max_bytes is None
            or sample['memory_max'] != limits.memory_max_bytes):
        return 'managed memory limit missing or changed'
    if sample['swap_max'] != 0 or sample['swap_current'] != 0:
        return 'managed swap prohibited or limit changed'
    if sample['memory_current'] < 0 or sample['memory_current'] > sample['memory_max']:
        return 'managed memory bound exceeded'
    if any(sample['events'].get(key,0) for key in ('oom','oom_kill','oom_group_kill')):
        return 'managed cgroup OOM evidence'
    if sample.get('swap_events',{}).get('fail',0):
        return 'managed swap allocation failure'
    return None

class WorkloadWatch:
    def __init__(self, limits=WorkloadLimits(), probe=None):
        if limits.registration_seconds <= 0 or (limits.memory_max_bytes is not None and limits.memory_max_bytes <= 0):
            raise ValueError('invalid workload memory policy')
        self.limits = limits
        self.probe = probe or CgroupProbe()
        self.lock = threading.Lock()
        self.target = None
        self.scope = None
        self.expected_since = None
        self.retiring = False
        self.fingerprint = None

    def observe(self, demand):
        with self.lock:
            scope = (demand['id'],demand['generation'])
            if self.scope is not None and self.scope != scope:
                raise ValueError('managed reservation identity changed')
            self.scope = scope
            if demand['effect_started'] and self.expected_since is None:
                self.expected_since = time.monotonic()
            if demand.get('process'):
                target = Workload.from_record(demand)
                if self.target is not None and target != self.target:
                    raise ValueError('managed workload identity changed')
                self.target = target

    def sample(self):
        with self.lock:
            if self.target is None:
                if self.expected_since is None:
                    return dict(status='not_expected')
                if time.monotonic()-self.expected_since > self.limits.registration_seconds:
                    raise RuntimeError('managed workload registration evidence unavailable')
                return dict(status='awaiting_registration')
            sample = self.probe.read(self.target, self.retiring)
            if self.fingerprint is not None and sample.get('cgroup',self.fingerprint[0])!=self.fingerprint[0]:
                raise RuntimeError('managed cgroup replaced')
            if 'cgroup_identity' in sample:
                fingerprint = (sample['cgroup'],tuple(sample['cgroup_identity']))
                if self.fingerprint is not None and self.fingerprint != fingerprint:
                    raise RuntimeError('managed cgroup replaced')
                self.fingerprint = fingerprint
            return sample

    def retire(self):
        with self.lock:
            self.retiring = True

    def clear(self, demand):
        with self.lock:
            if (demand['state'] not in ('cancelled','released','expired')
                    or demand['process'] is not None or demand['effect_started']
                    or not demand['released']):
                raise ValueError('managed cleanup is not confirmed')
            if self.scope and (demand['id'],demand['generation']) != self.scope:
                raise ValueError('cleanup belongs to another managed workload')
            self.target = None
            self.scope = None
            self.expected_since = None
            self.retiring = False
            self.fingerprint = None
