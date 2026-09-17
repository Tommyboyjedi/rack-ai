import json
import tempfile
import urllib.request
import urllib.error
import unittest
from pathlib import Path
from support import Rack

class ProtocolTests(unittest.TestCase):
    def test_opaque_payload_replay_tools_stream_and_stale_capability(self):
        with tempfile.TemporaryDirectory(prefix='rack-pr35-protocol-') as root:
            def configure(c):
                c['profiles'][0]['streaming']=True
                chat=next(p for p in c['profiles'] if p['tag']=='local-fun-chat')
                chat['endpoint']=c['profiles'][0]['endpoint']
                chat['args'][1]=c['profiles'][0]['args'][1]
                c['profiles'][0]['protocols']=['chat_completions','responses']
            r = Rack(root,configure=configure)
            try:
                d = r.wait(r.acquire('athba','local-primary','low'))
                body = dict(model='local-primary',messages=[{'role':'user','content':'hi'}],
                    tools=[{'type':'function','function':{'name':'test','parameters':{'type':'object'}}}],max_tokens=16,stream=True)
                url = f'http://{r.address}'+d['gateway_path']+'/chat/completions'
                def call(url, body):
                    request = urllib.request.Request(url,data=json.dumps(body).encode(),headers={'Content-Type':'application/json'})
                    return urllib.request.urlopen(request,timeout=8).read()
                response = call(url,body)
                self.assertIn(b'data: [DONE]',response)
                self.assertEqual(call(url,body),response)
                self.assertEqual(r.counts('dispatch')['local-primary'],1)
                forwarded = json.loads(Path(str(r.events)+'.requests').read_text().splitlines()[0])
                self.assertEqual(forwarded,body)
                replies=json.loads(call(url.replace('/chat/completions','/responses'),dict(model='local-primary',input='hello',max_output_tokens=16)))
                self.assertEqual(replies['status'],'completed')
                unbounded=dict(model='local-primary',messages=[{'role':'user','content':'JCode omits an output bound'}])
                result=json.loads(call(url,unbounded))
                self.assertEqual(result['model'],'local-primary')
                forwarded=json.loads(Path(str(r.events)+'.requests').read_text().splitlines()[-1])
                self.assertEqual(forwarded['max_tokens'],r.config['profiles'][0]['max_output_tokens'])
                chat = r.wait(r.acquire('cb','local-fun-chat','paramount'))
                r.wait(d,'preempted');r.release(chat);r.wait(chat,'released')
                new=r.wait(r.acquire('athba','local-primary','low',identity='protocol-explicit-reacquire'))
                self.assertNotEqual(new['gateway_path'],d['gateway_path'])
                with self.assertRaises(urllib.error.HTTPError) as caught:
                    call(url,dict(body,messages=[{'role':'user','content':'stale'}]))
                self.assertEqual(caught.exception.code,409)
                caught.exception.close()
                self.assertEqual(r.counts('dispatch')['local-fun-chat'],0)
            finally:
                r.close()

    def test_gateway_waiters_are_bounded_and_control_remains_responsive(self):
        from concurrent.futures import ThreadPoolExecutor
        import time
        with tempfile.TemporaryDirectory(prefix='rack-pr35-gateway-pressure-') as root:
            r=Rack(root,configure=lambda c:c.update(limits=dict(max_gateway_waiters=1)))
            try:
                p=r.wait(r.acquire('athba','local-primary','low'));r.controls('local-primary',delay=2)
                url=f'http://{r.address}'+p['gateway_path']+'/chat/completions'
                body=dict(model='local-primary',messages=[dict(role='user',content='bounded')],max_tokens=16)
                def call(key):
                    req=urllib.request.Request(url,data=json.dumps(body).encode(),headers={'Content-Type':'application/json','Idempotency-Key':key})
                    try:response=urllib.request.urlopen(req,timeout=10)
                    except urllib.error.HTTPError as error:response=error
                    with response:return response.status,json.loads(response.read())
                with ThreadPoolExecutor(max_workers=1) as pool:
                    first=pool.submit(call,'one');deadline=time.monotonic()+3
                    invocations={}
                    while time.monotonic()<deadline:
                        invocations=json.loads((Path(root)/'authority/managed.json').read_text())['data']['invocations']
                        if invocations:break
                        time.sleep(.02)
                    self.assertEqual(len(invocations),1)
                    status,error=call('two');self.assertEqual(status,429);self.assertEqual(error['error'],'capacity_gateway_waiters')
                    started=time.monotonic();r.inspect(p)
                    r.call('athba','cancel',invocation_id=next(iter(invocations)))
                    self.assertLess(time.monotonic()-started,2)
                    status,error=first.result(timeout=3);self.assertEqual(status,409);self.assertIn('Cancelled',error['error'])
                self.assertLessEqual(r.counts('dispatch')['local-primary'],1)
            finally:r.close()

if __name__=='__main__':
    unittest.main(verbosity=2)
