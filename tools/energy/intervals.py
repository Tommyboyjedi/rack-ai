"""Counter deltas, restart/gap checks, and the explicitly assumed total model."""
import math
from model import Batch, Interval

JOULES_PER_KWH = 3_600_000
MAX_VALID_GAP = 90
MAX_SENSOR_WATTS = 3000

class Intervals:
    def __init__(self, config):
        self.config = config

    def between(self, pair):
        previous, current = pair
        batch = Batch(current, issues=list(current.issues))
        if previous is None:
            batch.issues.append("Gathering the first energy interval")
            return batch
        duration = current.monotonic - previous.monotonic
        wall = current.at - previous.at
        if previous.boot != current.boot or not 0 < duration <= MAX_VALID_GAP or abs(wall - duration) > 2:
            batch.issues.append("Collection gap or clock change; missing energy not invented")
            return batch
        old = {g.id: g for g in previous.gpus}
        gpu_energy = {}
        for gpu in current.gpus:
            prior = old.get(gpu.id)
            if prior and prior.joules is not None and gpu.joules is not None:
                delta = gpu.joules - prior.joules
                if self.valid(delta, duration):
                    gpu_energy[gpu.id] = delta
                    batch.intervals.append(Interval(gpu.id, gpu.name, "measured_device",
                                                     previous.at, current.at, delta / JOULES_PER_KWH))
                else:
                    batch.issues.append("GPU counter reset or invalid delta; interval omitted")
        cpu = self.cpu_delta((previous, current))
        if cpu is not None and self.valid(cpu, duration):
            batch.intervals.append(Interval("cpu-package", "CPU package", "counter_derived",
                                             previous.at, current.at, cpu / JOULES_PER_KWH))
        else:
            cpu = None
            batch.issues.append("CPU interval unavailable")
        complete = len(gpu_energy) == self.config.expected_gpus and len(current.gpus) == self.config.expected_gpus
        if complete and cpu is not None:
            total = (sum(gpu_energy.values()) + cpu + self.config.other_components_watts * duration) / self.config.psu_efficiency
            batch.intervals.append(Interval("estimated-total", "Estimated rack total", "estimated",
                                             previous.at, current.at, total / JOULES_PER_KWH))
        else:
            batch.issues.append("Estimated total unavailable: incomplete component readings")
        return batch

    @staticmethod
    def valid(joules, seconds):
        return math.isfinite(joules) and 0 <= joules <= seconds * MAX_SENSOR_WATTS

    @staticmethod
    def cpu_delta(pair):
        previous, current = pair
        if not current.packages or current.packages.keys() != previous.packages.keys():
            return None
        total = 0
        duration = current.monotonic - previous.monotonic
        for name, now in current.packages.items():
            old = previous.packages[name]
            if now.modulus and (duration * 1000 >= now.modulus or not 0 <= now.joules < now.modulus or not 0 <= old.joules < now.modulus):
                return None
            delta = now.joules - old.joules
            if delta < 0:
                if not now.modulus or now.modulus != old.modulus:
                    return None
                delta += now.modulus
            total += delta
        return total
