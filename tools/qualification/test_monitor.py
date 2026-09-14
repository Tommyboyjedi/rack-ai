import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from monitor import Monitor

class MonitorTests(unittest.TestCase):
    def sample(self, changes=None):
        value = dict(time=1,pgpgin=0,available_mib=50000,swap_mib=194,
                     gpus='uuid, 1, 15000, 36, 0, 0',
                     temperatures={'k10temp:Tctl:hwmon1':95,'k10temp:Tdie:hwmon1':60})
        value.update(changes or {})
        return value

    def exercise(self, sample):
        with tempfile.TemporaryDirectory() as root:
            with patch('monitor.snapshot',return_value=self.sample()):
                monitor = Monitor(Path(root))
            with patch('monitor.snapshot',return_value=sample), patch.object(
                    monitor.finished,'wait',side_effect=lambda seconds: monitor.finished.set()):
                monitor.run()
            return monitor.failure

    def test_offset_control_temperature_is_not_actual_die_temperature(self):
        self.assertIsNone(self.exercise(self.sample()))

    def test_die_limit_aborts(self):
        self.assertIn('temperature',self.exercise(self.sample(dict(
            temperatures={'k10temp:Tdie:hwmon1':68}))))

    def test_memory_reserve_aborts(self):
        self.assertIn('memory reserve',self.exercise(self.sample(dict(available_mib=6000))))

if __name__ == '__main__':
    unittest.main(verbosity=2)
