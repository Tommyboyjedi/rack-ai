"""Isolated real receiver/process checks supplement the simulated-time Rust tests."""
import tempfile
import time
import unittest
from support import Rack

class IdleRuntimeTests(unittest.TestCase):
    def test_renewal_polling_do_not_prevent_automatic_cleanup(self):
        with tempfile.TemporaryDirectory(prefix="rack-idle-http-") as root:
            def configure(c):c["idle_timeout_seconds"]=2
            r=Rack(root,configure=configure)
            try:
                d=r.wait(r.acquire("cb","local-primary","paramount"))
                r.call("cb","control",reservation_id=d["id"],request=dict(generation=d["generation"],action=dict(kind="renew",ttl_seconds=60)))
                expired=r.wait(d,"expired")
                self.assertEqual(expired["reason"],"idle_timeout")
                self.assertTrue(expired["released"])
                self.assertIsNone(expired["last_activity_at"])
                self.assertEqual(r.counts("stop")["local-primary"],1)
                fresh=r.acquire("athba","local-primary","medium")
                self.assertEqual(fresh["state"],"preparing")
                r.release(fresh)
            finally:r.close()

    def test_started_inference_survives_idle_threshold_then_releases(self):
        with tempfile.TemporaryDirectory(prefix="rack-idle-running-") as root:
            def configure(c):c["idle_timeout_seconds"]=2
            r=Rack(root,configure=configure)
            try:
                r.controls("local-primary",delay=3)
                d=r.wait(r.acquire("cb","local-primary","paramount"))
                i=r.infer(d)
                completed=r.result(i)
                self.assertEqual(completed["state"],"completed")
                current=r.inspect(d)
                self.assertFalse(current["released"])
                self.assertGreaterEqual(current["last_activity_at"],completed["started"])
                expired=r.wait(d,"expired")
                self.assertEqual(expired["reason"],"idle_timeout")
                self.assertEqual(r.counts("dispatch")["local-primary"],1)
            finally:r.close()
