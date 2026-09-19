"""Published schemas match actual receiver responses and reusable client fixtures."""
import json
import tempfile
from pathlib import Path
import subprocess
import unittest
import urllib.error
import urllib.request
import jsonschema
from support import Rack, ROOT, VERSION

class ContractTests(unittest.TestCase):
    def test_public_responses_and_fixture_schemas(self):
        schema=json.loads((ROOT/'config/runtime/response.schema.json').read_text())
        requests=json.loads((ROOT/'config/runtime/request.schema.json').read_text())
        for fixture in (ROOT/'config/runtime/fixtures').glob('*.json'):
            jsonschema.validate(json.loads(fixture.read_text()),requests)
        def validate(result):
            jsonschema.validate(dict(schema=VERSION,result=result),schema)
        with tempfile.TemporaryDirectory(prefix='rack-pr35-contract-') as root:
            r=Rack(root)
            try:
                validate(r.call('athba','discover'))
                d=r.wait(r.acquire('athba','local-primary','low'));validate(d)
                denied=r.acquire('athba','local-primary','low');validate(denied)
                invocation=r.infer(d);validate(invocation);validate(r.result(invocation))
                r.release(d);validate(r.wait(d,'released'))
            finally:r.close()

    def test_example_configuration_validates_without_effects(self):
        with tempfile.TemporaryDirectory(prefix='rack-pr35-config-') as root:
            path=Path(root)/'config.json'
            path.write_bytes((ROOT/'config/runtime/config.example.json').read_bytes());path.chmod(0o600)
            result=subprocess.run([str(ROOT/'target/debug/rack_ai_runtime'),'validate',str(path)],capture_output=True,text=True,timeout=5)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.strip(),'RUNTIME_CONFIG_VALID')
            self.assertEqual(list(Path(root).iterdir()),[path])

    def test_local_coder_runtime_capabilities_match_workspace_qualification(self):
        runtime = json.loads((ROOT/'config/runtime/config.example.json').read_text())
        models = json.loads((ROOT/'config/models.json').read_text())
        workers = json.loads((ROOT/'config/workers.json').read_text())
        coder_profile = next(p for p in runtime['profiles'] if p['tag'] == 'local-coder')
        coder_worker = next(w for w in workers['workers'] if w['id'] == 'local-coder')
        coder_model = next(m for m in models['models'] if m['id'] == coder_worker['model_id'])
        qualified = coder_model['eligibility_profile']['capabilities']
        self.assertEqual(coder_profile['capabilities'], ['coding'])
        self.assertEqual(qualified, ['coding'])
        self.assertLessEqual(set(coder_profile['capabilities']), set(qualified))

class ContractEndpointTests(unittest.TestCase):
    def get_contract(self, rack, authorization=None):
        headers = {} if authorization is None else {'Authorization': authorization}
        request = urllib.request.Request(
            f'http://{rack.address}/runtime/v1/contract', headers=headers)
        try:
            response = urllib.request.urlopen(request, timeout=4)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, json.loads(response.read())

    def test_contract_requires_existing_bearer_authentication(self):
        with tempfile.TemporaryDirectory(prefix='rack-contract-auth-') as root:
            r = Rack(root)
            try:
                for token in [None, 'Bearer wrong', 'Basic athba', 'bearer athba']:
                    with self.subTest(authorization=token):
                        status, body = self.get_contract(r, token)
                        self.assertEqual(status, 401)
                        self.assertEqual(body, dict(schema=VERSION, error='unauthorized'))
                for source in ['athba', 'other']:
                    self.assertEqual(self.get_contract(r, f'Bearer {source}')[0], 200)
            finally:
                r.close()

    def test_contract_contains_canonical_files_and_is_read_only(self):
        with tempfile.TemporaryDirectory(prefix='rack-contract-embedded-') as root:
            r = Rack(root)
            try:
                # No contract files exist in the receiver's configured runtime directory.
                self.assertFalse((r.root/'docs').exists())
                self.assertFalse((r.root/'config/runtime').exists())
                discovery = r.call('athba', 'discover')
                authority = r.root/'authority/managed.json'
                before = authority.read_bytes()
                status, body = self.get_contract(r, 'Bearer athba')
                self.assertEqual(status, 200)
                self.assertEqual(body, dict(
                    schema='rack-ai/runtime-contract/v1', contract_version='1.2.0',
                    documentation=(ROOT/'docs/reservation-work.md').read_text(),
                    request_schema=json.loads((ROOT/'config/runtime/request.schema.json').read_text()),
                    response_schema=json.loads((ROOT/'config/runtime/response.schema.json').read_text())))
                self.assertEqual(r.call('athba', 'discover'), discovery)
                self.assertEqual(authority.read_bytes(), before)
                self.assertFalse(r.events.exists())
            finally:
                r.close()

if __name__=='__main__':unittest.main(verbosity=2)
