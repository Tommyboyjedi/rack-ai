"""Public HTTP tests. State corruption is limited to disposable authority roots."""
import concurrent.futures
import json
import tempfile
import time
import unittest
from pathlib import Path
from support import Rack, VERSION

class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='rack-pr35-')
        self.rack = Rack(self.directory.name)

    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()

    def test_equal_concurrent_and_opposite_resource_order(self):
        r = self.rack
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            grants = list(pool.map(lambda _: r.acquire('other','big-brain','medium'),range(8)))
        self.assertEqual(sum(d['state']!='denied' for d in grants),1)
        winner = r.wait(next(d for d in grants if d['state']!='denied'))
        self.assertEqual(r.counts('start')['big-brain'],1)
        self.assertEqual(len(winner['resources']),3)

    def test_policy_identity_and_denial_replay(self):
        r = self.rack
        d = r.wait(r.acquire('cb','local-fun-chat','paramount'))
        denied = r.acquire('athba','local-primary','low',identity='denied')
        self.assertEqual(denied['state'],'denied')
        r.release(d); r.wait(d,'released')
        self.assertEqual(r.call('athba','acquire',request=denied['request']),denied)
        changed = dict(denied['request'],priority='medium')
        r.call('athba','acquire',status=409,request=changed)
        r.call('athba','acquire',status=403,request=dict(changed,acquisition_id='spoof',source_system='cb'))
        r.call('athba','acquire',status=403,request=dict(changed,acquisition_id='ceiling',priority='high'))
        r.call('bad-token','discover',status=401)
        fresh = r.wait(r.acquire('athba','local-primary','low'))
        self.assertEqual(r.call('athba','acquire',request=fresh['request'])['id'],fresh['id'])
        r.call('cb','inspect',status=404,reservation_id=fresh['id'])

    def test_hold_cancel_prevents_restoration_and_pending_dispatch(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        chat = r.wait(r.acquire('cb','local-fun-chat','paramount'))
        p = r.wait(p,'held')
        pending = r.infer(p)
        r.release(p,'cancel')
        r.wait(p,'cancelled')
        r.release(chat); r.wait(chat,'released')
        self.assertEqual(r.result(pending,'cancelled')['started'],None)
        self.assertEqual(r.counts('start')['local-primary'],1)
        self.assertEqual(r.counts('dispatch')['local-primary'],0)

    def test_stale_generation_rejected_and_tag_rebinding_is_frozen(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        request = r.infer(p)['request']
        chat = r.wait(r.acquire('cb','local-fun-chat','paramount'))
        r.release(chat)
        restored = r.wait(p)
        r.call('athba','infer',status=409,request=dict(request,submission_id='stale'))
        self.assertNotEqual(restored['generation'],p['generation'])
        self.assertEqual(restored['profile_hash'],p['profile_hash'])

    def test_cancel_and_expire_during_startup(self):
        r = self.rack
        r.controls('local-primary',startup_delay=1)
        p = r.acquire('athba','local-primary','low')
        time.sleep(.25)
        r.release(p,'cancel'); r.wait(p,'cancelled')
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        p = r.acquire('athba','local-primary','low',ttl_seconds=1)
        r.wait(p,'expired')
        self.assertEqual(r.counts('dispatch')['local-primary'],0)

    def test_bounded_drain_keeps_unrelated_dispatch_usable(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        c = r.wait(r.acquire('athba','local-coder','low'))
        r.controls('local-primary',delay=1)
        invocation = r.infer(p)
        r.result(invocation,'started')
        chat = r.acquire('cb','local-fun-chat','paramount')
        r.result(r.infer(c))
        self.assertEqual(r.counts('stop')['local-primary'],0)
        r.result(invocation)
        r.wait(chat)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        self.assertEqual(r.counts('stop')['local-coder'],0)

    def test_uncertain_not_replayed_across_receiver_restart(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-primary',delay=2)
        invocation = r.infer(p,identity='once')
        r.result(invocation,'started')
        deadline = time.monotonic()+3
        while r.counts('dispatch')['local-primary']!=1 and time.monotonic()<deadline:
            time.sleep(.01)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        r.process.kill(); r.process.wait(timeout=5); r.log.close(); r.start()
        i = r.result(invocation,'uncertain')
        replay = r.call('athba','infer',request=i['request'])
        self.assertEqual(replay['id'],i['id'])
        time.sleep(2.2)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        self.assertEqual(r.call('athba','reconcile',invocation_id=i['id'])['state'],'uncertain')

    def test_startup_model_mismatch_restores_displaced_demand(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-fun-chat',model='wrong-model')
        chat = r.acquire('cb','local-fun-chat','paramount')
        r.wait(chat,'released',seconds=12)
        p = r.wait(p)
        self.assertEqual(r.counts('start')['local-primary'],2)
        self.assertEqual(r.counts('dispatch')['local-fun-chat'],0)
        r.result(r.infer(p))

    def test_failed_cleanup_never_announces_takeover(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-primary',ignore_stop=True)
        chat = r.acquire('cb','local-fun-chat','paramount')
        failed = r.wait(chat,'recovery_required')
        self.assertIn('cleanup_unproven',failed['reason'])
        self.assertEqual(r.counts('start')['local-fun-chat'],0)
        self.assertNotEqual(r.inspect(p)['state'],'held')

    def test_corrupt_authority_prevents_dispatch(self):
        r = self.rack
        p = r.wait(r.acquire('athba','local-primary','low'))
        r.process.kill(); r.process.wait(timeout=5); r.log.close()
        state = r.root/'authority/managed.json'
        state.write_text('{corrupt')
        with self.assertRaises(AssertionError):
            r.start()
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        self.assertEqual(state.read_text(),'{corrupt')

    def restart(self):
        r=self.rack
        r.process.terminate(); r.process.wait(timeout=5); r.log.close(); r.start()

    def test_opposite_resource_order_alias_and_frozen_profile_after_restart(self):
        r=self.rack
        import copy
        alias=copy.deepcopy(r.config['profiles'][-1]); alias['tag']='reverse'; alias['resources'].reverse()
        r.config['profiles'].append(alias)
        r.config['sources'][-1]['tags'].append('reverse')
        self.restart()
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            futures=[pool.submit(r.acquire,'other',tag,'medium') for tag in ['big-brain','reverse']]
            grants=[f.result() for f in futures]
        self.assertEqual(sum(g['state']!='denied' for g in grants),1)
        winner=r.wait(next(g for g in grants if g['state']!='denied'))
        original=winner['profile_hash']
        next(p for p in r.config['profiles'] if p['tag']==winner['request']['tag'])['version']='rebound-version'
        self.restart()
        self.assertEqual(r.inspect(winner)['profile_hash'],original)
        self.assertEqual(r.inspect(winner)['profile_version'],'fixture-v1')
        r.config['max_ttl_seconds']=10
        self.restart()
        self.assertEqual(r.call(winner['owner'],'acquire',request=winner['request'])['id'],winner['id'])

    def test_unqualified_path_and_per_tag_priority_are_source_authorized(self):
        r=self.rack
        r.config['profiles'][-1]['qualified']=False
        r.config['sources'][0]['tag_priorities']={'big-brain':['medium']}
        self.restart()
        denied=r.acquire('athba','big-brain','medium')
        self.assertEqual(denied['reason'],'unqualified_profile')
        r.call('athba','acquire',status=403,request=dict(denied['request'],acquisition_id='qualify',qualification=True))
        r.call('athba','acquire',status=403,request=dict(denied['request'],acquisition_id='low',priority='low'))
        d=r.wait(r.acquire('other','big-brain','medium',qualification=True))
        r.result(r.infer(d))

    def test_insufficient_reserved_host_memory_denies_without_mutation(self):
        r=self.rack
        r.config['host_capacity_mib']=50
        self.restart()
        p=r.wait(r.acquire('athba','local-primary','low'))
        d=r.acquire('athba','local-coder','low')
        self.assertEqual(d['state'],'denied')
        self.assertEqual(d['reason'],'insufficient_host_memory')
        self.assertEqual(r.inspect(p)['generation'],p['generation'])
        self.assertEqual(r.counts('stop')['local-primary'],0)

    def test_cancel_takeover_during_drain_restores_original_owner(self):
        r=self.rack
        p=r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-primary',delay=1)
        invocation=r.infer(p); r.result(invocation,'started')
        chat=r.acquire('cb','local-fun-chat','paramount')
        r.release(chat,'cancel'); r.wait(chat,'cancelled')
        r.result(invocation)
        restored=r.wait(p)
        self.assertEqual(restored['generation'],p['generation'])
        self.assertEqual(r.counts('start')['local-fun-chat'],0)
        self.assertEqual(r.counts('stop')['local-primary'],0)

    def test_partial_persistence_failure_does_not_start_new_runtime(self):
        r=self.rack
        p=r.wait(r.acquire('athba','local-primary','low'))
        authority=r.root/'authority'
        before=(authority/'managed.json').read_bytes()
        authority.chmod(0o500)
        try:
            request=dict(p['request'],acquisition_id='disk-fail',tag='local-coder')
            r.call('athba','acquire',status=400,request=request)
            self.assertEqual((authority/'managed.json').read_bytes(),before)
            self.assertEqual(r.counts('start')['local-coder'],0)
        finally:
            authority.chmod(0o700)
        r.result(r.infer(r.wait(p)))

    def test_simulated_reboot_or_stale_pid_quarantines_ownership(self):
        r=self.rack
        p=r.wait(r.acquire('athba','local-primary','low'))
        r.process.terminate(); r.process.wait(timeout=5); r.log.close()
        path=r.root/'authority/managed.json'
        state=json.loads(path.read_text())
        state['data']['demands'][p['id']]['process']['boot']='prior-boot'
        path.write_text(json.dumps(state))
        r.start()
        r.wait(p,'recovery_required')
        denial=r.acquire('cb','local-fun-chat','paramount')
        self.assertEqual(denial['state'],'denied')
        self.assertEqual(r.counts('start')['local-fun-chat'],0)

    def test_expired_held_demand_never_resurrects(self):
        r=self.rack
        p=r.wait(r.acquire('athba','local-primary','low',ttl_seconds=3))
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        r.wait(p,'expired')
        r.release(chat);r.wait(chat,'released')
        self.assertEqual(r.counts('start')['local-primary'],1)
        self.assertEqual(r.inspect(p)['state'],'expired')

    def test_expired_takeover_during_drain_preserves_started_invocation(self):
        r=self.rack
        p=r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-primary',delay=2)
        invocation=r.infer(p);r.result(invocation,'started')
        chat=r.acquire('cb','local-fun-chat','paramount',ttl_seconds=1)
        r.wait(chat,'expired')
        r.result(invocation)
        self.assertEqual(r.wait(p)['generation'],p['generation'])
        self.assertEqual(r.counts('start')['local-fun-chat'],0)

    def test_late_readiness_failure_cannot_overwrite_release(self):
        r=self.rack
        p=r.wait(r.acquire('athba','local-primary','low'))
        baseline=r.counts('probe')['local-primary']
        r.controls('local-primary',probe_delay=1,model='wrong-model')
        deadline=time.monotonic()+3
        while r.counts('probe')['local-primary']<=baseline and time.monotonic()<deadline:
            time.sleep(.01)
        self.assertGreater(r.counts('probe')['local-primary'],baseline)
        r.release(p); r.wait(p,'released')
        time.sleep(.2)
        self.assertEqual(r.inspect(p)['state'],'released')
        self.assertEqual(r.counts('stop')['local-primary'],1)

if __name__=='__main__':
    unittest.main(verbosity=2)
