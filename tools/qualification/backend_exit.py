"""Trusted manager exit evidence for one authenticated transient activation."""
from dataclasses import asdict
import json
import os
import subprocess
import time

MAIN_PROCESS_EXIT = '98e322203f7a4ed290d09fe03c09fe15'
JOURNAL_VISIBILITY_SECONDS = 1.0
JOURNAL_POLL_SECONDS = .05


def exit_records(target):
    # Underscore journal fields are assigned by journald, not the backend.
    fields = dict(USER_UNIT=target.unit,USER_INVOCATION_ID=target.invocation,
                  _BOOT_ID=target.boot.replace('-',''),MESSAGE_ID=MAIN_PROCESS_EXIT,
                  _UID=str(os.getuid()),_COMM='systemd',_SYSTEMD_USER_UNIT='init.scope')
    raw = subprocess.check_output(['journalctl','--user','--no-pager','-o','json',
        '-n','2',*(key+'='+value for key,value in fields.items())],text=True,timeout=2)
    return [json.loads(line) for line in raw.splitlines()]

class ExitEvidence:
    def __init__(self, roots):
        self.roots = roots

    def read(self, target):
        self.process_exited(target)
        records = self.records(target)
        if len(records)!=1:
            raise ValueError('owned backend exit evidence missing or ambiguous')
        record = records[0]
        expected = dict(USER_UNIT=target.unit,USER_INVOCATION_ID=target.invocation,
                        _BOOT_ID=target.boot.replace('-',''),MESSAGE_ID=MAIN_PROCESS_EXIT,
                        _UID=str(os.getuid()),_COMM='systemd',
                        _SYSTEMD_USER_UNIT='init.scope',COMMAND='ExecStart')
        if any(record.get(key)!=value for key,value in expected.items()):
            raise ValueError('backend exit evidence identity changed')
        executable = record.get('_EXE')
        if executable not in ('/usr/lib/systemd/systemd','/lib/systemd/systemd'):
            raise ValueError('backend exit evidence is not from systemd')
        if record.get('EXIT_CODE') not in ('exited','killed','dumped') or not record.get('EXIT_STATUS'):
            raise ValueError('backend exit status unavailable')
        # Recheck PID reuse while querying journal evidence.
        self.process_exited(target)
        return dict(status='exited',process=asdict(target),exit_evidence=record)

    def records(self, target):
        deadline=time.monotonic()+JOURNAL_VISIBILITY_SECONDS
        while True:
            records=exit_records(target)
            if records or time.monotonic()>=deadline:
                return records
            # Only wait for journal visibility, never restart or redispatch.
            self.process_exited(target)
            time.sleep(JOURNAL_POLL_SECONDS)

    def process_exited(self, target):
        boot=(self.roots.proc/'sys/kernel/random/boot_id').read_text().strip()
        if boot!=target.boot:
            raise ValueError('managed process boot changed')
        root=self.roots.proc/str(target.pid)
        try:
            fields=(root/'stat').read_text().rsplit(') ',1)[1].split()
        except FileNotFoundError:
            if root.exists():
                raise ValueError('managed process evidence unavailable')
            return
        if fields[19]!=target.start:
            raise ValueError('managed process generation changed')
        if fields[0]!='Z':
            raise ProcessStillRunning('managed process still exists despite exit observation')


class ProcessExited(Exception):
    """A zombie still needs trusted exit evidence; this alone is not proof."""


class ProcessStillRunning(ValueError):
    """Missing counters for a live process remain monitoring evidence loss."""
