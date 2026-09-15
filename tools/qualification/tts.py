"""Bounded PR36 proof through an already deployed PR36 receiver; never switches services."""
import argparse
import io
import json
from pathlib import Path
import subprocess
import time
import urllib.request
import uuid
import wave

UUID = "GPU-357ef569-8fac-7c7d-ee1c-51677efb174f"

class Client:
    def __init__(self, options):
        self.options = options
        self.origin = "http://" + options.config["listen"]
        self.token = Path(options.token_file).read_text().strip()

    def post(self, call):
        path, body, key = call
        headers = {"Authorization":"Bearer " + self.token, "Content-Type":"application/json"}
        if key: headers["Idempotency-Key"] = key
        request = urllib.request.Request(self.origin + path, data=json.dumps(body).encode(), headers=headers)
        with urllib.request.urlopen(request, timeout=65) as response:
            raw = response.read(2880045)
            return raw if response.headers.get_content_type()=="audio/wav" else json.loads(raw)["result"]

    def operation(self, body):
        return self.post(("/runtime/v1", body, None))

    def acquire(self, tag):
        return self.operation(dict(operation="acquire", request=dict(
            schema="rack-ai/runtime/v1", source_system=self.options.source,
            work_id="pr36-qualification", acquisition_id=uuid.uuid4().hex, tag=tag,
            priority="low" if tag=="local-coder" else "paramount",
            capabilities=["coding"] if tag=="local-coder" else ["audio"],
            context_tokens=8192, ttl_seconds=3600, qualification=True)))

    def wait(self, demand):
        identity, state = demand
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            record = self.operation(dict(operation="inspect", reservation_id=identity))
            if record["state"] == state: return record
            if record["state"] in ("denied", "recovery_required", "cancelled", "expired"):
                raise RuntimeError("qualification_state:" + record["state"])
            time.sleep(.2)
        raise TimeoutError("qualification_wait")

    def release(self, demand):
        current = self.operation(dict(operation="inspect", reservation_id=demand["id"]))
        return self.operation(dict(operation="control", reservation_id=current["id"],
            request=dict(generation=current["generation"], action=dict(kind="release"))))

def preflight(config):
    state = json.loads((Path(config["authority_root"]) / "managed.json").read_text())
    problems = []
    if not any(p["tag"]=="local-tts" for p in config["profiles"]):
        problems.append("deployed_receiver_has_no_local_tts_profile")
    if any(d["state"]=="recovery_required" for d in state["data"]["demands"].values()):
        problems.append("existing_canonical_recovery_requires_operator_resolution")
    if state["claims"]:
        problems.append("qualification_window_not_quiescent")
    return problems

def run(client, report):
    coder = client.wait((client.acquire("local-coder")["id"], "ready"))
    report["coder_before"] = {"id":coder["id"], "generation":coder["generation"]}
    start = time.monotonic()
    tts = client.acquire("local-tts")
    try:
        tts = client.wait((tts["id"], "ready"))
        report["cold_start_to_ready_seconds"] = time.monotonic() - start
        client.wait((coder["id"], "held"))
        report["samples"] = []
        for text in ("Wait... seriously? [gasp] It actually worked?",
                     "Now that is a pleasant surprise. [chuckle] Let's try that again."):
            start = time.monotonic()
            raw = client.post((tts["gateway_path"]+"/speech",
                {"text":text,"voice":client.options.voice},uuid.uuid4().hex))
            elapsed = time.monotonic() - start
            with wave.open(io.BytesIO(raw)) as wav:
                assert (wav.getnchannels(),wav.getframerate(),wav.getsampwidth())==(1,24000,2)
                duration=wav.getnframes()/24000
            report["samples"].append(dict(wall_seconds=elapsed,audio_seconds=duration,rtf=elapsed/duration))
        logs = subprocess.run(["journalctl","--user","-u","rack-runtime-"+tts["generation"]+".service",
            "--no-pager","-o","cat"],capture_output=True,text=True,timeout=5,check=True).stdout
        report["worker_metrics"] = [json.loads(line) for line in logs.splitlines()
            if line.startswith('{"event": "model_loaded"') or line.startswith('{"event": "synthesis"')]
        assert sum(e["event"]=="model_loaded" for e in report["worker_metrics"])==1
        assert sum(e["event"]=="synthesis" for e in report["worker_metrics"])==2
    finally:
        start = time.monotonic()
        client.release(tts)
        client.wait((tts["id"],"released"))
        restored = client.wait((coder["id"],"ready"))
        report["restoration_seconds"] = time.monotonic()-start
        report["coder_restored"] = {"id":restored["id"],"generation":restored["generation"]}
        assert restored["generation"]!=coder["generation"]
        with urllib.request.urlopen("http://127.0.0.1:8018/v1/models",timeout=5) as response:
            assert json.load(response)["data"][0]["id"]=="local-coder"
        report["automatic_restoration"] = True

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--config",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--preflight-only",action="store_true")
    parser.add_argument("--token-file")
    parser.add_argument("--source",default="qualification-operator")
    parser.add_argument("--voice",default="approved")
    args=parser.parse_args()
    args.config=json.loads(args.config.read_text())
    report={"gpu_uuid":UUID,"blockers":preflight(args.config),"live_qualified":False}
    try:
        if report["blockers"] or args.preflight_only: return
        if not args.token_file: raise ValueError("token_file_required")
        run(Client(args),report)
        report["live_qualified"]=True
    finally:
        args.output.write_text(json.dumps(report,indent=2)+"\n")

if __name__=="__main__":main()
