import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from support import Rack, ROOT

class HostingTests(unittest.TestCase):
    def exercise(self, driver, fault=None):
        with tempfile.TemporaryDirectory(prefix='rack-pr35-hosting-') as directory:
            root=Path(directory); bin=root/'bin';bin.mkdir()
            for name in ['systemd-run','systemctl','docker','nvidia-smi']:
                path=bin/name;path.write_text('#!'+sys.executable+'\n'+(ROOT/'tests/runtime/fake_host.py').read_text().split('\n',1)[1]);path.chmod(0o700)
            def configure(c):
                p=c['profiles'][0];p['driver']=driver
                if driver=='docker':
                    p.update(container_image='sha256:'+'a'*64,executable=str(bin/'docker'),executable_sha256=hashlib.sha256((bin/'docker').read_bytes()).hexdigest())
                else:
                    artifact=root/'synthetic.gguf';artifact.write_bytes(b'fixture-only-gguf')
                    p.update(backend='llama_cpp',artifact=str(artifact),artifact_sha256=hashlib.sha256(artifact.read_bytes()).hexdigest())
            environment=dict(os.environ,RACK_HOST_FIXTURE=str(root),PATH=str(bin)+':'+os.environ['PATH'])
            if fault in ['memory','foreign']:
                (root/'faults.json').write_text(json.dumps({'memory_mib':1} if fault=='memory' else {'foreign_pid':os.getpid()}))
            r=Rack(root/'rack',configure=configure,environment=environment)
            try:
                d=r.acquire('athba','local-primary','low')
                if fault in ['memory','foreign']:
                    failed=r.wait(d,'recovery_required')
                    self.assertEqual(failed['reason'],'insufficient_device_memory' if fault=='memory' else 'foreign_gpu_process')
                    self.assertEqual(r.counts('start')['local-primary'],0)
                    return
                d=r.wait(d)
                result=r.result(r.infer(d))
                self.assertEqual(result['activation'],d['generation'])
                self.assertEqual(r.counts('dispatch')['local-primary'],1)
                if fault=='exited':
                    import signal,time
                    os.kill(d['process']['pid'],signal.SIGTERM)
                    r.wait(d,'recovery_required')
                    r.release(d);r.wait(d,'released')
                    return
                if fault in ('release', 'uncertain-release'):
                    if fault == 'uncertain-release':
                        r.controls('local-primary', uncertain=True)
                        r.result(r.infer(d), 'uncertain')
                    (root/'faults.json').write_text(json.dumps({'foreign_pid':os.getpid()}))
                    r.release(d); failed=r.wait(d,'recovery_required')
                    self.assertIn('gpu_cleanup',failed['reason'])
                    self.assertEqual(r.acquire('cb','local-fun-chat','paramount')['state'],'denied')
                    self.assertEqual(r.counts('start')['local-fun-chat'],0)
                    return
                r.release(d);r.wait(d,'released')
                self.assertEqual(r.counts('stop')['local-primary'],1)
                calls=[json.loads(l) for l in (root/'commands.jsonl').read_text().splitlines()]
                if driver=='docker':
                    start=next(c['args'] for c in calls if c['args'][0]=='run')
                    self.assertIn('--pull=never',start);self.assertIn('--memory=32m',start)
                else:
                    start=next(c['args'] for c in calls if c['program']=='systemd-run')
                    self.assertIn('--property=KillMode=control-group',start)
                    self.assertIn('--property=MemoryMax=32M',start)
            finally:r.close()

    def test_per_device_memory_fails_before_effects(self):self.exercise('systemd','memory')
    def test_foreign_gpu_process_is_never_stopped(self):self.exercise('systemd','foreign')
    def test_uncertain_release_keeps_claims(self):self.exercise('systemd','release')
    def test_uncertain_output_with_foreign_gpu_keeps_claims(self):self.exercise('systemd','uncertain-release')

    def test_already_exited_owned_container_can_be_released(self):self.exercise('docker','exited')

    def test_docker_vllm_production_adapter(self):self.exercise('docker')
    def test_systemd_llama_cpp_production_adapter(self):self.exercise('systemd')

if __name__=='__main__':unittest.main(verbosity=2)
