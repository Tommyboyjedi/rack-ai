"""Exercise durable scope control and races through the managed compatibility gateway."""
import json,socket,tempfile,time,unittest,urllib.request,urllib.error
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from support import Rack

class WorkspaceScopes(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory(prefix='rack-pr35-scopes-')
        def configure(config):
            if self._testMethodName=='test_failed_scoped_call_replays_promptly_and_next_call_succeeds':
                config['limits']=dict(max_response_bytes=4096)
        self.r=Rack(self.directory.name,configure=configure)
        self.p=self.r.wait(self.r.acquire('athba','local-primary','low'))
        self.base=f'http://{self.r.address}'+self.p['gateway_path']
    def tearDown(self):self.r.close();self.directory.cleanup()
    def post(self,path,body):
        req=urllib.request.Request(self.base+path,data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
        try:response=urllib.request.urlopen(req,timeout=10)
        except urllib.error.HTTPError as error:response=error
        with response:return response.status,response.read().decode()
    def open(self,namespace='one',seconds=10):
        deadline=int(time.time()*1000+seconds*1000)
        self.assertEqual(self.post('/scopes/'+namespace,dict(operation='open',deadline_ms=deadline)),(204,''))
        return deadline
    def close_scope(self,namespace='one'):return self.post('/scopes/'+namespace,dict(operation='close'))
    def body(self):return dict(model='local-primary',messages=[dict(role='user',content='scoped')],max_tokens=16)
    def call(self,namespace='one'):return self.post('/calls/'+namespace+'/chat/completions',self.body())
    def data(self):return json.loads((self.r.root/'authority/managed.json').read_text())['data']
    def wait_invocation(self,state='queued'):
        deadline=time.monotonic()+4
        while time.monotonic()<deadline:
            for i in self.data()['invocations'].values():
                if i['request'].get('workspace_scope') and i['state']==state:return i
            time.sleep(.02)
        self.fail('exact scoped invocation did not reach '+state)
    def hold(self):
        chat=self.r.wait(self.r.acquire('cb','local-fun-chat','paramount'));self.r.wait(self.p,'held');return chat
    def test_scope_deadline_does_not_replace_per_call_waiting_limits(self):
        self.open(seconds=900)
        self.assertEqual(self.close_scope(),(204,''))
        self.assertEqual(self.call()[0],409)
        self.assertEqual(self.data()['invocations'],{})

    def test_close_replays_and_preserves_unrelated_shared_reservation_work(self):
        self.r.controls('local-primary',delay=1)
        blocker=self.r.infer(self.p,submission_id='scope-blocker')
        self.r.result(blocker,'running')
        deadline=self.open()
        with ThreadPoolExecutor() as pool:
            pending=pool.submit(self.call);i=self.wait_invocation()
            unrelated=self.r.infer(self.p,submission_id='unrelated')
            self.assertEqual(self.close_scope(),(204,''));first=self.data()['invocations'][i['id']]
            self.assertEqual(self.close_scope(),(204,''));self.assertEqual(first,self.data()['invocations'][i['id']])
            self.assertEqual(self.post('/scopes/one',dict(operation='open',deadline_ms=deadline)),(204,''))
            self.assertEqual(pending.result()[0],409)
            self.assertEqual(self.call()[0],409)
            self.assertEqual(self.r.result(blocker)['state'],'completed')
            self.assertEqual(self.r.result(unrelated)['state'],'completed')
            self.assertEqual(self.r.counts('dispatch')['local-primary'],2)
            self.assertEqual(self.r.result(i,'cancelled')['state'],'cancelled')
            self.assertFalse(self.r.inspect(self.p)['released'])

    def test_delayed_http_submission_cannot_cross_closed_or_expired_scope(self):
        for namespace,expire in [('closed',False),('expired',True)]:
            with self.subTest(namespace=namespace):
                deadline=self.open(namespace,seconds=.7 if expire else 5)
                host,port=self.r.address.split(':');stream=socket.create_connection((host,int(port)),timeout=3)
                body=json.dumps(self.body()).encode();path=self.p['gateway_path']+'/calls/'+namespace+'/chat/completions'
                stream.sendall(f'POST {path} HTTP/1.1\r\nHost: {self.r.address}\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n'.encode()+body[:1])
                if expire:time.sleep(max(0,deadline/1000-time.time())+.1)
                else:self.assertEqual(self.close_scope(namespace),(204,''))
                stream.sendall(body[1:]);response=b''
                while chunk:=stream.recv(4096):response+=chunk
                stream.close();self.assertIn(b'409 Conflict',response);self.assertIn(b'workspace_scope_closed_or_unknown',response)
        self.assertEqual(self.data()['invocations'],{});self.assertEqual(self.r.counts('dispatch')['local-primary'],0)
    def test_failed_scoped_call_replays_promptly_and_next_call_succeeds(self):
        self.open('oversized')
        self.r.controls('local-primary', content='x'*10000)
        started=time.monotonic(); status,error=self.call('oversized')
        self.assertEqual(status,409); self.assertIn('invocation_Failed',error)
        self.assertLess(time.monotonic()-started,2.0)
        self.assertEqual(self.r.counts('dispatch')['local-primary'],1)
        replay_started=time.monotonic(); status,error=self.call('oversized')
        self.assertEqual(status,409); self.assertIn('invocation_Failed',error)
        self.assertLess(time.monotonic()-replay_started,1.0)
        self.assertEqual(self.r.counts('dispatch')['local-primary'],1)
        self.open('after-failed')
        self.r.controls('local-primary', content='scoped-ok')
        status,body=self.call('after-failed')
        self.assertEqual(status,200); self.assertIn('scoped-ok',body)
        self.assertEqual(self.r.counts('dispatch')['local-primary'],2)

    def test_temporary_disconnect_reconciles_and_legitimate_work_continues(self):
        self.r.controls('local-primary',delay=1)
        blocker=self.r.infer(self.p,submission_id='disconnect-blocker')
        self.r.result(blocker,'running')
        self.open()
        req=urllib.request.Request(self.base+'/calls/one/chat/completions',data=json.dumps(self.body()).encode(),headers={'Content-Type':'application/json'})
        with self.assertRaises(TimeoutError):urllib.request.urlopen(req,timeout=.3)
        i=self.wait_invocation();self.assertIsNone(i['cancellation'])
        self.assertEqual(self.r.result(blocker)['state'],'completed')
        self.assertEqual(self.r.result(i)['state'],'completed')
        self.assertEqual(self.call()[0],200);self.assertEqual(self.r.counts('dispatch')['local-primary'],2)

    def test_close_during_started_call_retains_one_late_success(self):
        self.open();self.r.controls('local-primary',delay=1.5)
        with ThreadPoolExecutor() as pool:
            response=pool.submit(self.call);i=self.wait_invocation('running')
            self.assertEqual(self.close_scope(),(204,''));self.assertEqual(self.close_scope(),(204,''))
            self.assertEqual(response.result()[0],409)
        actual=self.r.result(i,'cancelled')
        self.assertIsNotNone(actual['cancellation']);self.assertIsNone(actual['result']);self.assertIsNotNone(actual['late_result'])
        self.assertEqual(self.r.counts('dispatch')['local-primary'],1)
    def test_close_during_readiness_probe_prevents_dispatch(self):
        self.open();self.r.controls('local-primary',probe_delay=1)
        with ThreadPoolExecutor() as pool:
            response=pool.submit(self.call);i=self.wait_invocation();time.sleep(.2)
            self.assertEqual(self.close_scope(),(204,''));self.assertEqual(response.result()[0],409)
        time.sleep(1.2)
        self.assertIsNone(self.data()['invocations'][i['id']]['started']);self.assertEqual(self.r.counts('dispatch')['local-primary'],0)
    def test_close_storage_failure_is_explicit_and_scope_survives_restart(self):
        self.r.controls('local-primary',delay=3)
        blocker=self.r.infer(self.p,submission_id='storage-blocker')
        self.r.result(blocker,'running')
        deadline=self.open(seconds=10)
        with ThreadPoolExecutor() as pool:
            response=pool.submit(self.call);i=self.wait_invocation();root=self.r.root/'authority'
            root.chmod(0o500)
            try:
                status,error=self.close_scope();self.assertEqual(status,409);self.assertIn('workspace_scope_persistence_or_control_failed',error)
                self.assertEqual(self.data()['workspace_scopes'][next(iter(self.data()['workspace_scopes']))]['deadline_ms'],deadline)
            finally:root.chmod(0o700)
            self.r.process.kill();self.r.process.wait();self.r.log.close();self.r.start()
            self.assertEqual(self.close_scope(),(204,''))
            self.assertEqual(self.r.result(i,'cancelled')['state'],'cancelled')
            try:self.assertEqual(response.result()[0],409)
            except ConnectionError:pass
            self.assertEqual(self.r.counts('dispatch')['local-primary'],1)
            self.assertFalse(self.r.inspect(self.p)['released'])

if __name__=='__main__':unittest.main(verbosity=2)
