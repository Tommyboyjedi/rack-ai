"""Disposable TTS protocol fixture; no model or GPU."""
import runpy
import sys
from pathlib import Path
# Reuse the existing fixture's event/control lifecycle, changing only its protocol.
source = (Path(__file__).with_name("backend.py")).read_text()
source = source.replace("import http.server", "import http.server\nimport io\nimport wave")
source = source.replace("self.reply({'data':", """if self.path == '/health':
            self.reply(dict(model=model, activation=activation, sample_rate=24000, voices=['approved', 'second']))
            return
        self.reply({'data':""")
source = source.replace("assert request['model'] == model", """assert self.headers.get('Authorization') == 'Bearer '+activation
        assert set(request) == {'text','voice'}""")
source = source.replace("if self.path=='/v1/responses':", """if self.path=='/speech':
            output=io.BytesIO()
            with wave.open(output,'wb') as wav:
                wav.setnchannels(1); wav.setsampwidth(2); wav.setframerate(24000)
                wav.writeframes(b'\\0\\0'*2400)
            data=output.getvalue()
            if control().get('invalid_wav'): data=b'not-wav'
            self.send_response(200); self.send_header('Content-Type','audio/wav')
            self.send_header('Content-Length',str(len(data))); self.end_headers()
            self.wfile.write(data); event('complete'); return
        if self.path=='/v1/responses':""")
exec(compile(source, __file__, "exec"))
