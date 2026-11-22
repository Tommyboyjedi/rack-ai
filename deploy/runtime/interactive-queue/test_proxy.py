import json
import unittest
from unittest.mock import patch

import proxy


class InteractiveQueueTests(unittest.TestCase):
    def test_progress_reads_do_not_resubmit_the_completion(self):
        trace = "1" * 32
        invocation = "2" * 32
        url = "http://127.0.0.1:8095/scoped/reservation/capability/v1/chat/completions"
        def progress(state):
            return (202, "application/json", json.dumps({
                "invocation_id": invocation, "state": state,
            }).encode())
        terminal = (200, "application/json", b'{"choices":[]}')
        headers = {"Prefer": "respond-async", "Idempotency-Key": "one"}
        with (patch.object(proxy, "forward", side_effect=[progress("queued"), progress("running"), terminal]) as forward,
              patch.object(proxy.time, "sleep"), patch.object(proxy, "report", wraps=proxy.report) as track):
            assert proxy.forward_interactive(url.replace("http://127.0.0.1:8095", "https://example.test"), b"{}", headers, 5, trace, None) == terminal

        self.assertEqual(forward.call_args_list[0].args[:3], (url, b"{}", headers))
        for call in forward.call_args_list[1:]:
            self.assertEqual(call.args[:3], (url.rsplit("/v1/", 1)[0] + "/v1/invocations/" + invocation, None, {}))
        self.assertEqual([call.args for call in track.call_args_list],
                         [(trace, "queued"), (trace, "running")])

    def test_invalid_progress_stops_without_following_untrusted_url(self):
        response = (202, "application/json", b'{"state":"queued","invocation_id":"../injection"}')
        with patch.object(proxy, "forward", return_value=response) as forward:
            self.assertEqual(proxy.forward_interactive("https://example.test/scoped/reservation/capability/v1/chat/completions", b"{}", {}, 5, None, None)[0], 502)
        self.assertEqual(forward.call_count, 1)
