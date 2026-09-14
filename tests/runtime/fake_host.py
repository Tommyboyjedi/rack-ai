#!/usr/bin/env python3
"""Fixture-only systemd/Docker/NVIDIA transport with actual disposable child processes."""
import fcntl
import json
import os
import signal
import subprocess
import sys
from pathlib import Path
root=Path(os.environ['RACK_HOST_FIXTURE'])
name=Path(sys.argv[0]).name
args=sys.argv[1:]
lock=open(root/'machine.lock','a'); fcntl.flock(lock,fcntl.LOCK_EX)
path=root/'machine.json'
s=json.loads(path.read_text()) if path.exists() else {}
def alive():
    try:
        return Path(f'/proc/{s["pid"]}/stat').read_text().rsplit(') ',1)[1].split()[0]!='Z'
    except (KeyError,FileNotFoundError):
        return False

def launch(command,activation):
    child=subprocess.Popen(command,env=dict(os.environ,RACK_RUNTIME_ACTIVATION=activation),stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    s.update(pid=child.pid,activation=activation,invocation=activation)

def stop():
    if alive():
        env=Path(f'/proc/{s["pid"]}/environ').read_bytes()
        assert f'RACK_RUNTIME_ACTIVATION={s["activation"]}'.encode() in env.split(b'\0')
        os.kill(s['pid'],signal.SIGTERM)

fault_path=root/'faults.json'
faults=json.loads(fault_path.read_text()) if fault_path.exists() else {}
if name=='nvidia-smi':
    if any('query-gpu=memory' in a for a in args):
        print(faults.get('memory_mib',16384))
    elif '-x' in args:
        uuid=args[args.index('-i')+1]
        record=f'<process_info><pid>{s["pid"]}</pid><type>C</type></process_info>' if alive() else ''
        if faults.get('foreign_pid'): record+=f'<process_info><pid>{faults["foreign_pid"]}</pid><type>C</type></process_info>'
        print(f'<nvidia_smi_log><gpu><uuid>{uuid}</uuid><processes>{record}</processes></gpu></nvidia_smi_log>')
    else:
        sys.exit(3)
elif name=='systemd-run':
    unit=next(a.split('=',1)[1] for a in args if a.startswith('--unit='))
    activation=next(a.split('=',2)[2] for a in args if a.startswith('--setenv=RACK_RUNTIME_ACTIVATION='))
    s['unit']=unit
    launch(args[args.index('--')+1:],activation)
elif name=='systemctl':
    if 'show' in args:
        active=alive()
        cgroup=Path(f'/proc/{s["pid"]}/cgroup').read_text().split('::',1)[1].strip() if active else ''
        print(f'Id={s.get("unit","")}\nJob=\nInvocationID={s.get("invocation","") if active else ""}\nMainPID={s["pid"] if active else 0}\nControlGroup={cgroup}\nActiveState={"active" if active else "inactive"}')
    elif 'stop' in args: stop()
    else: sys.exit(4)
elif name=='docker':
    if args[0]=='run':
        image=next(a for a in args if a.startswith('sha256:'))
        activation=next(a.split('=',2)[2] for a in args if a.startswith('--label=rack.activation='))
        s.update(image=image,id=(activation*2))
        launch([sys.executable,*args[args.index(image)+1:]],activation)
        print(s['id'])
    elif args[0]=='inspect':
        assert args[1]==s['id']
        print(json.dumps([dict(Id=s['id'],Image=s['image'],State=dict(Pid=s['pid'] if alive() else 0,Running=alive()),Config=dict(Labels={'rack.activation':s['activation']}))]))
    elif args[0]=='top':
        print('PID\n'+str(s['pid']))
    elif args[0]=='exec':
        for descriptor in Path(f'/proc/{s["pid"]}/fd').iterdir():
            try: print(os.readlink(descriptor))
            except FileNotFoundError: pass
    elif args[0]=='kill': stop()
    else: sys.exit(5)
else: sys.exit(6)
if name!='nvidia-smi':
    with open(root/'commands.jsonl','a') as out:out.write(json.dumps(dict(program=name,args=args))+'\n')
tmp=root/'machine.next';tmp.write_text(json.dumps(s));tmp.replace(path)
