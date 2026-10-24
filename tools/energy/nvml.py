"""Read-only NVIDIA telemetry; no runtime/model/control operations."""
import ctypes
from model import Gpu

class Nvml:
    def __init__(self):
        self.lib = ctypes.CDLL("libnvidia-ml.so.1")
        self.call("nvmlInit_v2", ())
        self.lib.nvmlDeviceGetCount_v2.argtypes = [ctypes.POINTER(ctypes.c_uint)]
        self.lib.nvmlDeviceGetHandleByIndex_v2.argtypes = [ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)]
        self.lib.nvmlDeviceGetTotalEnergyConsumption.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulonglong)]
        self.lib.nvmlDeviceGetPowerUsage.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint)]
        for name in ["nvmlDeviceGetName", "nvmlDeviceGetUUID"]:
            getattr(self.lib, name).argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint]

    def call(self, name, args):
        if getattr(self.lib, name)(*args) != 0:
            raise RuntimeError("GPU telemetry unavailable")

    def read(self):
        count = ctypes.c_uint()
        self.call("nvmlDeviceGetCount_v2", (ctypes.byref(count),))
        if count.value > 16:
            raise RuntimeError("GPU telemetry exceeds device bound")
        result = []
        for index in range(count.value):
            handle = ctypes.c_void_p()
            self.call("nvmlDeviceGetHandleByIndex_v2", (index, ctypes.byref(handle)))
            name, identity = ctypes.create_string_buffer(128), ctypes.create_string_buffer(128)
            self.call("nvmlDeviceGetName", (handle, name, 128))
            self.call("nvmlDeviceGetUUID", (handle, identity, 128))
            power, energy = ctypes.c_uint(), ctypes.c_ulonglong()
            power_ok = self.lib.nvmlDeviceGetPowerUsage(handle, ctypes.byref(power)) == 0
            energy_ok = self.lib.nvmlDeviceGetTotalEnergyConsumption(handle, ctypes.byref(energy)) == 0
            result.append(Gpu(identity.value.decode(), name.value.decode(),
                              power.value / 1000 if power_ok else None,
                              energy.value / 1000 if energy_ok else None))
        return result
