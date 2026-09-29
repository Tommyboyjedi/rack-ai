import json
import tempfile
import time
import unittest

import jsonschema

from support import Rack, ROOT


class PublicWorkExecutionTests(unittest.TestCase):
    def test_managed_inference_work_execution_projection_is_owner_checked(self):
        schema = json.loads((ROOT / 'config/runtime/response.schema.json').read_text())

        def validate(result):
            jsonschema.validate(dict(schema='rack-ai/runtime/v1', result=result), schema)

        with tempfile.TemporaryDirectory(prefix='rack-work-execution-public-') as root:
            r = Rack(root)
            try:
                reservation = r.wait(r.acquire('athba', 'local-primary', 'low'))
                request = dict(
                    reservation_id=reservation['id'],
                    service='local-primary',
                    work_id='public-exec-result',
                    payload=dict(
                        kind='inference',
                        prompt='return a tiny deterministic answer',
                        max_tokens=16,
                        timeout_seconds=5,
                    ),
                )
                submitted = r.call('athba', 'submit_work', request=request)
                validate(submitted)

                deadline = time.monotonic() + 8
                report = None
                while time.monotonic() < deadline:
                    report = r.call('athba', 'inspect_work_execution', work_id='public-exec-result')
                    validate(report)
                    if report['work']['state'] == 'completed':
                        break
                    time.sleep(0.03)
                self.assertEqual(report['work']['state'], 'completed', report)
                self.assertEqual(report['schema'], 'rack-ai/work-execution/v1')
                self.assertEqual(report['contract_version'], '1.4.0')
                self.assertEqual(report['work']['work_id'], 'public-exec-result')
                self.assertEqual(report['outcome']['kind'], 'inference')
                self.assertTrue(report['outcome']['terminal'])
                self.assertTrue(report['outcome']['outcome_known'])
                self.assertEqual(report['outcome']['category'], 'succeeded')
                self.assertTrue(report['outcome']['attempt']['known'])
                self.assertEqual(report['outcome']['historical_invocation']['state'], 'completed')
                self.assertTrue(report['outcome']['model_usage']['available'])
                self.assertIn(report['closure']['cleanup_state'], ['not_required', 'archived'])
                self.assertTrue(report['closure']['safe_closure_known'])
                self.assertEqual(report['activity']['current'], 'terminal')
                self.assertEqual(report['activity']['timings']['queue_wait_seconds']['availability'], 'recorded')
                self.assertEqual(report['activity']['timings']['active_execution_seconds']['availability'], 'recorded')
                self.assertIn('agent_execution_seconds', report['activity']['timings'])
                self.assertIn('budget', report['activity'])
                self.assertIn('replay_safety', report['closure'])
                rendered = json.dumps(report)
                self.assertNotIn('packet_path', rendered)
                self.assertNotIn('worktree_path', rendered)

                denied = r.call('cb', 'inspect_work_execution', status=404, work_id='public-exec-result')
                self.assertEqual(denied['error'], 'not_found')
                self.assertEqual(r.call('athba', 'get_work_artifact', status=400, artifact_id='wa1.bad')['error'], 'invalid_artifact_id')

                replay = r.call('athba', 'submit_work', request=request)
                validate(replay)
                self.assertEqual(replay['invocation_id'], report['work']['invocation_id'])
                changed = dict(request)
                changed['payload'] = dict(request['payload'], prompt='changed prompt')
                conflict = r.call('athba', 'submit_work', status=409, request=changed)
                self.assertEqual(conflict['error'], 'identity_conflict')
                r.release(reservation)
                r.wait(reservation, 'released')
            finally:
                r.close()


if __name__ == '__main__':
    unittest.main(verbosity=2)
