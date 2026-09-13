# PR24 human browser passwords

Open **https://gpurack.duckdns.org/** and sign in with your personal password. The browser stays signed in for **30 days**. The launcher provides **Change password** and **Log out** alongside its existing ComfyUI controls.

## First use

If no personal password has been configured, the Password form shows: “Use the existing operator credential once, then set a password.” Sign in with that existing credential, choose **Change password**, enter the existing credential as Current password, and enter/confirm your own memorable passphrase.

Use at least 12 characters; long passphrases and spaces are supported. No uppercase, number, symbol or periodic-rotation rule is imposed. The server bounds passwords to 1024 UTF-8 bytes and the form accepts up to 256 characters for a new password. Existing machine credentials cannot be selected as a new personal password.

After the first successful save, the bootstrap note disappears and browser login accepts the personal password only. The operator bearer credential continues to authenticate authorized API requests, but can no longer be used as a browser password. Codex does not choose or record the user's eventual password.

The native UI retains **https://gpurack.tailc214fc.ts.net:8444/**. It uses the same personal password but a separate host-only cookie. A fresh browser signs in on that domain separately; no API credential is needed after a personal password has been set. Keep the native tab open while working. The deployment does not claim cross-domain single sign-on.

## Change password and logout

The authenticated `/account` page requires Current password, New password and Confirm new password. Password changes require an authenticated browser cookie and an exact configured Origin. An API bearer credential alone cannot change the browser password.

A successful change writes a freshly salted password hash and rotates the authentication generation before issuing a fresh 30-day cookie. All other browser cookies, including the native domain's cookies, become invalid. This remains true if receiver restart or an interrupted write occurs before stale session metadata is pruned. Current work, interactive sessions, managed jobs and GPU leases are not cancelled.

`POST /logout` revokes the current server-side browser session and expires its cookie. It requires an exact allowed Origin; GET does not log out. Logout applies to the current domain/session. Password change or local recovery revokes all browser sessions on both domains.

Already-open native browser WebSockets recheck their session before forwarding browser data and at most every two seconds while idle. Revoked/expired connections close. Bearer-authenticated API/native connections keep their existing authentication behavior.

## Password storage and bounded verification

The permanent configuration sets:

~~~json
"browser_auth_file": "/srv/rack-ai-media/secrets/browser-auth.json"
~~~

The parent directory is private mode 0700 and the record is created mode 0600. The record contains only the encoded **Argon2id v19** password hash plus operator identity and an opaque revocation generation. Salt is included in the encoded hash; plaintext passwords are never persisted or logged.

Current KDF policy: **64 MiB, three iterations, one lane, 16 random salt bytes and 32 output bytes**, implemented by RustCrypto Argon2. Verification uses the library's constant-time password verification and accepts only the bounded deployed KDF parameters. Passwords are not SHA-256 hashed; the existing SHA-256 digests remain exclusively the machine-token/session-identifier mechanisms.

The private atomic writer creates its temporary file with restrictive permissions, flushes and syncs it, atomically renames it and syncs the parent directory. Login, password changes and local recovery serialize on the same bounded authentication lock. Corrupt, malformed, symlinked, oversized or improperly permissioned password records fail closed at browser authentication; they do not enable bootstrap. A corrupt password record does not disable valid API bearer authentication.

Configurations predating this field remain compatible: their isolated fallback is `state_root/browser-auth/auth.json`, with a newly created private directory. No test configuration defaults to the live secret path.

[Argon2 specification](https://www.rfc-editor.org/rfc/rfc9106.html) and [RustCrypto implementation](https://docs.rs/argon2/0.5.3/argon2/) describe the underlying KDF. No custom cryptographic primitive is implemented.

## Sessions and login throttling

The random browser cookie is `Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000` on the deployed HTTPS origins. Server-side expiry uses the same 30-day constant. Only a digest of the random session value is persisted; no password or API bearer token enters a URL, HTML, localStorage or sessionStorage. Loopback-only HTTP fixtures retain the explicit existing test exception for Secure transport.

Existing pre-migration browser sessions retain their original expiry until a new login/password change issues a fresh cookie. Their legacy generation is accepted only in initial bootstrap mode. Setting a password revokes them.

One bounded KDF attempt may run at a time across the launcher and native login endpoints. Excess concurrent attempts receive 429 with Retry-After. Repeated invalid passwords progressively back off for 1, 2, 4, 8, 16 and at most 30 seconds. Rejected requests during a cooldown do not extend it. A successful authentication clears the failure state; inactivity expires it. This is a fixed-size in-memory single-operator policy, with no growing per-IP map or permanent account lockout. API status/job authentication and the supervisor do not wait for the password KDF.

## Local recovery

Recovery requires shell access as the deployment owner on gpurack. It has no remote unauthenticated reset route and never accepts a replacement password in argv.

~~~sh
/srv/rack-ai-media/current/rack_ai_media_admin /srv/rack-ai-media/config.json reset-browser-password
~~~

This atomically replaces only the human authentication record with bootstrap mode and a new revocation generation. Existing browser cookies stop working. Machine/API credentials and media/job/session/resource state remain unchanged; do not manually delete any of that state.

Return to the launcher, use the existing protected operator credential once, and set your own password. A brief existing throttle cooldown may require waiting up to 30 seconds. No receiver, ComfyUI or rack restart is required for password recovery.

## Deployment and rollback

Build/install the receiver, client and local admin executable together in an isolated permanent release. Preserve the prior release/configuration, deploy only from a verified stopped backend and restart only the media receiver. Keep the ComfyUI unit, Manager flags, resource authority, private HTTPS, renewal services and native origin unchanged.

Once a human password exists, use an authentication-aware release for rollback. A pre-password receiver does not understand authentication generations and must not silently be restored over an installation using personal passwords. The pre-migration release is an emergency rollback only while the installation is still deliberately in bootstrap mode and its compatible configuration is restored. Never restore stale browser or media state to undo a password change.

The live qualification deliberately leaves an unconfigured installation in bootstrap mode, verifies the existing credential and account UI, and lets the operator select the final password interactively. Password creation/change/revocation/recovery use synthetic disposable fixtures; no live personal password is invented.

See [password qualification](pr24-password-qualification.md) for implemented, fixture-tested and deployed results.
