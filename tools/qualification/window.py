"""One explicitly authorized window: fixed trials, reload, verified restoration.

Run only after reviewing the recorded preflight, configuration and service list.
The receiver configuration must use the canonical rack authority. Evidence is
private and must be outside Git. No automatic tuning or inference replay occurs.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import time
from benchmark import run_trial
from cases import CASES
from client import Client, Connection, save
from lifecycle import Lifecycle
from monitor import Monitor
from services import Services, ROOT


def interrupted(signum, frame):
    raise KeyboardInterrupt('qualification interrupted by signal ' + str(signum))


def trials(context, reload_check):
    client, directory, monitor = context
    directory.mkdir(mode=0o700)
    lifecycle = Lifecycle(client, directory)
    try:
        demand = lifecycle.acquire(monitor)
        selected = (CASES[1],) if reload_check else CASES
        save(directory/'plan.json', [dict(name=c.name,prompt=c.prompt,expected=c.expected)
                                    for c in selected])
        results = []
        for case in selected:
            monitor.check()
            results.append(run_trial((client,demand,directory),case))
        monitor.check()
        save(directory/'summary.json', results)
    finally:
        lifecycle.release()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('config', type=Path)
    parser.add_argument('connection', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    config = json.loads(args.config.read_text())
    if Path(config['authority_root']).resolve() != ROOT:
        raise ValueError('canonical authority required')
    if len(config['profiles']) != 1 or config['profiles'][0]['qualified']:
        raise ValueError('one unqualified candidate required')
    save(args.output/'config.json', config)
    for signum in (signal.SIGTERM,signal.SIGHUP):
        signal.signal(signum, interrupted)
    executable = Path(__file__).resolve().parents[2]/'target/debug/rack_ai_runtime'
    subprocess.run([str(executable),'validate',str(args.config)],check=True,timeout=10)
    services = Services(args.output)
    client = Client(Connection(**json.loads(args.connection.read_text())))
    monitor = Monitor(args.output)
    os.environ['RACK_QUALIFICATION_ABORT'] = str((args.output/'ABORT').resolve())
    receiver = None
    try:
        services.stop()
        monitor.thread.start()
        with (args.output/'receiver.log').open('x') as log:
            receiver = subprocess.Popen([str(executable),str(args.config)],
                stdout=log,stderr=log,start_new_session=True)
            deadline = time.monotonic()+10
            while time.monotonic()<deadline:
                if receiver.poll() is not None:
                    raise RuntimeError('qualification receiver failed')
                try:
                    client.call(dict(operation='discover'))
                    break
                except OSError:
                    time.sleep(.2)
            else:
                raise TimeoutError('receiver startup')
            trials((client,args.output/'initial',monitor),False)
            trials((client,args.output/'reload',monitor),True)
    finally:
        # Restore checks canonical claims and process evidence before any GPU start.
        # Failed cleanup intentionally leaves the receiver alive for reconciliation.
        if monitor.thread.ident is not None:
            monitor.close()
        services.restore()
        if receiver is not None:
            receiver.terminate()
            receiver.wait(timeout=10)

if __name__ == '__main__':
    main()
