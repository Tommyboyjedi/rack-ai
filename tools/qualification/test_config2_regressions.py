"""Config2 failures reproduced without Docker/systemd mutations or models."""
import copy
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from services import Services
from test_workload_probe import KernelFixture

class ExitFixture(KernelFixture):
    def prepared_exit(self):
        # Same authenticated invocation; the process is now reaped.
        self.unit.update(MainPID='0',ActiveState='failed')
        for path in (self.roots.proc/'42').iterdir(): path.unlink()
        (self.roots.proc/'42').rmdir()
        record=dict(MESSAGE_ID='98e322203f7a4ed290d09fe03c09fe15',
                    USER_UNIT=self.target.unit,USER_INVOCATION_ID=self.target.invocation,
                    _BOOT_ID='boot',_UID=str(os.getuid()),
                    _COMM='systemd',_EXE='/usr/lib/systemd/systemd',
                    _SYSTEMD_USER_UNIT='init.scope',COMMAND='ExecStart',
                    EXIT_CODE='exited',EXIT_STATUS='1',MESSAGE='Main process exited')
        return record

class BackendExitTests(ExitFixture):
    def test_exact_owned_startup_exit_is_a_backend_failure(self):
        record=self.prepared_exit()
        with patch('backend_exit.exit_records',return_value=[record]):
            sample=self.probe.read(self.target)
        self.assertEqual(sample['status'],'exited')
        self.assertEqual(sample['exit_evidence']['EXIT_STATUS'],'1')

class ExitSafetyTests(ExitFixture):
    def test_pid_reuse_is_never_reclassified_as_backend_failure(self):
        record=self.prepared_exit()
        proc=self.roots.proc/'42'
        proc.mkdir()
        (proc/'stat').write_text('42 (foreign) S '+'0 '*18+'999')
        with patch('backend_exit.exit_records',return_value=[record]):
            with self.assertRaisesRegex(ValueError,'generation changed'):
                self.probe.read(self.target)

    def test_changed_unit_or_invocation_remains_blocked(self):
        record=self.prepared_exit()
        for change in (dict(Id='foreign.service'),dict(InvocationID='foreign')):
            with self.subTest(change=change):
                original=dict(self.unit)
                self.unit.update(change)
                with patch('backend_exit.exit_records',return_value=[record]) as journal:
                    with self.assertRaisesRegex(ValueError,'identity changed'):
                        self.probe.read(self.target)
                    journal.assert_not_called()
                self.unit=original

    def test_unexplained_or_untrusted_exit_still_fails_safe(self):
        record=self.prepared_exit()
        variants=[[],[record,record],[dict(record,_EXE='/bin/backend')],
                  [dict(record,USER_INVOCATION_ID='other')],[dict(record,_BOOT_ID='other')]]
        for records in variants:
            with self.subTest(records=records), patch('backend_exit.exit_records',return_value=records):
                with self.assertRaises(ValueError): self.probe.read(self.target)

    def test_journal_failure_is_monitoring_evidence_loss(self):
        self.prepared_exit()
        with patch('backend_exit.exit_records',side_effect=PermissionError('journal unavailable')):
            with self.assertRaises(PermissionError): self.probe.read(self.target)

    def test_pid_reuse_during_journal_query_is_blocked(self):
        record=self.prepared_exit()
        def records(target):
            proc=self.roots.proc/'42'
            proc.mkdir()
            (proc/'stat').write_text('42 (foreign) S '+'0 '*18+'999')
            return [record]
        with patch('backend_exit.exit_records',side_effect=records):
            with self.assertRaisesRegex(ValueError,'generation changed'): self.probe.read(self.target)

    def test_delayed_journal_visibility_is_bounded_reconciliation(self):
        record=self.prepared_exit()
        with patch('backend_exit.exit_records',side_effect=[[],[record]]) as journal:
            self.assertEqual(self.probe.read(self.target)['status'],'exited')
        self.assertEqual(journal.call_count,2)

    def test_process_exit_before_systemd_observation_settles(self):
        record=self.prepared_exit()
        ended=dict(self.unit)
        active=dict(ended,MainPID='42',ActiveState='active')
        observations=[]
        def show(unit):
            observations.append(unit)
            return active if len(observations)<3 else ended
        self.show.side_effect=show
        with patch('backend_exit.exit_records',return_value=[record]):
            self.assertEqual(self.probe.read(self.target)['status'],'exited')
        self.assertGreaterEqual(len(observations),4)

    def test_unsettled_unit_after_exact_pid_exit_remains_ambiguous(self):
        record=self.prepared_exit()
        self.unit.update(MainPID='42',ActiveState='active')
        with patch('backend_exit.exit_records',return_value=[record]):
            with self.assertRaisesRegex(ValueError,'unit state is ambiguous'):
                self.probe.read(self.target)

    def test_same_generation_zombie_needs_matching_exit_evidence(self):
        record=self.prepared_exit()
        proc=self.roots.proc/'42'
        proc.mkdir()
        (proc/'stat').write_text('42 (backend) Z '+'0 '*18+'123')
        with patch('backend_exit.exit_records',return_value=[record]):
            self.assertEqual(self.probe.read(self.target)['status'],'exited')

class ExitIntegrationTests(ExitFixture):
    def monitor(self):
        from monitor import Monitor, MonitorLimits
        from workload_watch import WorkloadWatch
        snapshot=dict(time=1,pgpgin=0,available_mib=50000,swap_mib=100,
                      gpus='uuid, 1, 15000, 36, 0, 0',temperatures={})
        with patch('monitor.snapshot',return_value=snapshot):
            monitor=Monitor(Path(self.tmp.name),MonitorLimits(workload=self.limits))
        monitor.workload=WorkloadWatch(self.limits,self.probe)
        monitor.workload.observe(self.demand)
        return monitor,snapshot

    def test_monitor_persists_exit_without_integrity_corruption_label(self):
        monitor,snapshot=self.monitor()
        record=self.prepared_exit()
        with patch('monitor.snapshot',return_value=snapshot), patch('backend_exit.exit_records',return_value=[record]):
            monitor.run()
        with self.assertRaisesRegex(RuntimeError,'managed backend startup failure: exited status=1'):
            monitor.check()
        self.assertNotIn('monitor failed',monitor.failure)
        saved=json.loads((Path(self.tmp.name)/'backend-exit.json').read_text())
        self.assertEqual(saved['process']['start'],'123')
        self.assertEqual(saved['exit_evidence'],record)
        hardware=json.loads((Path(self.tmp.name)/'hardware.jsonl').read_text())
        self.assertEqual(hardware['workload']['status'],'exited')

    def test_authority_failure_before_monitor_tick_is_classified(self):
        from lifecycle import Lifecycle
        from unittest.mock import Mock
        monitor,_=self.monitor()
        record=self.prepared_exit()
        starting=dict(self.demand,state='preparing',preflight_done=True)
        failed=dict(starting,state='recovery_required',reason='process_exited')
        client=Mock()
        client.call.side_effect=[starting,failed]
        with patch('backend_exit.exit_records',return_value=[record]):
            with self.assertRaisesRegex(RuntimeError,'managed backend startup failure'):
                Lifecycle(client,Path(self.tmp.name)).acquire(monitor)
        self.assertTrue((Path(self.tmp.name)/'backend-exit.json').exists())

    def test_release_keeps_backend_journal_after_process_record_is_cleared(self):
        from lifecycle import Lifecycle
        from unittest.mock import Mock
        monitor,_=self.monitor()
        current=dict(self.demand,state='cancelled',process=None,effect_started=False,released=True)
        client=Mock()
        client.call.side_effect=[current,current,current]
        lifecycle=Lifecycle(client,Path(self.tmp.name))
        lifecycle.monitor=monitor
        lifecycle.demand=current
        result=Mock(stdout='backend allocation failure\n',stderr='')
        with patch('lifecycle.subprocess.run',return_value=result) as commands, patch(
                'lifecycle.urllib.request.urlopen',side_effect=OSError('backend exited')):
            lifecycle.release()
        self.assertEqual(commands.call_args_list[0].args[0][3],self.target.unit)
        self.assertIn('backend allocation failure',(Path(self.tmp.name)/'backend-journal.txt').read_text())

    def test_actual_disposable_process_exits_nonzero_during_activation(self):
        import subprocess
        import sys
        from workload_probe import CgroupProbe, ProbeRoots, Workload
        child=subprocess.Popen([sys.executable,'-c','import sys; sys.stdin.read(1); sys.exit(7)'],
                               stdin=subprocess.PIPE,env=dict(os.environ,RACK_RUNTIME_ACTIVATION='abc'))
        try:
            proc=Path('/proc')/str(child.pid)
            fields=(proc/'stat').read_text().rsplit(') ',1)[1].split()
            self.process.update(pid=child.pid,start=fields[19],
                                boot=Path('/proc/sys/kernel/random/boot_id').read_text())
            self.target=Workload.from_record(self.demand)
            self.probe=CgroupProbe(ProbeRoots(self.roots.cgroup,Path('/proc')))
            child.communicate(b'x',timeout=3)
            self.assertEqual(child.returncode,7)
            record=self.prepared_exit()
            record.update(_BOOT_ID=self.target.boot.replace('-',''),EXIT_STATUS='7')
            monitor,snapshot=self.monitor()
            with patch('backend_exit.exit_records',return_value=[record]), patch('monitor.snapshot',return_value=snapshot):
                monitor.run()
            self.assertEqual(monitor.failure,'managed backend startup failure: exited status=7')
            self.assertEqual(json.loads((Path(self.tmp.name)/'backend-exit.json').read_text())['process']['pid'],child.pid)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=3)

    def test_exit_proof_does_not_hide_replaced_cgroup(self):
        monitor,_=self.monitor()
        monitor.workload.sample()
        record=self.prepared_exit()
        old=self.directory.with_name('old-managed')
        self.directory.rename(old)
        self.directory.mkdir()
        with patch('backend_exit.exit_records',return_value=[record]):
            with self.assertRaisesRegex(RuntimeError,'cgroup replaced'):
                monitor.workload.sample()

class MountRestoreTests(unittest.TestCase):
    def exercise(self, change):
        mount=dict(Type='bind',Source='/models',Destination='/model',Mode='ro',
                   RW=False,Propagation='rprivate')
        original=dict(Id='owned',Image='sha256:owned',Config={'Env':['A=B']},
                      HostConfig={'Binds':['/models:/model:ro']},
                      Mounts=[mount,dict(mount,Source='/cache',Destination='/cache')],
                      State={'Health':{'Status':'healthy'}})
        current=copy.deepcopy(original)
        change(current)
        with tempfile.TemporaryDirectory() as directory:
            services=Services.__new__(Services)
            services.directory=Path(directory)
            services.containers={'coder':original}
            services.changed_models=['coder']
            services.changed_units=[]
            services.quiesced=True
            services.units={}
            services.comfy='inactive'
            def command(argv):
                if argv[:2]==['docker','inspect']: return json.dumps([current])
                if argv[:2]==['docker','start']: return 'owned'
                raise AssertionError(argv)
            with patch('services.ROOT',Path(directory)), patch('services.gpu_empty'), \
                    patch('services.unit_state',return_value='inactive'), \
                    patch('services.command',side_effect=command) as calls:
                try:
                    services.restore()
                except (RuntimeError,ValueError):
                    self.assertFalse(any(c.args[0][:2]==['docker','start'] for c in calls.call_args_list))
                    raise
            self.assertIn(['docker','start','owned'],[c.args[0] for c in calls.call_args_list])

    def test_mount_order_is_not_configuration_change(self):
        self.exercise(lambda value:value['Mounts'].reverse())

    def test_genuine_mount_changes_are_rejected(self):
        for field,value in dict(Type='volume',Source='/other',Destination='/other',
                                Mode='rw',RW=True,Propagation='shared').items():
            with self.subTest(field=field), self.assertRaisesRegex(RuntimeError,'Mounts'):
                self.exercise(lambda current:current['Mounts'][0].update({field:value}))

    def test_duplicate_count_and_extra_mount_fields_are_preserved(self):
        for change in (lambda c:c['Mounts'].append(dict(c['Mounts'][0])),
                       lambda c:c['Mounts'][0].update(Name='unexpected')):
            with self.assertRaisesRegex(RuntimeError,'Mounts'): self.exercise(change)

    def test_other_container_identity_checks_remain_strict(self):
        for field,value in dict(Id='foreign',Image='other',Config={},HostConfig={}).items():
            with self.subTest(field=field), self.assertRaisesRegex(RuntimeError,field):
                self.exercise(lambda current:current.update({field:value}))
