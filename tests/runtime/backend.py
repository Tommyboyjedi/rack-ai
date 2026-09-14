"""Controllable disposable hosting process. Never opens a GPU or workspace tool."""
import http.server
import json
import os
import signal
import sys
import time
from pathlib import Path

port, model, events, controls = sys.argv[1:]
activation = os.environ['RACK_RUNTIME_ACTIVATION']

def control():
    path = Path(controls)
    return json.loads(path.read_text()) if path.exists() else {}

def event(kind):
    with open(events, 'a') as out:
        out.write(json.dumps(dict(kind=kind, model=model, pid=os.getpid(), activation=activation))+'\n')

def stop(*_):
    if control().get('ignore_stop'):
        return
    event('stop')
    sys.exit(0)

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        event('probe')
        time.sleep(control().get('probe_delay',0))
        self.reply({'data': [{'id': control().get('model', model)}]})

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert request['model'] == model
        event('dispatch')
        with open(events+'.requests','a') as out:
            out.write(json.dumps(request)+'\n')
        time.sleep(control().get('delay', 0))
        if control().get('uncertain'):
            self.connection.close()
            return
        if self.path=='/v1/responses':
            self.reply({'id':activation,'model':model,'status':'completed','output':[{'type':'message','content':[{'type':'output_text','text':'fixture'}]}]})
            event('complete'); return
        if request.get('stream'):
            body = 'data: '+json.dumps({'model':model,'choices':[{'delta':{'content':'fixture'}}]})+'\n\ndata: [DONE]\n\n'
            data = body.encode()
            self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Content-Length',str(len(data))); self.end_headers()
            self.wfile.write(data); event('complete'); return
        self.reply({'id':activation, 'model':model, 'choices':[{'message':{'role':'assistant','content':'fixture response'}}],
                    'usage':{'prompt_tokens':1,'completion_tokens':2}})
        event('complete')

signal.signal(signal.SIGTERM, stop)
time.sleep(control().get('startup_delay', 0))
server = http.server.ThreadingHTTPServer(('127.0.0.1', int(port)), Handler)
event('start')
server.serve_forever()
