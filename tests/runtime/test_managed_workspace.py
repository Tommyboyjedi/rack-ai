"""Reserved workspace work through managed HTTP, real sandboxing and acceptance."""
import copy,json,os,subprocess,tempfile,time,unittest,urllib.request,urllib.error
from pathlib import Path
from support import Rack,ROOT

class ManagedWorkspace(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory(prefix='rack-pr35-workspace-')
        self.root=Path(self.directory.name)
        def configure(c):
            c['limits']=dict(max_wait_seconds=45,max_response_bytes=3*1024*1024,retention_admission_bytes=30*1024*1024)
            for p in c['profiles']:
                p['inference_seconds']=1
                if p['tag'] in ['local-primary','local-coder']:
                    p['streaming']=True
            next(p for p in c['profiles'] if p['tag']=='local-coder')['capabilities']=['coding']
        self.rack=Rack(self.root/'runtime',configure=configure)
        self.fixture=self.root/'fixture';(self.fixture/'src').mkdir(parents=True)
        (self.fixture/'Cargo.toml').write_text('[package]\nname="managed-proof"\nversion="0.1.0"\nedition="2021"\n')
        (self.fixture/'src/lib.rs').write_text('pub fn answer()->i32 { 0 }\n')
        (self.fixture/'tests').mkdir();(self.fixture/'tests/acceptance.rs').write_text('#[test] fn answer(){assert_eq!(managed_proof::answer(),42);}\n')
        self.git('init','-b','main');self.git('config','user.email','rackai-proof@example.invalid');self.git('config','user.name','RackAI test')
        subprocess.run(['cargo','generate-lockfile','--offline'],cwd=self.fixture,check=True,capture_output=True)
        self.git('add','.');self.git('commit','-m','fixture');self.base=self.git('rev-parse','HEAD').strip()
        self.registry=self.root/'registry';(self.registry/'config').mkdir(parents=True)
        subprocess.run(['git','-C',str(self.registry),'init','-b','main'],check=True,capture_output=True)
        self.write('repositories',dict(workspace_root=str(self.root/'workspaces'),executor=dict(backend='podman',image='docker.io/library/rust:bookworm'),repositories=[dict(id='proof',root=str(self.fixture))]))
        self.workers=json.loads((ROOT/'config/workers.json').read_text())
        for worker in self.workers['workers']:worker['entrypoint']=str(ROOT/'tests/runtime/workspace_harness.py')
        self.write('workers',self.workers);self.write('resources',json.loads((ROOT/'config/resources.json').read_text()))
        self.models=json.loads((ROOT/'config/models.json').read_text())
        self.p=self.rack.wait(self.rack.acquire('athba','local-primary','low'))
        self.c=self.rack.wait(self.rack.acquire('athba','local-coder','low'))
        self.write('models',self.models)
        self.rack.process.terminate();self.rack.process.wait(timeout=5);self.rack.log.close()
        self.rack.config['workspace']=dict(registry_root=str(self.registry),state_root=str(self.registry))
        self.rack.start()
        for tag in ['local-primary','local-coder']:self.rack.controls(tag,content='pub fn answer()->i32 { 42 }\n')

    def tearDown(self):
        self.rack.close();self.directory.cleanup()
    def git(self,*args):
        return subprocess.run(['git','-C',str(self.fixture),*args],check=True,capture_output=True,text=True).stdout
    def write(self,name,value):(self.registry/'config'/f'{name}.json').write_text(json.dumps(value))
    def spec(self,identity,capability='reasoning'):
        service='local-coder' if capability=='coding' else 'local-primary'
        reservation=self.c if service=='local-coder' else self.p
        return dict(reservation_id=reservation['id'],service=service,work_id=identity,payload=dict(kind='workspace',workspace=dict(
            repository=dict(id='proof',base_ref='main',base_sha=self.base),objective='Make answer return 42.',
            allowed_paths=['src/'],acceptance=dict(commands=[['cargo','test','--offline']],required_artifacts=['src/lib.rs']),
            requirements=dict(complexity='small',requires_large_context=False),
            limits=dict(max_implementation_attempts=1,timeout_seconds=45,network='disabled'))))
    def launch(self,work):
        self.rack.call('athba','submit_work',request=work)
        return work['work_id']
    def finish(self,identity,success=True):
        end=time.monotonic()+65
        while time.monotonic()<end:
            state=self.rack.call('athba','inspect_work',work_id=identity)
            if state['state'] in ['completed','cancelled','expired','uncertain']:
                result=state.get('result') or state.get('late_result') or dict(error=state.get('error'))
                self.assertEqual(result.get('acceptance_verdict')=='approved',success,state)
                return result
            time.sleep(.05)
        self.fail(state)
    def invocations(self):
        return json.loads((self.rack.root/'authority/managed.json').read_text())['data']['invocations']
    def assert_proof(self,result,worker):
        self.assertEqual(result['acceptance_verdict'],'approved',result)
        self.assertEqual(result['selected_worker_id'],worker)
        self.assertEqual(result['worker_provenance']['worker_id'],worker)
        expected=next(model for model in self.models['models'] if model['worker_id']==worker)
        self.assertEqual(result['worker_provenance']['model_id'],expected['id'])
        self.assertEqual(result['worker_provenance']['provider_profile'],worker)
        reservation=self.p if worker=='local-primary' else self.c
        self.assertIn(result['worker_provenance']['resource_id'],reservation['resources'])
        packet=json.loads(Path(result['packet_path']).read_text())
        self.assertEqual(packet['worker_provenance'],result['worker_provenance'])
        self.assertEqual(packet['selection_decision']['selected_worker_id'],worker)
        self.assertIsNotNone(result['accepted_revision'])
        self.assertEqual(subprocess.check_output(['git','-C',result['worktree_path'],'rev-parse','HEAD'],text=True).strip(),result['accepted_revision'])
        self.assertEqual(self.git('rev-parse','HEAD').strip(),self.base)
        self.assertFalse((Path(result['worktree_path'])/'forbidden.txt').exists())
        self.assertIn('PATH_BOUNDARY_ENFORCED',json.dumps(packet))

    def test_reserved_work_uses_managed_access_for_all_turns(self):
        r=self.rack
        # The registry remains its original raw configuration. RackAI must bind
        # access internally for every scoped turn.
        self.models=json.loads((ROOT/'config/models.json').read_text())
        self.write('models',self.models)
        before=(self.registry/'config/models.json').read_bytes()
        r.process.terminate();r.process.wait(timeout=5);r.log.close()
        r.config['workspace']=dict(registry_root=str(self.registry),state_root=str(self.registry))
        r.start()
        (self.fixture/'src/harness-control.json').write_text(json.dumps(dict(three_turns=True)))
        self.git('add','src/harness-control.json');self.git('commit','-m','three bounded turns')
        self.base=self.git('rev-parse','HEAD').strip()
        work=self.spec('reserved')
        accepted=r.call('athba','submit_work',request=work)
        end=time.monotonic()+25
        marker=None
        while time.monotonic()<end:
            marker=next((self.root/'workspaces').rglob('first-turn'),None)
            if marker:break
            state=r.call('athba','inspect_work',work_id='reserved')
            if state['state'] in ['completed','uncertain']:
                packet=state.get('result',{}).get('packet_path')
                self.fail(Path(packet).read_text() if packet else state)
            time.sleep(.05)
        self.assertIsNotNone(marker)
        marker.with_name('continue-turn').write_text('continue')
        end=time.monotonic()+30
        while time.monotonic()<end:
            result=r.call('athba','inspect_work',work_id='reserved')
            if result['state'] in ['completed','uncertain','cancelled','expired']:break
            time.sleep(.05)
        self.assertEqual(result['state'],'completed',result)
        self.assert_proof(result['result'],'local-primary')
        self.assertEqual(r.counts('dispatch')['local-primary'],3)
        self.assertEqual(r.call('athba','submit_work',request=work)['invocation_id'],accepted['invocation_id'])
        self.assertEqual((self.registry/'config/models.json').read_bytes(),before)
        children=[i for i in self.invocations().values() if i['request'].get('workspace_scope')]
        self.assertTrue(children)
        self.assertEqual(len({i['request']['workspace_scope'] for i in children}),1)
        self.assertTrue(all(i['activation']==self.p['generation'] for i in children))
        coder=dict(work,reservation_id=self.c['id'],service='local-coder',work_id='reserved-coder')
        r.call('athba','submit_work',request=coder)
        end=time.monotonic()+25
        while time.monotonic()<end:
            result=r.call('athba','inspect_work',work_id='reserved-coder')
            if result['state'] in ['completed','uncertain','cancelled','expired']:break
            time.sleep(.05)
        self.assertEqual(result['state'],'completed',result)
        self.assert_proof(result['result'],'local-coder')
        self.assertEqual((self.registry/'config/models.json').read_bytes(),before)

    def test_preempted_workspace_requires_explicit_new_acquisition(self):
        r=self.rack
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        preempted=r.wait(self.p,'preempted')
        self.assertEqual(preempted['preempted_by'],chat['id'])
        request=self.spec('preempted')
        r.call('athba','submit_work',status=409,request=request)
        self.assert_proof(self.finish(self.launch(self.spec('coder','coding'))),'local-coder')
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        r.release(chat)
        time.sleep(.2)
        self.assertEqual(r.inspect(self.p)['state'],'preempted')
        replacement=r.wait(r.acquire('athba','local-primary','low',identity='explicit-reacquire'))
        retry=self.spec('reacquired')
        retry['reservation_id']=replacement['id']
        self.assert_proof(self.finish(self.launch(retry)),'local-primary')
        self.assertEqual(r.counts('dispatch')['local-primary'],1)

    def test_identical_workspace_requests_have_distinct_invocations(self):
        one=self.finish(self.launch(self.spec('identical-one')));self.assert_proof(one,'local-primary')
        two=self.finish(self.launch(self.spec('identical-two')));self.assert_proof(two,'local-primary')
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],2)

    def test_revision_path_acceptance_and_timeout_protections(self):
        wrong=self.spec('revision');wrong['payload']['workspace']['repository']['base_sha']='0'*40
        self.rack.call('athba','submit_work',status=400,request=wrong)
        traversal=self.spec('path');traversal['payload']['workspace']['allowed_paths']=['../']
        self.rack.call('athba','submit_work',status=400,request=traversal)
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],0)
        self.rack.controls('local-primary',content='pub fn answer()->i32 { 17 }\n')
        rejected=self.finish(self.launch(self.spec('acceptance')),False)
        self.assertNotEqual(rejected.get('acceptance_verdict'),'approved');self.assertIsNone(rejected.get('accepted_revision'))
        self.rack.controls('local-primary',delay=3)
        timed=self.spec('timeout');timed['payload']['workspace']['limits']['timeout_seconds']=1
        result=self.finish(self.launch(timed),False)
        self.assertNotEqual(result.get('acceptance_verdict'),'approved')


if __name__=='__main__':unittest.main(verbosity=2)
