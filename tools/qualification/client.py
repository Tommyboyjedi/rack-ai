"""Bounded managed API access. No inference transport retries."""
from dataclasses import dataclass
from pathlib import Path
import json
import os
import time
import urllib.request

@dataclass(frozen=True)
class Connection:
    url: str
    token: str

class Client:
    def __init__(self, connection):
        if not connection.url.startswith('http://127.0.0.1:'):
            raise ValueError('qualification requires a loopback managed receiver')
        self.connection = connection
        self.safety_check = None
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def call(self, packet):
        request = urllib.request.Request(self.connection.url + '/runtime/v1',
            data=json.dumps(packet).encode(), headers={
                'Authorization': 'Bearer ' + self.connection.token,
                'Content-Type': 'application/json'})
        with self.http.open(request, timeout=10) as response:
            value = json.load(response)
        return value['result']

    def wait(self, identity, bound):
        operation, field, expected = identity
        deadline = time.monotonic() + bound
        while time.monotonic() < deadline:
            if self.safety_check:
                self.safety_check()
            abort = os.environ.get('RACK_QUALIFICATION_ABORT')
            if abort and Path(abort).exists():
                raise RuntimeError('qualification aborted: ' + Path(abort).read_text())
            value = self.call(dict(operation=operation, **field))
            if self.safety_check:
                self.safety_check()
            if value['state'] == expected:
                return value
            if value['state'] in ('uncertain', 'cancelled', 'expired',
                                  'recovery_required', 'denied', 'released'):
                raise RuntimeError(json.dumps(value))
            time.sleep(0.5)
        raise TimeoutError('managed operation did not reach ' + expected)


def save(path, value):
    """Create each evidence record once; flush before any dependent dispatch."""
    with Path(path).open('x') as output:
        json.dump(value, output, indent=2)
        output.write('\n')
        output.flush()
        os.fsync(output.fileno())
    directory = os.open(str(Path(path).parent), os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
