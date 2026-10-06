"""Real receiver/scoped gateway and bounded workspace fixtures; no GPU/model qualification."""
import json
import time
import unittest
import urllib.request
import urllib.error
from pathlib import Path
import jsonschema
import test_managed_workspace as workspace_fixture
from support import ROOT, VERSION

class InteractionDiagnosticsTests(unittest.TestCase):
    def setUp(self):
        self.fixture = workspace_fixture.ManagedWorkspace('test_reserved_work_uses_managed_access_for_all_turns')
        self.fixture.setUp()
        self.r = self.fixture.rack
        self.response_schema = json.loads((ROOT/'config/runtime/response.schema.json').read_text())
    def tearDown(self):
        self.fixture.tearDown()
    def validate(self, result):
        jsonschema.validate(dict(schema=VERSION, result=result), self.response_schema)
    def enabled(self):
        self.r.release(self.fixture.p)
        self.r.wait(self.fixture.p, 'released')
        request = dict(acquisition_id='diagnostic-acquisition', work_id='diagnostic-fixture',
            services=['local-primary'], priority='low', ttl_seconds=60,
            diagnostics=dict(retained_model_interactions=True))
        reservation = self.r.call('athba', 'reserve', request=request)
        deadline = time.monotonic()+10
        while reservation['state'] != 'ready' and time.monotonic() < deadline:
            reservation = self.r.call('athba', 'inspect_reservation', reservation_id=reservation['id'])
            time.sleep(.03)
        self.assertEqual(reservation['state'], 'ready', reservation)
        self.fixture.p = reservation['services']['local-primary']
        self.validate(reservation)
        changed = dict(request, diagnostics=dict(retained_model_interactions=False))
        self.r.call('athba', 'reserve', status=409, request=changed)
        return reservation
    def post(self, path, body, identity=None):
        headers = {'Content-Type':'application/json'}
        if identity: headers['Idempotency-Key']=identity
        req = urllib.request.Request('http://'+self.r.address+self.fixture.p['gateway_path']+path,
            data=json.dumps(body).encode(), headers=headers)
        try: response=urllib.request.urlopen(req,timeout=15)
        except urllib.error.HTTPError as error: response=error
        with response: return response.status, response.read().decode()
    def report(self):
        report = self.r.call('athba', 'inspect_work_execution', work_id='diagnostic-task')
        self.validate(report)
        return report
    def three_turn_task(self):
        f=self.fixture
        (f.fixture/'src/harness-control.json').write_text(json.dumps(dict(three_turns=True)))
        f.git('add','src/harness-control.json');f.git('commit','-m','bounded diagnostics fixture')
        f.base=f.git('rev-parse','HEAD').strip()
        submitted=self.r.call('athba','submit_work',request=f.spec('diagnostic-task'))
        deadline=time.monotonic()+20
        marker=None
        while time.monotonic()<deadline:
            marker=next((f.root/'workspaces').rglob('first-turn'),None)
            if marker:return submitted,marker
            time.sleep(.04)
        self.fail(self.r.call('athba','inspect_work',work_id='diagnostic-task'))
    def test_active_public_history_effective_bounds_restart_and_release(self):
        reservation=self.enabled()
        self.assertTrue(self.r.call('athba','discover')['model_interaction_diagnostics']['supported'])
        submitted,marker=self.three_turn_task()
        # Register an independent generic scope against the same live parent, through public control.
        status,_=self.post('/scopes/diagnostic-tools',dict(operation='open',
            deadline_ms=int(time.time()*1000)+20000, invocation_id=submitted['invocation_id']))
        self.assertEqual(status,204)
        body=dict(model='local-primary',messages=[dict(role='system',content='source\n  indentation'),
            dict(role='user',content='Bearer do-not-persist /srv/private/path api_key=do-not-persist')],
            tools=[dict(type='function',function=dict(name='read',parameters=dict(type='object')))],
            tool_choice='auto')
        self.r.controls('local-primary', response=dict(model='local-primary',choices=[dict(message=dict(
            role='assistant',content='read the file',tool_calls=[dict(id='tool-1',type='function',
            function=dict(name='read',arguments='{"path":"src/lib.rs"}'))]),finish_reason='tool_calls')],
            usage=dict(prompt_tokens=18,completion_tokens=7)))
        status,raw=self.post('/calls/diagnostic-tools/chat/completions',body,'tool-emitter')
        self.assertEqual(status,200,raw)
        # A retry reconciles; it must not allocate a second diagnostic record.
        self.assertEqual(self.post('/calls/diagnostic-tools/chat/completions',body,'tool-emitter')[0],200)
        self.r.controls('local-primary',content='pub fn answer()->i32 { 42 }\n')
        status,raw=self.post('/calls/diagnostic-tools/chat/completions',dict(model='local-primary',
            messages=[dict(role='tool',tool_call_id='tool-1',content='file contents')],max_tokens=16),'tool-result')
        self.assertEqual(status,200,raw)
        self.assertEqual(self.post('/scopes/diagnostic-tools',dict(operation='close'))[0],204)
        marker.with_name('continue-turn').write_text('continue')
        self.fixture.assert_proof(self.fixture.finish('diagnostic-task'),'local-primary')
        report=self.report()
        records=report['model_interactions']['records']
        self.assertEqual(len(records),5,report)
        self.assertEqual([r['sequence'] for r in records],sorted(r['sequence'] for r in records))
        retrieved=[self.r.call('athba','get_work_artifact',artifact_id=r['artifact_id']) for r in records]
        for record in retrieved:
            self.validate(record)
            self.assertEqual(record['identity']['parent_invocation_id'],submitted['invocation_id'])
            self.assertTrue(record['diagnostic_complete'])
            self.assertIsNone(record['timing']['prefill_seconds'])
        tool=next(v for v in retrieved if (v['request']['body'] or {}).get('tools'))
        self.assertEqual(tool['request']['body']['max_tokens'],128)
        self.assertEqual(tool['request']['body']['messages'][0]['content'],'source\n  indentation')
        self.assertNotIn('do-not-persist',json.dumps(tool))
        self.assertNotIn('/srv/private/path',json.dumps(tool))
        self.assertEqual(tool['response']['body']['usage']['completion_tokens'],7)
        self.assertEqual(tool['response']['body']['choices'][0]['finish_reason'],'tool_calls')
        self.assertEqual(tool['response']['body']['choices'][0]['message']['tool_calls'][0]['id'],'tool-1')
        for item in records:self.r.call('cb','get_work_artifact',status=404,artifact_id=item['artifact_id'])
        # Restart when no work is running; retained traces remain owner-readable.
        self.r.process.terminate();self.r.process.wait(timeout=5);self.r.log.close();self.r.start()
        self.assertEqual(self.report()['model_interactions']['records'],records)
        self.r.call('athba','release_reservation',reservation_id=reservation['id'])
        for item in records:self.r.call('athba','get_work_artifact',status=404,artifact_id=item['artifact_id'])
        deadline=time.monotonic()+8
        base=self.r.root/'authority/model-interactions'
        while time.monotonic()<deadline and list(base.glob('*/*.json')):time.sleep(.03)
        self.assertEqual(list(base.glob('*/*.json')),[])
        self.assertNotIn('model_interactions',self.report())
        self.assertEqual(self.r.call('athba','inspect_work',work_id='diagnostic-task')['state'],'completed')
        self.r.process.terminate();self.r.process.wait(timeout=5);self.r.log.close();self.r.start()
        for item in records:self.r.call('athba','get_work_artifact',status=404,artifact_id=item['artifact_id'])
    def test_ordinary_workspace_has_no_diagnostic_documents(self):
        submitted,marker=self.three_turn_task()
        marker.with_name('continue-turn').write_text('continue')
        self.fixture.assert_proof(self.fixture.finish('diagnostic-task'),'local-primary')
        self.assertNotIn('model_interactions',self.report())
        self.assertFalse((self.r.root/'authority/model-interactions').exists())
    def test_optional_storage_failure_does_not_fail_workspace(self):
        self.enabled()
        (self.r.root/'authority/model-interactions').write_text('injected non-directory')
        submitted,marker=self.three_turn_task()
        marker.with_name('continue-turn').write_text('continue')
        self.fixture.assert_proof(self.fixture.finish('diagnostic-task'),'local-primary')
        self.assertEqual(self.report()['model_interactions']['availability'],'storage_unavailable')

if __name__=='__main__':unittest.main(verbosity=2)
