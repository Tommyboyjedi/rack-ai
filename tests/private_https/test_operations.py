import importlib
import subprocess
import sys
from pathlib import Path
from unittest.mock import Mock
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "ops/private_https"))
settings = importlib.import_module("settings")
duckdns = importlib.import_module("duckdns")
updater = importlib.import_module("update_dns")
hook = importlib.import_module("dns_hook")
prepare = importlib.import_module("proxy_prepare")
reload = importlib.import_module("proxy_reload")

@pytest.fixture
def config(tmp_path):
    secret_dir = tmp_path / "secret"
    secret_dir.mkdir(mode=0o700)
    token = secret_dir / "token"
    token.write_text("synthetic-duckdns-credential-never-real")
    token.chmod(0o600)
    (tmp_path / "config").mkdir()
    return settings.Settings("gpurack.duckdns.org", token, tmp_path)

@pytest.mark.parametrize("address", ["8.8.8.8", "192.168.1.5", "127.0.0.1", "::1", "", "100.64.1.1\n8.8.8.8"])
def test_non_tailnet_addresses_never_reach_dns_api(config, monkeypatch, address):
    connection = Mock()
    monkeypatch.setattr(duckdns.http.client, "HTTPSConnection", connection)
    with pytest.raises(RuntimeError):
        duckdns.DuckDns(config).set_ipv4(address)
    connection.assert_not_called()

@pytest.mark.parametrize("mode", [0o644, 0o660, 0o400])
def test_secret_permissions_fail_closed(config, mode):
    config.token_file.chmod(mode)
    with pytest.raises(RuntimeError, match="permissions"):
        duckdns.DuckDns(config).set_ipv4("100.116.176.86")

def test_secret_symlink_rejected(config):
    target = config.token_file.with_name("original")
    config.token_file.rename(target)
    config.token_file.symlink_to(target)
    with pytest.raises(RuntimeError, match="ownership"):
        duckdns.DuckDns(config).set_ipv4("100.116.176.86")

def test_api_transport_errors_cannot_disclose_token(config, monkeypatch, capsys):
    token = config.token_file.read_text()
    connection = Mock()
    connection.request.side_effect = OSError("secret URL token=" + token)
    monkeypatch.setattr(duckdns.http.client, "HTTPSConnection", Mock(return_value=connection))
    with pytest.raises(RuntimeError) as failure:
        duckdns.DuckDns(config).set_ipv4("100.116.176.86")
    assert token not in str(failure.value)
    assert failure.value.__suppress_context__
    assert token not in capsys.readouterr().out
    connection.close.assert_called_once()

def test_api_explicit_address_and_no_redirect_following(config, monkeypatch):
    connection = Mock()
    response = connection.getresponse.return_value
    response.status = 302
    response.read.return_value = b"OK"
    factory = Mock(return_value=connection)
    monkeypatch.setattr(duckdns.http.client, "HTTPSConnection", factory)
    with pytest.raises(RuntimeError):
        duckdns.DuckDns(config).set_ipv4("100.116.176.86")
    factory.assert_called_once_with("www.duckdns.org", timeout=20)
    assert "ip=100.116.176.86" in connection.request.call_args.args[1]
    assert connection.request.call_count == 1

def test_no_api_address_autodetection(config):
    with pytest.raises(RuntimeError, match="autodetection"):
        duckdns.DuckDns(config)._request({})

def test_unwanted_ipv6_cleared_before_explicit_safe_ipv4(config, monkeypatch):
    api = Mock()
    monkeypatch.setattr(updater, "DuckDns", Mock(return_value=api))
    monkeypatch.setattr(updater, "synchronize", lambda s: "100.116.176.86")
    responses = iter([("2001:db8::1",), (), ("100.116.176.86",), (), ("100.116.176.86",), ()])
    monkeypatch.setattr(updater, "resolve", lambda query, resolver: next(responses))
    assert updater.update(config) == "100.116.176.86"
    assert api.method_calls == [
        ("clear_addresses", ("100.116.176.86",), {}),
        ("set_ipv4", ("100.116.176.86",), {}),
    ]

def test_dns_resolver_failure_is_not_empty_success(monkeypatch):
    monkeypatch.setattr(settings.subprocess, "run", lambda *a, **kw:
                        subprocess.CompletedProcess([], 0, ";; ->>HEADER<<- status: SERVFAIL, id: 1\n"))
    with pytest.raises(RuntimeError, match="resolver failed"):
        settings.resolve(("gpurack.duckdns.org", "AAAA"), "1.1.1.1")

def test_challenge_rejects_other_domain(config, monkeypatch):
    monkeypatch.setenv("CERTBOT_DOMAIN", "unapproved.duckdns.org")
    api = Mock()
    monkeypatch.setattr(hook, "DuckDns", api)
    with pytest.raises(RuntimeError, match="Unapproved"):
        hook.challenge(config, "auth")
    api.assert_not_called()

def test_challenge_auth_requires_both_resolvers(config, monkeypatch):
    monkeypatch.setenv("CERTBOT_DOMAIN", config.hostname)
    monkeypatch.setenv("CERTBOT_VALIDATION", "challenge-value")
    api = Mock()
    monkeypatch.setattr(hook, "DuckDns", Mock(return_value=api))
    resolver = Mock(return_value=("challenge-value",))
    monkeypatch.setattr(hook, "resolve", resolver)
    hook.challenge(config, "auth")
    assert resolver.call_count == 2
    api.set_txt.assert_called_once_with("challenge-value")

def test_challenge_propagation_has_deadline(config, monkeypatch):
    monkeypatch.setenv("CERTBOT_DOMAIN", config.hostname)
    monkeypatch.setenv("CERTBOT_VALIDATION", "challenge-value")
    monkeypatch.setattr(hook, "DuckDns", Mock())
    monkeypatch.setattr(hook, "PROPAGATION_SECONDS", 0)
    with pytest.raises(RuntimeError, match="deadline"):
        hook.challenge(config, "auth")

def test_private_listener_uses_discovered_ip(config, monkeypatch):
    monkeypatch.setattr(prepare, "tailnet_ipv4", lambda: "100.116.176.86")
    prepare.prepare(config)
    assert (config.root / "config/listen.conf").read_text() == "listen 100.116.176.86:443 ssl;\n"

def test_renewal_does_not_start_inactive_proxy_or_restart_rack(config, monkeypatch):
    run = Mock()
    monkeypatch.setattr(reload.subprocess, "run", run)
    monkeypatch.setattr(reload.subprocess, "check_output",
                        lambda *a, **kw: "ActiveState=inactive\nMainPID=0\n")
    reload.reload_proxy(config)
    assert run.call_count == 2
    assert all(call.args[0][0] == "openssl" for call in run.call_args_list)

def test_proxy_binding_failure_prevents_dns_change(config, monkeypatch):
    def unavailable(_):
        raise RuntimeError("Proxy binding failed")
    monkeypatch.setattr(updater, "synchronize", unavailable)
    api = Mock()
    monkeypatch.setattr(updater, "DuckDns", api)
    with pytest.raises(RuntimeError, match="binding failed"):
        updater.update(config)
    api.assert_not_called()

def test_dns_propagation_failure_is_bounded(config, monkeypatch):
    monkeypatch.setattr(updater, "synchronize", lambda s: "100.116.176.86")
    monkeypatch.setattr(updater, "DuckDns", Mock())
    monkeypatch.setattr(updater, "resolve", lambda query, resolver: ())
    ticks = iter([0, 181])
    monkeypatch.setattr(updater.time, "monotonic", lambda: next(ticks))
    with pytest.raises(RuntimeError, match="verification deadline"):
        updater.update(config)

def test_reload_rejects_foreign_systemd_command(config, monkeypatch):
    monkeypatch.setattr(reload.subprocess, "run", Mock())
    monkeypatch.setattr(reload.subprocess, "check_output", lambda *a, **kw:
                        "ActiveState=active\nMainPID=42\nControlGroup=/system.slice/other.service\nExecStart=unrelated\n")
    with pytest.raises(RuntimeError, match="Unexpected systemd"):
        reload.reload_proxy(config)
    assert all(call.args[0][0] == "openssl" for call in reload.subprocess.run.call_args_list)

@pytest.mark.parametrize("previous", [None, "listen 100.64.1.2:443 ssl;\n"])
def test_failed_rebind_remains_retryable_without_publishing_dns(config, monkeypatch, previous):
    binding = importlib.import_module("binding")
    target = config.root / "config/listen.conf"
    if previous is not None:
        target.write_text(previous)
    monkeypatch.setattr(binding, "tailnet_ipv4", lambda: "100.116.176.86")
    monkeypatch.setattr(prepare, "tailnet_ipv4", lambda: "100.116.176.86")
    monkeypatch.setattr(binding.subprocess, "check_output", lambda *a, **kw: "active\n")
    failed = Mock(side_effect=RuntimeError("reload failed"))
    monkeypatch.setattr(binding, "reload_proxy", failed)
    for _ in range(2):
        with pytest.raises(RuntimeError, match="reload failed"):
            binding.synchronize(config)
        assert (target.read_text() if target.exists() else None) == previous
    assert failed.call_count == 2
