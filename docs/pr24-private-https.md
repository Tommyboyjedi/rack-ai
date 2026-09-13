# PR24 private DuckDNS HTTPS operations

Human-facing launcher: **https://gpurack.duckdns.org/**.
Native UI remains **https://gpurack.tailc214fc.ts.net:8444/**.

## Routing and privacy

The DuckDNS A record contains the IPv4 address returned by the installed `tailscale ip -4` CLI, currently `100.116.176.86`. It is never populated by WAN autodetection. The updater accepts only one address in `100.64.0.0/10`, removes unwanted AAAA records and verifies A/AAAA through Cloudflare and Google DNS. DNS-01 TXT updates do not change the A record.

The dedicated nginx listens on that exact Tailscale IPv4 at 443 and proxies only `127.0.0.1:8191`. Raw ComfyUI is not proxied by this hostname. The systemd service allows network traffic only with loopback and Tailscale IPv4 addresses. No LAN/wildcard/public listener, HTTP-01 endpoint, router forwarding or Funnel is configured.

The former Tailscale Serve 443 listener is disabled while nginx serves DuckDNS. Tailscale Serve 8444 is retained unchanged, including its existing private IPv6 listener. No temporary 8443 listener is advertised: Rack's exact Host/Origin validation would reject that third origin. The tested rollback restores the original .ts.net launcher on 443 after stopping nginx.

## Authentication and lifecycle

Rack configuration changes only `public_origin` to `https://gpurack.duckdns.org`. `native_origin`, credentials, gate, resource authority, ComfyUI unit and restart implementation remain unchanged.

The existing operator credential remains at `/srv/rack-ai-media/secrets/operator`; its value was not changed or published.

The two hostnames are different browser cookie domains. A fresh browser signs in at the DuckDNS launcher, then signs in with the same operator credential when Open ComfyUI reaches the retained native domain. The browser uses separate host-only HttpOnly/Secure/SameSite=Strict cookies. No credential is placed in a URL, shared across domains, or injected by nginx. Subsequent cross-site navigation may ask for native sign-in again; keep the native tab open during a session.

Start → Ready → Open, normal Extensions → Restart → Confirm, generation verification, same interactive session/lease and Finish remain Rack lifecycle operations. TLS renewal never restarts Rack AI or ComfyUI.

The existing disposable Director environment/endpoint use the new origin and the same separate credential. Its database is explicitly checked as `/srv/rack-ai-media/director-test/director.sqlite3` before the operator helper changes its endpoint. Production Director data is not modified.

## Operational files and services

The permanent HTTPS root is `/srv/rack-ai-media/https`, within the private media root:

- `ops/`: checked-in helpers from `ops/private_https/`.
- `config/operations.json`: hostname and token-file path only, from the checked-in example.
- `config/nginx.conf`: dedicated proxy configuration; `listen.conf` is generated from Tailscale.
- `acme/certbot/`: private ACME account, certificate archive/live symlinks and renewal configuration.
- `acme/work/` and `acme/logs/`: private Certbot runtime and logs.
- `run/` and `logs/`: nginx PID, private temporary directories and diagnostics.
- `rollback/`: prior media configuration, Director environment and Tailscale Serve snapshot.

DuckDNS token: `/home/tomp/.config/duckdns/token`. Its directory must remain 0700, file 0600, owned by tomp, with no secret symlink. The HTTPS API client reads it directly into memory, uses certificate-verified HTTPS without redirects, never passes it through argv/environment and suppresses token-bearing transport errors. Do not copy token contents into configuration, shell commands, Git or logs.

The system units are:

| Unit | Responsibility |
| --- | --- |
| `rack-ai-private-https.service` | nginx as tomp, only CAP_NET_BIND_SERVICE, private binding and cgroup network restrictions |
| `rack-ai-duckdns.service/timer` | boot-time and six-hour DNS/binding checks; bounded failures |
| `rack-ai-certificate-renew.service/timer` | twice-daily renewal checks, randomized scheduling, persistent missed-run handling |

Distribution `nginx.service` and `certbot.service/timer` stay masked to prevent an unintended default listener or separate renewal job. The dedicated units above own this installation. The packaged cron entry defers to systemd on this host.

If the Tailscale IPv4 changes, DNS maintenance prepares the new explicit binding and, when active, reloads the owned proxy before publishing the new A record. Invalid/unavailable addresses fail closed. Failed rebinds restore the previous generated configuration so a later updater retries the reload before publishing DNS. nginx startup also discovers the current address before validating its configuration.

## Certificates and automatic renewal

Issuer: Let's Encrypt, DNS-01 only, exact SAN `gpurack.duckdns.org`. Certificate/key paths used directly by nginx are:

~~~
/srv/rack-ai-media/https/acme/certbot/live/gpurack.duckdns.org/fullchain.pem
/srv/rack-ai-media/https/acme/certbot/live/gpurack.duckdns.org/privkey.pem
~~~

Certbot's supported automatic authentication and cleanup hooks update DuckDNS TXT records and wait for independent DNS propagation. Although the Certbot plugin is named `manual`, both hooks are configured; issuance and renewal are noninteractive.

Certbot was chosen instead of the preferred acme.sh after reviewing upstream commit `181425b3c8373ca23c0664948b97edf5ed84e9c5`: its stock `dns_duckdns` provider saves the token into account configuration and passes the token-bearing URL to its HTTP command. The direct Python hooks meet this deployment's stricter single-secret-source/no-token-in-argv requirement without maintaining a patched ACME client.

The deploy hook validates certificate trust/hostname/expiry, verifies nginx's systemd command/cgroup/PID, signals only that nginx instance and verifies the served certificate equals the renewed leaf. It does not restart the receiver or ComfyUI. An inactive proxy is not started by certificate renewal. Initial nginx configuration validation runs with the service's scoped binding capability; no global capability or unprivileged-port setting is changed.

No recurring manual certificate steps are needed. Inspect automation using:

~~~sh
systemctl list-timers rack-ai-duckdns.timer rack-ai-certificate-renew.timer
systemctl status rack-ai-duckdns.service rack-ai-certificate-renew.service
journalctl -u rack-ai-certificate-renew.service
~~~

A single staging renewal dry run with deploy hooks is suitable for qualification; do not repeatedly force production issuance.


For initial issuance after installing the hooks and validating private directory ownership, the noninteractive command is:

~~~sh
/usr/bin/certbot certonly --non-interactive --agree-tos --register-unsafely-without-email \
  --server https://acme-v02.api.letsencrypt.org/directory \
  --manual --preferred-challenges dns \
  --manual-auth-hook '/usr/bin/python3 /srv/rack-ai-media/https/ops/dns_hook.py auth' \
  --manual-cleanup-hook '/usr/bin/python3 /srv/rack-ai-media/https/ops/dns_hook.py cleanup' \
  --deploy-hook '/usr/bin/python3 /srv/rack-ai-media/https/ops/proxy_reload.py' \
  --config-dir /srv/rack-ai-media/https/acme/certbot \
  --work-dir /srv/rack-ai-media/https/acme/work \
  --logs-dir /srv/rack-ai-media/https/acme/logs \
  -d gpurack.duckdns.org
~~~

Run issuance as tomp with umask 0077. This deployed certificate already exists; use the renewal service for maintenance. Certbot retains the hook paths in its private renewal configuration. The DuckDNS token is read only from its protected file, never from an argument or environment variable.

Protocol references: [DuckDNS API specification](https://www.duckdns.org/spec.jsp), [Certbot validation hooks](https://eff-certbot.readthedocs.io/en/stable/using.html#pre-and-post-validation-hooks), [Certbot renewal](https://eff-certbot.readthedocs.io/en/stable/using.html#renewing-certificates), and the [reviewed acme.sh provider](https://github.com/acmesh-official/acme.sh/blob/181425b3c8373ca23c0664948b97edf5ed84e9c5/dnsapi/dns_duckdns.sh).

## Installation and changes

Install the helpers and configuration examples into the permanent root, with private directory/secret permissions. Install the five dedicated systemd files from `config/media/https/` as root-owned files in `/etc/systemd/system/`. Mask distribution default services before installing nginx/Certbot so package installation cannot start a default public listener.

Before switching origins: preserve config/Serve snapshots, obtain the trusted DNS-01 certificate, validate the exact private nginx configuration and prepare rollback. Use the existing Rack launcher to Finish/reconcile work and prove Stopped, no lease and no 4080 process. The operator switch refuses an unsafe media state and preserves current state rather than restoring a snapshot.

The switch helper requires sudo for the scoped capability/service/Serve actions and retains tomp ownership on private configuration files. Its nginx preflight drops to tomp with only the binding capability before opening runtime files; do not run a bare root nginx check against these paths. It accepts an already-disabled owned Serve 443 during resumption, rejects foreign/malformed 443 routes and verifies native 8444 before making changes. Allow the restarted receiver a few seconds to become ready:

~~~sh
sudo /usr/bin/python3 /srv/rack-ai-media/https/ops/switch_origin.py duckdns
~~~

This switches only the receiver and the disposable Director instance, enables the private proxy, updates the approved origins and preserves native 8444. It never deletes lease/resource files.

## Tested rollback

After Finish/managed idle release and verified Stopped:

~~~sh
sudo /usr/bin/python3 /srv/rack-ai-media/https/ops/switch_origin.py tsnet
~~~

This disables/stops only the new proxy, restores Tailscale Serve HTTPS 443 → `127.0.0.1:8191`, restores the old launcher origin and disposable Director endpoint/allowlist, then starts the receiver and disposable Director web/worker. Open **https://gpurack.tailc214fc.ts.net/**. Native 8444 is unchanged.

The old endpoint is a tested restoration route, not a second simultaneous launcher origin. DNS/certificate timers may remain enabled during rollback; they cannot start the proxy or restart Rack/ComfyUI. Keep current sessions/jobs/artifacts/credentials/resource receipts. Do not restore stale state or a pre-restart-fix receiver.

Current qualification and exact evidence are recorded in [private HTTPS qualification](pr24-private-https-qualification.md). Earlier permanent installation and restart records remain historical evidence.
