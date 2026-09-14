"""Cleanup behavior with disposable mock transports, no live service calls."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch
from lifecycle import Lifecycle
from client import save
from services import Services

class LifecycleTests(unittest.TestCase):
    def test_workload_observation_starts_at_acquisition_and_before_ready(self):
        with tempfile.TemporaryDirectory() as root:
            accepted=dict(id='owned',generation='gen',state='preparing',
                          preflight_done=False,process=None,effect_started=False)
            starting=dict(accepted,preflight_done=True,effect_started=True)
            registered=dict(starting,process={'pid':42})
            ready=dict(registered,state='ready')
            monitor=Mock()
            client=Mock()
            client.call.side_effect=[accepted,starting,registered,ready]
            with patch('lifecycle.time.sleep'):
                self.assertEqual(Lifecycle(client,Path(root)).acquire(monitor),ready)
            observations=[call.args[0] for call in monitor.workload.observe.call_args_list]
            self.assertEqual(observations,[accepted,starting,registered,ready])

    def test_lost_acquisition_reuses_exact_identity_before_release(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)
            request = dict(acquisition_id='original')
            save(path/'acquisition-request.json',request)
            client = Mock()
            current = dict(id='reservation',generation='activation',state='ready',process=None)
            terminal = dict(current,state='cancelled')
            client.call.side_effect = [current,current,current,terminal]
            Lifecycle(client,path).release()
            self.assertEqual(client.call.call_args_list[0].args[0],
                             dict(operation='acquire',request=request))
            self.assertEqual(client.call.call_args_list[2].args[0]['request']['action'],
                             dict(kind='cancel'))

    def test_unproven_cleanup_is_explicit(self):
        with tempfile.TemporaryDirectory() as root:
            client = Mock()
            current = dict(id='reservation',generation='activation',state='ready',process=None)
            client.call.side_effect = [current,current,dict(current,state='recovery_required',reason='foreign_gpu')]
            lifecycle = Lifecycle(client,Path(root))
            lifecycle.demand = current
            with self.assertRaisesRegex(RuntimeError,'foreign_gpu'):
                lifecycle.release()

    def test_restore_never_starts_models_with_live_claims(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)
            save(path/'managed.json',dict(claims={'gpu':'owned'},data=dict(demands={})))
            services = Services.__new__(Services)
            services.quiesced = True
            with patch('services.ROOT',path), patch('services.gpu_empty'), patch('services.command') as command:
                with self.assertRaisesRegex(RuntimeError,'cleanup'):
                    services.restore()
                command.assert_not_called()

    def test_missing_model_mount_blocks_before_any_service_change(self):
        with tempfile.TemporaryDirectory() as root:
            services = Services.__new__(Services)
            services.containers = {'vllm-primary': dict(Mounts=[
                dict(Type='bind',Source=str(Path(root)/'deleted-cache'))])}
            with patch('services.command') as command:
                with self.assertRaisesRegex(RuntimeError,'missing bind source'):
                    services.stop()
                command.assert_not_called()

if __name__ == '__main__':
    unittest.main(verbosity=2)
