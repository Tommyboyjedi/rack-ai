import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from monitor import Monitor, MonitorLimits
from workload_watch import WorkloadLimits

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
                monitor = Monitor(Path(root),MonitorLimits(workload=WorkloadLimits(
                    memory_max_bytes=sample.get('workload',{}).get('memory_max',1024))))
            with patch('monitor.snapshot',return_value=sample), patch.object(
                    monitor.workload,'sample',return_value=sample.get('workload',dict(status='not_expected'))), patch.object(
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

    def test_global_swap_growth_with_host_reserve_and_no_workload_swap_is_not_failure(self):
        healthy = dict(status='active', memory_current=46246285312,
                       memory_max=47244640256, swap_current=0, swap_max=0,
                       events=dict(low=0,high=0,max=0,oom=0,oom_kill=0))
        self.assertIsNone(self.exercise(self.sample(dict(available_mib=58166,
            swap_mib=1000,workload=healthy))))

    def test_workload_swap_or_oom_aborts(self):
        for change in (dict(swap_current=1),dict(events=dict(oom=1,oom_kill=0)),
                       dict(events=dict(oom=0,oom_kill=1)),dict(swap_max=1024)):
            with self.subTest(change=change):
                workload=dict(status='active',memory_current=100,memory_max=1024,
                              swap_current=0,swap_max=0,events=dict(oom=0,oom_kill=0))
                workload.update(change)
                self.assertIsNotNone(self.exercise(self.sample(dict(workload=workload))))

    def test_snapshot_failure_aborts(self):
        with tempfile.TemporaryDirectory() as root:
            with patch('monitor.snapshot',return_value=self.sample()):
                monitor=Monitor(Path(root))
            with patch('monitor.snapshot',side_effect=OSError('evidence unavailable')):
                monitor.run()
            with self.assertRaisesRegex(RuntimeError,'evidence unavailable'):
                monitor.check()

    def test_log_creation_failure_is_not_a_silent_monitor_death(self):
        with tempfile.TemporaryDirectory() as root:
            with patch('monitor.snapshot',return_value=self.sample()):
                monitor=Monitor(Path(root))
            with patch.object(Path,'open',side_effect=PermissionError('read-only evidence')):
                monitor.run()
            with self.assertRaises(RuntimeError):
                monitor.check()

class PressureTests(unittest.TestCase):
    def test_sustained_page_in_guard_is_retained_only_for_inference(self):
        baseline=dict(time=0,pgpgin=0,available_mib=50000,swap_mib=100,
                      gpus='uuid, 1, 15000, 36, 0, 0',temperatures={})
        for phase in ('loading','inference'):
            with self.subTest(phase=phase), tempfile.TemporaryDirectory() as root:
                with patch('monitor.snapshot',return_value=baseline):
                    monitor=Monitor(Path(root))
                monitor.phase=phase
                samples=[dict(baseline,time=i*2,pgpgin=i*2*300*1024) for i in range(1,11)]
                waits=[]
                def pause(seconds):
                    waits.append(seconds)
                    if len(waits)==10: monitor.finished.set()
                with patch('monitor.snapshot',side_effect=samples), patch.object(
                        monitor.finished,'wait',side_effect=pause):
                    monitor.run()
                if phase=='loading': self.assertIsNone(monitor.failure)
                else: self.assertEqual(monitor.failure,'sustained inference page-in pressure')

if __name__ == '__main__':
    unittest.main(verbosity=2)
