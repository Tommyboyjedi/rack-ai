"""Manual sessions retain explicit start/finish and an auditable idle safety backstop."""
import time
import requests
from fixture import receiver, wait_for, TOKEN

def test_manual_session_idle_release_and_fresh_start(tmp_path):
    def configure(c):c["reservation_idle_seconds"]=3
    with receiver(tmp_path/"media",configure=configure) as env:
        api=env["api"]+"/api/media/v1"
        headers={"Authorization":"Bearer "+TOKEN}
        request={"schema":"rack-ai/media/v1","idempotency_key":"first"}
        first=requests.post(api+"/sessions",headers=headers,json=request,timeout=3).json()
        def session():
            return requests.get(api+"/sessions/"+first["id"],headers=headers,timeout=3).json()
        wait_for(lambda:session()["state"]=="ready")
        # Repeated owner inspection does not extend activity.
        stopped=wait_for(lambda:(s if (s:=session())["state"]=="stopped" else None))
        assert stopped["terminal_reason"]=="idle_timeout"
        assert stopped["access_url"] is None
        assert not list((env["root"]/"resources/leases").glob("*.json"))
        status=requests.get(api+"/status",headers=headers,timeout=3).json()
        assert status["message"]=="idle_timeout"
        replay=requests.post(api+"/sessions",headers=headers,json=request,timeout=3).json()
        assert replay["id"]==first["id"] and replay["state"]=="stopped"
        request["idempotency_key"]="second"
        fresh=requests.post(api+"/sessions",headers=headers,json=request,timeout=3).json()
        assert fresh["id"]!=first["id"]
        requests.post(api+"/sessions/"+fresh["id"]+"/release",headers=headers,json={},timeout=3).raise_for_status()
        wait_for(lambda:requests.get(api+"/sessions/"+fresh["id"],headers=headers,timeout=3).json()["state"]=="stopped")
