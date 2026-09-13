"""Keep a changed Tailscale address and the private nginx listener consistent."""
import subprocess
from settings import tailnet_ipv4
from proxy_prepare import prepare, write_binding
from proxy_reload import reload_proxy, UNIT

def synchronize(settings):
    address = tailnet_ipv4()
    target = settings.root / "config/listen.conf"
    expected = "listen " + address + ":443 ssl;\n"
    previous = target.read_text() if target.exists() else None
    if previous != expected:
        try:
            prepared = prepare(settings)
            if prepared != address:
                raise RuntimeError("Tailscale address changed during binding preparation")
            state = subprocess.check_output(["systemctl", "show", UNIT, "-p", "ActiveState",
                                             "--value"], text=True, timeout=10).strip()
            if state == "active":
                reload_proxy(settings)
        except Exception:
            # A failed reload must not look committed to the next DNS update.
            if previous is not None:
                write_binding(settings, previous)
            else:
                target.unlink(missing_ok=True)
            raise
    return address
