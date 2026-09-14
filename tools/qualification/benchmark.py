"""Run fixed fresh trials through RackAI against an already acquired reservation.

Requires a private connection JSON (url/token), a public reservation JSON and a
NEW evidence directory. On lost response stop and reconcile; never rerun this
command as a retry. Each persisted request identifies exactly one intended call.
"""
import argparse
import json
from pathlib import Path
import time
import uuid
from cases import CASES, assess
from client import Client, Connection, save


def run_trial(context, case):
    client, reservation, directory = context
    identity = 'gptoss-' + str(uuid.uuid4())
    body = dict(model='big-brain', messages=[dict(role='user', content=case.prompt)],
                stream=False, max_tokens=1024, temperature=0, seed=120,
                reasoning_effort='low')
    request = dict(schema='rack-ai/runtime/v1', submission_id=identity,
        reservation_id=reservation['id'], generation=reservation['generation'],
        profile_hash=reservation['profile_hash'], prompt='', max_tokens=1024,
        timeout_seconds=240, wait_seconds=30,
        payload=dict(protocol='chat_completions', body=body))
    prefix = directory / case.name
    save(str(prefix) + '-request.json', request)
    start = time.monotonic()
    invocation = client.call(dict(operation='infer', request=request))
    save(str(prefix) + '-accepted.json', invocation)
    terminal = client.wait(('result', dict(invocation_id=invocation['id']),
                            'completed'), 255)
    elapsed = time.monotonic() - start
    save(str(prefix) + '-terminal.json', terminal)
    body = json.loads(terminal['result']['rack_protocol_response']['body'])
    result = assess(case, body)
    result.update(case=case.name, invocation_id=terminal['id'],
                  submission_id=identity, elapsed_seconds=elapsed,
                  activation=terminal['activation'], started=terminal['started'])
    save(str(prefix) + '-assessment.json', result)
    print(json.dumps(result), flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('connection', type=Path)
    parser.add_argument('reservation', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reload-check', action='store_true')
    args = parser.parse_args()
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    connection = Connection(**json.loads(args.connection.read_text()))
    client = Client(connection)
    reservation = json.loads(args.reservation.read_text())
    if reservation['state'] != 'ready' or reservation['owner'] != 'gptoss-qualification':
        raise ValueError('ready dedicated qualification reservation required')
    cases = (CASES[1],) if args.reload_check else CASES
    save(args.output / 'plan.json', [dict(name=c.name, prompt=c.prompt,
         expected=c.expected, warm=c.warm) for c in cases])
    results = [run_trial((client, reservation, args.output), case) for case in cases]
    save(args.output / 'summary.json', results)

if __name__ == '__main__':
    main()
