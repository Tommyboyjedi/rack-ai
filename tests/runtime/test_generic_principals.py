"""Caller-owned priority and uniform published-service access through the real receiver."""
import copy
import tempfile
import unittest
from pathlib import Path
from support import Rack

class Principals(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='rack-generic-principals-')
        def configure(c):
            c['profiles'][-1]['qualified'] = False
        self.rack = Rack(Path(self.directory.name), configure=configure)
    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()
    def test_all_priorities_for_both_principals(self):
        r = self.rack
        for owner in ['athba', 'cb']:
            for priority in ['low', 'medium', 'high', 'paramount']:
                d = r.wait(r.acquire(owner, 'local-primary', priority))
                self.assertEqual(d['priority'], priority)
                r.release(d)
                r.wait(d, 'released')
    def test_discovery_and_qualification_are_uniform(self):
        r = self.rack
        a = r.call('athba', 'discover')
        self.assertEqual(a, r.call('cb', 'discover'))
        self.assertEqual(len(a['tags']), 5)
        self.assertEqual(a['priorities'], ['low','medium','high','paramount'])
        for owner in ['athba','cb']:
            d = r.acquire(owner, 'big-brain', 'paramount')
            self.assertEqual(d['state'], 'unavailable')
            self.assertEqual(d['reason'], 'unqualified_profile')
    def test_ties_independent_paramount_and_strict_preemption(self):
        r = self.rack
        low = r.wait(r.acquire('athba', 'local-primary', 'low'))
        high = r.wait(r.acquire('cb', 'local-fun-chat', 'paramount'))
        r.wait(low, 'preempted')
        denied = r.acquire('athba', 'local-primary', 'paramount', identity='tie')
        self.assertEqual(denied['reason'], 'incumbent_priority:'+high['id'])
        independent = r.wait(r.acquire('athba', 'local-coder', 'paramount'))
        self.assertEqual(r.inspect(high)['state'], 'ready')
        r.release(high)
        r.wait(high, 'released')
        self.assertEqual(r.inspect(low)['state'], 'preempted')
        # A changed claim environment invalidates the short denial cache. This is
        # a new client decision; RackAI never restores the displaced record.
        fresh = r.wait(r.acquire('athba', 'local-primary', 'paramount', identity='tie'))
        self.assertNotEqual(fresh['id'], low['id'])
        self.assertEqual(r.inspect(independent)['state'], 'ready')
    def test_ownership_and_qualification_operator_remain(self):
        r = self.rack
        d = r.wait(r.acquire('athba','local-primary','high'))
        r.call('cb','inspect',status=404,reservation_id=d['id'])
        r.call('cb','control',status=404,reservation_id=d['id'],request=dict(generation=d['generation'],action=dict(kind='cancel')))
        request = copy.deepcopy(d['request'])
        request['acquisition_id'] = 'spoof'
        r.call('cb','acquire',status=403,request=request)
        request.update(source_system='cb', qualification=True, tag='big-brain')
        r.call('cb','acquire',status=403,request=request)

if __name__ == '__main__': unittest.main(verbosity=2)
