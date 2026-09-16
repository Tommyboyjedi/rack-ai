import importlib.util
import json
import threading
import time
import unittest
import urllib.request
from pathlib import Path
from types import SimpleNamespace
from server import Server

spec = importlib.util.spec_from_file_location("rack_speech_worker", Path(__file__).with_name("__main__.py"))
module = importlib.util.module_from_spec(spec)
import sys
sys.modules[spec.name] = module
spec.loader.exec_module(module)

class WorkerTests(unittest.TestCase):
    def test_health_stays_responsive_and_one_generation(self):
        entered = threading.Event()
        leave = threading.Event()
        calls = []
        def generate(request):
            calls.append(request)
            entered.set()
            leave.wait(3)
            return b"fake-audio"
        worker = SimpleNamespace(activation="fixture",
            registry=SimpleNamespace(load=lambda: {"approved": object()}),
            engine=SimpleNamespace(load_seconds=1, synthesize=generate))
        server = Server(("127.0.0.1", 0), module.Handler)
        server.worker = worker
        runner = threading.Thread(target=server.serve_forever, daemon=True); runner.start()
        def request(path, body=None):
            data = json.dumps(body).encode() if body is not None else None
            req = urllib.request.Request(f"http://127.0.0.1:{server.server_port}"+path,
                data=data, headers={"Authorization":"Bearer fixture","Content-Type":"application/json"})
            try:
                with urllib.request.urlopen(req, timeout=4) as response: return response.status, response.read()
            except urllib.error.HTTPError as response:
                with response: return response.code, response.read()
        from concurrent.futures import ThreadPoolExecutor
        try:
            with ThreadPoolExecutor() as pool:
                pending=pool.submit(request,"/speech",{"text":"hello","voice":"approved"})
                self.assertTrue(entered.wait(2))
                self.assertEqual(request("/health")[0],200)
                self.assertEqual(request("/speech",{"text":"second","voice":"approved"})[0],429)
                leave.set()
                self.assertEqual(pending.result()[0],200)
                self.assertEqual(len(calls),1)
        finally:
            leave.set(); server.shutdown(); server.server_close(); runner.join(2)

if __name__=="__main__":unittest.main()
