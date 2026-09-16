"""Positive proof through work-unit CLI, real bubblewrap/bridge, managed HTTP, and Podman checks."""
import copy,json,os,subprocess,tempfile,time,unittest,urllib.request,urllib.error
from pathlib import Path
from support import Rack,ROOT

class ManagedWorkspace(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory(prefix='rack-pr35-workspace-')
        self.root=Path(self.directory.name);self.children=[]
        def configure(c):
            c['limits']=dict(max_wait_seconds=45)
            for p in c['profiles']:p['inference_seconds']=1
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
        self.bind(self.p);self.bind(self.c)
        for tag in ['local-primary','local-coder']:self.rack.controls(tag,content='pub fn answer()->i32 { 42 }\n')

    def tearDown(self):
        for child in self.children:
            if child.poll() is None:child.kill();child.wait()
        self.rack.close();self.directory.cleanup()
    def git(self,*args):
        return subprocess.run(['git','-C',str(self.fixture),*args],check=True,capture_output=True,text=True).stdout
    def write(self,name,value):(self.registry/'config'/f'{name}.json').write_text(json.dumps(value))
    def bind(self,d):
        model=next(m for m in self.models['models'] if m['worker_id']==d['request']['tag'])
        model['endpoint']=f'http://{self.rack.address}'+d['gateway_path'];model['port']=int(self.rack.address.split(':')[-1])
        self.write('models',self.models)
    def spec(self,identity,capability='reasoning'):
        return dict(version='rack-ai/work-unit/v2',workload=dict(id='rackai-proof',kind='application-development'),repository=dict(id='proof',base_ref='main',base_sha=self.base),
            work_unit=dict(id=identity,objective='Make answer return 42.',allowed_paths=['src/'],acceptance=dict(commands=[['cargo','test','--offline']],required_artifacts=['src/lib.rs']),
                requirements=dict(complexity='small',requires_large_context=False),limits=dict(max_implementation_attempts=1,timeout_seconds=45,network='disabled'),
                routing=dict(source_system='rackai-proof',work_id=identity,submission_id=identity,idempotency_key=identity,required_capabilities=[capability],priority='low')))
    def launch(self,spec):
        path=self.root/(spec['work_unit']['id']+'.json');path.write_text(json.dumps(spec))
        env=dict(os.environ,RACK_AI_RESOURCE_ROOT=str(self.rack.root/'authority'))
        child=subprocess.Popen([str(ROOT/'target/debug/rack_ai_cli'),'work-unit',str(path),'--emit-json','--repo-root',str(self.registry),'--state-root',str(self.registry)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env)
        self.children.append(child);return child
    def finish(self,child,success=True):
        out,err=child.communicate(timeout=65)
        self.assertEqual(child.returncode,0 if success else 1,(out,err))
        return json.loads(out) if out.strip().startswith('{') else dict(error=err)
    def invocations(self):
        return json.loads((self.rack.root/'authority/managed.json').read_text())['data']['invocations']
    def wait_pending(self):
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            for child in self.children:
                if child.poll() is not None:
                    out,err=child.communicate();self.fail(f'workspace exited early: {out} {err}')
            found=[i for i in self.invocations().values() if i['request']['reservation_id']==self.p['id']]
            if found:return found[0]
            time.sleep(.05)
        self.fail('workspace harness never submitted managed invocation')
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

    def test_reserved_work_uses_managed_access_for_all_turns_after_restoration(self):
        r=self.rack
        # The registry remains its original raw configuration. RackAI must bind access internally.
        self.models=json.loads((ROOT/'config/models.json').read_text())
        self.write('models',self.models)
        before=(self.registry/'config/models.json').read_bytes()
        r.process.terminate();r.process.wait(timeout=5);r.log.close()
        r.config['workspace']=dict(registry_root=str(self.registry),state_root=str(self.registry))
        r.start()
        (self.fixture/'src/harness-control.json').write_text(json.dumps(dict(three_turns=True)))
        self.git('add','src/harness-control.json');self.git('commit','-m','three bounded turns')
        self.base=self.git('rev-parse','HEAD').strip()
        spec=self.spec('reserved')
        payload={key:spec['work_unit'][key] for key in ['objective','allowed_paths','acceptance','requirements','limits']}
        payload['repository']=spec['repository']
        work=dict(reservation_id=self.p['id'],service='local-primary',work_id='reserved',payload=dict(kind='workspace',workspace=payload))
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
        contender=r.wait(r.acquire('cb','local-fun-chat','paramount'));r.wait(self.p,'held')
        marker.with_name('continue-turn').write_text('continue')
        end=time.monotonic()+8
        while time.monotonic()<end:
            pending=[i for i in self.invocations().values() if i['request'].get('workspace_scope') and i['state']=='accepted']
            if pending:break
            time.sleep(.04)
        self.assertTrue(pending)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        r.release(contender);restored=r.wait(self.p)
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
        self.assertEqual(len({i['request']['workspace_scope'] for i in children}),1)
        self.assertEqual(sum(i['activation']==restored['generation'] for i in children),2)
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


    def test_held_workspace_restores_once_coder_remains_usable_and_identity_is_scoped(self):
        r=self.rack;chat=r.wait(r.acquire('cb','local-fun-chat','paramount'));r.wait(self.p,'held')
        child=self.launch(self.spec('held'));pending=self.wait_pending()
        self.assertEqual(pending['state'],'accepted');self.assertIsNone(pending['started'])
        time.sleep(2);self.assertIsNone(child.poll());self.assertEqual(r.counts('dispatch')['local-primary'],0)
        coder=self.finish(self.launch(self.spec('coder','coding')));self.assert_proof(coder,'local-coder')
        self.assertIsNone(child.poll());r.release(chat);restored=r.wait(self.p)
        result=self.finish(child);self.assert_proof(result,'local-primary')
        actual=self.invocations()[pending['id']]
        self.assertEqual(actual['state'],'completed');self.assertEqual(actual['activation'],restored['generation'])
        self.assertEqual(actual['result']['rack_protocol_response']['body'] is not None,True)
        response=json.loads(actual['result']['rack_protocol_response']['body'])
        self.assertEqual(response['id'],actual['activation']);self.assertEqual(response['model'],'local-primary')
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        # A fresh logical workspace with identical model payload must execute separately.
        self.bind(restored);second=self.finish(self.launch(self.spec('identical')));self.assert_proof(second,'local-primary')
        self.assertEqual(r.counts('dispatch')['local-primary'],2)
        self.finish(self.launch(self.spec('identical')),False)
        self.assertEqual(r.counts('dispatch')['local-primary'],2)
        # An old published capability cannot start a new workspace model call.
        self.bind(self.p);stale=self.finish(self.launch(self.spec('stale')),False)
        self.assertNotEqual(stale.get('acceptance_verdict'),'approved')
        self.assertEqual(r.counts('dispatch')['local-primary'],2)

    def test_workspace_timeout_cancels_exact_pending_call_before_restoration(self):
        r=self.rack;chat=r.wait(r.acquire('cb','local-fun-chat','paramount'));r.wait(self.p,'held')
        spec=self.spec('authoritative-timeout');spec['work_unit']['limits']['timeout_seconds']=3
        child=self.launch(spec);pending=self.wait_pending()
        self.assertEqual(pending['state'],'accepted');self.assertIsNone(pending['started'])
        self.assertEqual(len(self.invocations()),1)
        result=self.finish(child,False)
        self.assertNotEqual(result.get('acceptance_verdict'),'approved')
        self.assertIn('wall-clock timeout exceeded',Path(result['packet_path']).read_text())
        self.assertGreater(pending['waiting_deadline'],time.time())
        self.assertGreater(r.inspect(self.p)['deadline'],time.time())
        r.release(chat);r.wait(self.p);time.sleep(2)
        actual=self.invocations()[pending['id']]
        print('timeout propagation evidence:',json.dumps(dict(invocation=actual,dispatches=r.counts('dispatch'))))
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        self.assertEqual(actual['state'],'cancelled');self.assertIsNone(actual['started'])
        self.assertIsNotNone(actual['cancellation'])
        self.assertFalse(r.inspect(self.p)['released'])
        self.assert_proof(self.finish(self.launch(self.spec('unrelated-coder','coding'))),'local-coder')

    def test_workspace_reports_cancellation_persistence_failure_without_late_dispatch(self):
        r=self.rack;chat=r.wait(r.acquire('cb','local-fun-chat','paramount'));r.wait(self.p,'held')
        spec=self.spec('timeout-storage-failure');spec['work_unit']['limits']['timeout_seconds']=3
        child=self.launch(spec);pending=self.wait_pending()
        self.assertEqual(pending['state'],'accepted');self.assertIsNone(pending['started'])
        authority=r.root/'authority';authority.chmod(0o500)
        try:
            result=self.finish(child,False)
            packet=Path(result['packet_path']).read_text()
            self.assertIn('wall-clock timeout exceeded',packet)
            self.assertIn('workspace scope control persistence unconfirmed',packet)
            self.assertEqual(self.invocations()[pending['id']]['state'],'accepted')
        finally:authority.chmod(0o700)
        r.release(chat);r.wait(self.p);time.sleep(1)
        self.assertEqual(r.result(pending,'cancelled')['state'],'cancelled')
        self.assertEqual(r.counts('dispatch')['local-primary'],0);self.assertFalse(r.inspect(self.p)['released'])

    def test_workspace_timeout_during_actual_dispatch_retains_late_evidence(self):
        # Delay only the disposable harness, then let a real backend finish after the workspace deadline.
        (self.fixture/'src/harness-control.json').write_text(json.dumps(dict(pre_submit_delay=2.3)))
        self.git('add','src/harness-control.json');self.git('commit','-m','delayed synthetic harness')
        self.base=self.git('rev-parse','HEAD').strip()
        self.rack.controls('local-primary',delay=.9,content='pub fn answer()->i32 { 42 }\n')
        spec=self.spec('started-timeout');spec['work_unit']['limits']['timeout_seconds']=3
        child=self.launch(spec);pending=self.wait_pending();deadline=time.monotonic()+2
        while time.monotonic()<deadline and self.rack.counts('dispatch')['local-primary']!=1:time.sleep(.01)
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],1)
        result=self.finish(child,False);self.assertIn('wall-clock timeout exceeded',Path(result['packet_path']).read_text())
        actual=self.rack.result(pending,'cancelled')
        self.assertIsNotNone(actual['started']);self.assertIsNotNone(actual['cancellation'])
        self.assertIsNone(actual['result']);self.assertIsNotNone(actual['late_result'])
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],1)
        self.assertFalse(self.rack.inspect(self.p)['released'])

    def test_identical_workspace_requests_have_distinct_invocations(self):
        one=self.finish(self.launch(self.spec('identical-one')));self.assert_proof(one,'local-primary')
        two=self.finish(self.launch(self.spec('identical-two')));self.assert_proof(two,'local-primary')
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],2)

    def test_revision_path_acceptance_and_timeout_protections(self):
        wrong=self.spec('revision');wrong['repository']['base_sha']='0'*40
        self.finish(self.launch(wrong),False)
        traversal=self.spec('path');traversal['work_unit']['allowed_paths']=['../']
        self.finish(self.launch(traversal),False)
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],0)
        self.rack.controls('local-primary',content='pub fn answer()->i32 { 17 }\n')
        rejected=self.finish(self.launch(self.spec('acceptance')),False)
        self.assertNotEqual(rejected.get('acceptance_verdict'),'approved');self.assertIsNone(rejected.get('accepted_revision'))
        chat=self.rack.wait(self.rack.acquire('cb','local-fun-chat','paramount'));self.rack.wait(self.p,'held')
        timed=self.spec('timeout');timed['work_unit']['limits']['timeout_seconds']=1
        started=time.monotonic();result=self.finish(self.launch(timed),False)
        self.assertLess(time.monotonic()-started,8);self.assertNotEqual(result.get('acceptance_verdict'),'approved')
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],1)
        self.rack.release(chat);self.rack.wait(self.p);time.sleep(1)
        self.assertEqual(self.rack.counts('dispatch')['local-primary'],1)

if __name__=='__main__':unittest.main(verbosity=2)
