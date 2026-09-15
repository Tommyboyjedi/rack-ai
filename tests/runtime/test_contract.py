"""Published schemas match actual receiver responses and reusable client fixtures."""
import json
import tempfile
from pathlib import Path
import subprocess
import unittest
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

if __name__=='__main__':unittest.main(verbosity=2)
