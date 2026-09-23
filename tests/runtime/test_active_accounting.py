"""Active-call accounting keeps compact control state separate from payloads."""
import json
import tempfile
import time
import unittest
from support import Rack, VERSION

class ActiveAccountingTests(unittest.TestCase):
    def managed(self, rack):
        return json.loads((rack.root/'authority'/'managed.json').read_text())

    def payload_bytes(self, rack):
        root = rack.root/'authority'/'payloads'
        return sum(path.stat().st_size for path in root.rglob('*') if path.is_file()) if root.exists() else 0

    def stored_invocation(self, rack, invocation_id):
        active = self.managed(rack)['data']['invocations'].get(invocation_id)
        if active is not None:
            return active
        archive = rack.root/'authority'/'archive'
        for path in archive.rglob('*.json') if archive.exists() else []:
            try:
                value = json.loads(path.read_text())
            except json.JSONDecodeError:
                continue
            record = value.get('record')
            if isinstance(record, dict) and record.get('id') == invocation_id:
                return record
        self.fail(f'invocation {invocation_id} was not found in active state or archive')

    def wait_dispatch(self, rack, model, count=1):
        deadline = time.monotonic() + 3
        while rack.counts('dispatch')[model] < count and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(rack.counts('dispatch')[model], count)

    def test_two_services_keep_three_mib_allowance_without_false_control_charge(self):
        with tempfile.TemporaryDirectory(prefix='rack-active-two-services-') as root:
            def configure(config):
                config['limits'] = dict(max_dispatch_workers=2, max_response_bytes=3*1024*1024)
            rack = Rack(root, configure=configure)
            try:
                primary = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                coder = rack.wait(rack.acquire('athba', 'local-coder', 'low'))
                rack.controls('local-primary', delay=1.5)
                rack.controls('local-coder', delay=1.5)
                started = time.monotonic()
                first = rack.infer(primary, identity='primary-running')
                second = rack.infer(coder, identity='coder-running')
                self.wait_dispatch(rack, 'local-primary')
                self.wait_dispatch(rack, 'local-coder')
                self.assertLess(time.monotonic()-started, 1.2)
                active = self.managed(rack)
                self.assertLess((rack.root/'authority'/'managed.json').stat().st_size, 30*1024*1024)
                self.assertGreater(self.payload_bytes(rack), 0)
                self.assertTrue(active['data']['invocations'][first['id']]['request_ref'])
                self.assertTrue(active['data']['invocations'][second['id']]['request_ref'])
                self.assertEqual(rack.result(first)['state'], 'completed')
                self.assertEqual(rack.result(second)['state'], 'completed')
            finally:
                rack.close()

    def test_near_limit_result_round_trips_and_hides_internal_refs(self):
        with tempfile.TemporaryDirectory(prefix='rack-active-roundtrip-') as root:
            rack = Rack(root)
            try:
                demand = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                content = ('unicode snowman ☃ quote " slash \\ newline \n tab \t ' * 25000)
                rack.controls('local-primary', content=content)
                invocation = rack.infer(demand, identity='near-limit')
                result = rack.result(invocation)
                self.assertEqual(result['result']['choices'][0]['message']['content'], content)
                for field in ['request_ref', 'result_ref', 'late_result_ref']:
                    self.assertNotIn(field, result)
                stored = self.stored_invocation(rack, invocation['id'])
                self.assertIn('result_ref', stored)
                self.assertIsNone(stored.get('result'))
                result_ref = stored['result_ref']
                result_path = rack.root/'authority'/result_ref['path']
                self.assertEqual(result_path.stat().st_size, result_ref['bytes'])
                self.assertGreater(result_ref['bytes'], len(content.encode()) + 50000)
            finally:
                rack.close()

    def test_oversized_output_fails_bounded_and_next_job_succeeds(self):
        with tempfile.TemporaryDirectory(prefix='rack-active-oversized-') as root:
            def configure(config):
                config['limits'] = dict(max_response_bytes=4096)
            rack = Rack(root, configure=configure)
            try:
                demand = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                rack.controls('local-primary', content='x' * 10000)
                bad = rack.infer(demand, identity='oversized')
                failed = rack.result(bad, 'failed')
                self.assertEqual(failed['error'], 'backend_response_oversized')
                rack.controls('local-primary', content='small')
                good = rack.infer(demand, identity='after-oversized')
                self.assertEqual(rack.result(good)['state'], 'completed')
                self.assertEqual(rack.counts('dispatch')['local-primary'], 2)
            finally:
                rack.close()

    def test_archive_maintenance_failure_does_not_rollback_release_or_completion(self):
        with tempfile.TemporaryDirectory(prefix='rack-active-archive-failure-') as root:
            rack = Rack(root)
            try:
                demand = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                archive = rack.root/'authority'/'archive'
                archive.write_text('not a directory')
                invocation = rack.infer(demand, identity='archive-blocked')
                self.assertEqual(rack.result(invocation)['state'], 'completed')
                rack.release(demand)
                released = rack.wait(demand, 'released')
                self.assertEqual(released['state'], 'released')
                self.assertEqual(self.managed(rack)['claims'], {})
                next_demand = rack.wait(rack.acquire('athba', 'local-primary', 'low', identity='after-archive-failure'))
                self.assertEqual(rack.result(rack.infer(next_demand))['state'], 'completed')
            finally:
                rack.close()

    def test_essential_request_store_failure_rejects_without_dispatch(self):
        with tempfile.TemporaryDirectory(prefix='rack-active-essential-failure-') as root:
            rack = Rack(root)
            try:
                demand = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                (rack.root/'authority'/'payloads').write_text('not a directory')
                request = dict(schema=VERSION, submission_id='payload-blocked', reservation_id=demand['id'],
                    generation=demand['generation'], profile_hash=demand['profile_hash'],
                    prompt='hello', max_tokens=16, timeout_seconds=5)
                failure = rack.call('athba', 'infer', status=400, request=request)
                self.assertNotEqual(failure['error'], 'capacity_active_evidence')
                self.assertEqual(rack.counts('dispatch')['local-primary'], 0)
            finally:
                rack.close()

if __name__ == '__main__':
    unittest.main(verbosity=2)
