"""Read CPU-package energy only; core subzones overlap and are never added."""
from pathlib import Path
import time
from model import Counter, Reading

class Sensors:
    def __init__(self, nvml):
        self.nvml = nvml
        self.boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()

    def read(self):
        issues, packages, seen = [], {}, set()
        packages_valid = True
        for directory in Path("/sys/class/powercap").glob("*"):
            if not directory.is_dir() or directory.resolve() in seen:
                continue
            seen.add(directory.resolve())
            if not (directory / "name").is_file() or not (directory / "energy_uj").exists():
                continue
            try:
                name = (directory / "name").read_text().strip()
                if name.startswith("package-"):
                    packages[name] = Counter(float((directory / "energy_uj").read_text()) / 1e6,
                                             float((directory / "max_energy_range_uj").read_text()) / 1e6)
            except OSError:
                packages_valid = False
        try:
            gpus = self.nvml.read()
        except (RuntimeError, OSError):
            gpus = []
            issues.append("GPU telemetry unavailable")
        if not packages_valid:
            packages = {}
        if not packages:
            issues.append("CPU package energy unavailable")
        return Reading(time.time(), time.monotonic(), self.boot, gpus, packages, sorted(set(issues)))
