"""Kernel-file fixtures: no systemd mutation, GPU call or live model."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch
from workload_probe import CgroupProbe, ProbeRoots, Workload
from workload_watch import WorkloadLimits, WorkloadWatch, memory_failure

class KernelFixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.roots = ProbeRoots(root/'cgroups', root/'proc')
        self.directory = self.roots.cgroup/'managed'
        self.directory.mkdir(parents=True)
        for name,value in {'memory.current':'100', 'memory.max':'1024',
                'memory.swap.current':'0','memory.swap.max':'0',
                'memory.events':'low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n',
                'memory.swap.events':'high 0\nmax 0\nfail 0\n'}.items():
            (self.directory/name).write_text(value)
        self.process = dict(pid=42,start='123',boot='boot',activation='abc',
                            unit='rack-runtime-abc.service',invocation='owned')
        self.demand = dict(id='reservation',generation='abc',process=self.process,
                           effect_started=True)
        self.target = Workload.from_record(self.demand)
        proc = self.roots.proc/'42'
        proc.mkdir(parents=True)
        (proc/'stat').write_text('42 (backend) S '+'0 '*18+'123')
        (proc/'environ').write_bytes(b'RACK_RUNTIME_ACTIVATION=abc\0')
        (proc/'cgroup').write_text('0::/managed\n')
        boot = self.roots.proc/'sys/kernel/random/boot_id'
        boot.parent.mkdir(parents=True)
        boot.write_text('boot\n')
        self.unit = dict(Id=self.target.unit,InvocationID='owned',MainPID='42',
                         ControlGroup='/managed',ActiveState='active',Job='')
        self.show = patch('workload_probe.unit_properties',side_effect=lambda _:dict(self.unit)).start()
        self.addCleanup(patch.stopall)
        self.probe = CgroupProbe(self.roots)
        self.limits = WorkloadLimits(1024)

class ProbeTests(KernelFixture):
    def test_owned_zero_swap_read_and_optional_pressure(self):
        value = self.probe.read(self.target)
        self.assertEqual(value['memory_current'],100)
        self.assertIsNone(value['pressure'])
        self.assertIsNone(memory_failure(value,self.limits))
        self.assertEqual(self.show.call_count,2)

    def test_real_counter_memory_failures(self):
        for name,value in (('memory.swap.current','1'),('memory.swap.max','1'),
                ('memory.max','2048'),('memory.current','1025'),
                ('memory.events','low 0\nhigh 0\nmax 0\noom 1\noom_kill 1'),
                ('memory.swap.events','high 0\nmax 0\nfail 1')):
            with self.subTest(name=name):
                path=self.directory/name
                original=path.read_text()
                path.write_text(value)
                self.assertIsNotNone(memory_failure(self.probe.read(self.target),self.limits))
                path.write_text(original)

    def test_reclaim_limit_events_alone_are_not_oom(self):
        (self.directory/'memory.events').write_text('low 0\nhigh 20\nmax 80\noom 0\noom_kill 0')
        self.assertIsNone(memory_failure(self.probe.read(self.target),self.limits))

    def test_missing_malformed_and_unbounded_required_counters_fail(self):
        path=self.directory/'memory.max'
        for value in ('max','broken'):
            path.write_text(value)
            with self.assertRaises(ValueError): self.probe.read(self.target)
        path.unlink()
        with self.assertRaises(FileNotFoundError): self.probe.read(self.target)

    def test_incomplete_event_evidence_fails(self):
        (self.directory/'memory.events').write_text('oom 0')
        with self.assertRaisesRegex(ValueError,'incomplete'): self.probe.read(self.target)

    def test_unit_identity_and_ownership_fail_closed(self):
        original=dict(self.unit)
        for change in (dict(InvocationID='foreign'),dict(MainPID='99'),
                       dict(InvocationID=''),dict(ControlGroup='/../elsewhere'),dict(ControlGroup='/')):
            with self.subTest(change=change):
                self.unit.update(change)
                with self.assertRaises(ValueError): self.probe.read(self.target)
                self.unit=original.copy()

    def test_process_generation_activation_and_membership_fail_closed(self):
        proc=self.roots.proc/'42'
        for name,value in (('stat','42 (backend) S '+'0 '*18+'124'),
                           ('environ','RACK_RUNTIME_ACTIVATION=other\0'),('cgroup','0::/other')):
            path=proc/name
            original=path.read_bytes()
            path.write_text(value)
            with self.assertRaises(ValueError): self.probe.read(self.target)
            path.write_bytes(original)

    def test_observation_transport_and_permission_errors_fail(self):
        self.show.side_effect=TimeoutError('systemd unavailable')
        with self.assertRaises(TimeoutError): self.probe.read(self.target)
        self.show.side_effect=lambda _:dict(self.unit)
        with patch.object(Path,'read_text',side_effect=PermissionError('denied')):
            with self.assertRaises(PermissionError): self.probe.read(self.target,True)

    def test_unexpected_disappearance_fails_but_explicit_retirement_is_observed(self):
        self.unit.update(MainPID='0',ControlGroup='',InvocationID='',ActiveState='inactive')
        with self.assertRaises(ValueError): self.probe.read(self.target)
        self.assertEqual(self.probe.read(self.target,True)['status'],'tearing_down')
        self.unit['InvocationID']='foreign'
        with self.assertRaises(ValueError): self.probe.read(self.target,True)

    def test_retiring_cgroup_still_reports_oom_and_missing_evidence_fails(self):
        self.unit.update(MainPID='0',ActiveState='deactivating',Job='42')
        (self.directory/'memory.events').write_text('low 0\nhigh 0\nmax 0\noom 1\noom_kill 0')
        sample=self.probe.read(self.target,True)
        self.assertEqual(sample['status'],'retiring')
        self.assertIn('OOM',memory_failure(sample,self.limits))
        (self.directory/'memory.events').unlink()
        with self.assertRaises(FileNotFoundError): self.probe.read(self.target,True)

class WatchTests(KernelFixture):
    def test_registration_is_bounded_and_begins_before_ready(self):
        watch=WorkloadWatch(self.limits,self.probe)
        self.assertEqual(watch.sample()['status'],'not_expected')
        with patch('workload_watch.time.monotonic',return_value=100):
            watch.observe(dict(self.demand,process=None))
            self.assertEqual(watch.sample()['status'],'awaiting_registration')
        with patch('workload_watch.time.monotonic',return_value=111):
            with self.assertRaisesRegex(RuntimeError,'registration'): watch.sample()
        watch.observe(self.demand)
        self.assertEqual(watch.sample()['status'],'active')

    def test_replaced_cgroup_is_not_rebound(self):
        watch=WorkloadWatch(self.limits,self.probe)
        watch.observe(self.demand)
        watch.sample()
        old=self.directory.with_name('old-managed')
        self.directory.rename(old)
        self.directory.mkdir()
        for path in old.iterdir(): (self.directory/path.name).write_bytes(path.read_bytes())
        with self.assertRaisesRegex(RuntimeError,'replaced'): watch.sample()

    def test_scope_and_cleanup_receipt_must_match(self):
        watch=WorkloadWatch(self.limits,self.probe)
        watch.observe(dict(self.demand,process=None))
        with self.assertRaisesRegex(ValueError,'reservation'):
            watch.observe(dict(self.demand,id='unrelated'))
        watch.observe(self.demand)
        receipt=dict(self.demand,state='cancelled',process=None,effect_started=False,released=True)
        for change in (dict(id='unrelated'),dict(effect_started=True),dict(released=False)):
            with self.assertRaises(ValueError): watch.clear(dict(receipt,**change))
        watch.retire()
        watch.clear(receipt)
        self.assertEqual(watch.sample()['status'],'not_expected')

    def test_monitor_abort_propagates_without_a_writable_abort_file(self):
        from client import Client, Connection
        client=Client(Connection('http://127.0.0.1:1','fixture'))
        client.safety_check=Mock(side_effect=RuntimeError('monitor persistence failure'))
        client.call=Mock()
        with self.assertRaisesRegex(RuntimeError,'persistence'):
            client.wait(('inspect',dict(reservation_id='owned'),'ready'),1)
        client.call.assert_not_called()
