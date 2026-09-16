"""Multipart registration against the real receiver and existing Python voice registry."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
import wave
import requests
from support import Rack
from test_speech import configure as speech_config, speech

def wav(samples=144000, rate=24000, channels=1, width=2, value=0):
    output=io.BytesIO()
    with wave.open(output,"wb") as stream:
        stream.setnchannels(channels);stream.setsampwidth(width);stream.setframerate(rate)
        stream.writeframes(bytes([value])*samples*channels*width)
    return output.getvalue()

class VoiceRegistrationTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix="rack-register-")
        root=Path(self.temp.name)
        self.storage=root/"voices";self.storage.mkdir()
        self.registry=root/"voices.json"
        self.registry.write_text(json.dumps(dict(root=str(self.storage),voices={})))
        self.registry.chmod(0o600)
        def configure(c):
            speech_config(c)
            next(p for p in c["profiles"] if p["tag"]=="local-tts")["args"] += ["--voices",str(self.registry)]
        self.r=Rack(root/"rack",configure)
        self.url="http://"+self.r.address+"/runtime/v1/voices/register"

    def tearDown(self):
        self.r.close()
        self.temp.cleanup()

    def upload(self, identity="character-jane", audio=None, filename="reference.wav", extra=None):
        data=dict(voice_id=identity)
        data.update(extra or {})
        return requests.post(self.url,data=data,
            files={"file":(filename,audio if audio is not None else wav(),"audio/wav")},timeout=10)

    def test_register_replace_and_use_without_model_reload(self):
        raw=wav()
        response=self.upload(audio=raw)
        self.assertEqual(response.status_code,200,response.text)
        self.assertEqual(response.json(),dict(voice_id="character-jane",registered=True,sha256=hashlib.sha256(raw).hexdigest()))
        record=json.loads(self.registry.read_text())
        first=record["voices"]["character-jane"]["file"]
        self.assertTrue(first.startswith("registered-"))
        self.assertEqual((self.storage/first).read_bytes(),raw)
        self.assertEqual(self.registry.stat().st_mode & 0o777,0o600)
        tts=self.r.wait(self.r.acquire("cb","local-tts","paramount"))
        body={"voice":"character-jane","text":"Hello! [chuckle]"}
        self.assertEqual(speech(self.r,tts,"before",body)[0],200)
        replacement=wav(samples=720000,value=1)
        response=self.upload(audio=replacement)
        self.assertEqual(response.status_code,200,response.text)
        self.assertEqual(response.json()["sha256"],hashlib.sha256(replacement).hexdigest())
        current=json.loads(self.registry.read_text())["voices"]["character-jane"]
        self.assertNotEqual(current["file"],first)
        self.assertEqual((self.storage/current["file"]).read_bytes(),replacement)
        self.assertEqual(speech(self.r,tts,"after",body)[0],200)
        self.assertEqual(self.upload("second-voice").status_code,200)
        status,listed=speech(self.r,tts,body={},path="voices")
        self.assertEqual(status,200)
        self.assertEqual(set(json.loads(listed)["voices"]),{"character-jane","second-voice"})
        self.assertEqual(speech(self.r,tts,"new",{"voice":"second-voice","text":"New reference!"})[0],200)
        self.assertEqual(self.r.counts("start")[tts["model"]],1)
        self.r.release(tts);self.r.wait(tts,"released")

    def test_invalid_ids_paths_and_multipart_leave_registry_unchanged(self):
        before=self.registry.read_bytes()
        for identity in ("../outside","/tmp/voice","..","bad name","x"*129,"bad\\path"):
            with self.subTest(identity=identity):
                self.assertEqual(self.upload(identity).status_code,422)
        for filename in ("../../escape.wav","/tmp/escape.wav","bad\\path.wav","reference.mp3"):
            with self.subTest(filename=filename):
                self.assertEqual(self.upload(filename=filename).status_code,422)
        self.assertEqual(self.upload(extra={"audio_prompt_path":"/etc/passwd"}).status_code,422)
        self.assertEqual(self.registry.read_bytes(),before)
        self.assertEqual(list(self.storage.iterdir()),[])

    def test_bad_audio_and_size_limits(self):
        for data in (b"not a wav",wav(samples=120000),wav(samples=720001),
                     wav(rate=8000),wav(channels=2),wav(width=1),wav()[:-1]):
            with self.subTest(size=len(data)):
                self.assertEqual(self.upload(audio=data).status_code,422)
        self.assertEqual(self.upload(audio=b"x"*(6*1024*1024+1)).status_code,413)
        self.assertEqual(json.loads(self.registry.read_text())["voices"],{})

    def test_replacement_cannot_exceed_retained_upload_budget(self):
        self.assertEqual(self.upload(audio=wav()).status_code,200)
        self.assertEqual(self.upload(audio=wav(value=1)).status_code,200)
        before=self.registry.read_bytes()
        # Sparse retained fixture consumes the byte budget without writing real audio.
        used=sum(p.stat().st_size for p in self.storage.iterdir())
        retained=self.storage/("registered-"+"f"*64+".wav")
        with retained.open("wb") as stream:
            stream.truncate(768*1024*1024-used)
        count=len(list(self.storage.iterdir()))
        self.assertEqual(self.upload(audio=wav(value=2)).status_code,429)
        self.assertEqual(self.registry.read_bytes(),before)
        self.assertEqual(len(list(self.storage.iterdir())),count)
        # Reusing an already retained reference needs no additional storage.
        self.assertEqual(self.upload(audio=wav()).status_code,200)

    def test_administrator_symlink_is_rejected(self):
        original=self.registry.with_suffix(".original")
        self.registry.rename(original)
        self.registry.symlink_to(original)
        response=self.upload()
        self.assertEqual(response.status_code,503)
        self.assertNotIn(str(self.storage),response.text)
        self.assertEqual(json.loads(original.read_text())["voices"],{})

if __name__=="__main__":unittest.main()
