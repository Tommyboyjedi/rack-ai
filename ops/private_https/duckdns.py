"""DuckDNS HTTPS transport: token never enters argv, environment or logs."""
import http.client
import os
import stat
from urllib.parse import urlencode
from settings import validate_tailnet_ip

API_HOST = "www.duckdns.org"
REQUEST_TIMEOUT = 20
MAX_RESPONSE = 1024

class DuckDns:
    def __init__(self, settings):
        self.settings = settings

    def _token(self):
        path = self.settings.token_file
        for target, expected in ((path.parent, 0o700), (path, 0o600)):
            metadata = target.lstat()
            if stat.S_ISLNK(metadata.st_mode) or metadata.st_uid != os.getuid():
                raise RuntimeError("DuckDNS secret ownership is invalid")
            if stat.S_IMODE(metadata.st_mode) != expected:
                raise RuntimeError("DuckDNS secret permissions are invalid")
        value = path.read_text().strip()
        if not 20 <= len(value) <= 256 or not all(c.isascii() and (c.isalnum() or c == '-') for c in value):
            raise RuntimeError("DuckDNS secret format is invalid")
        return value

    def _request(self, parameters):
        # Every operation explicitly sets a safe IP or uses the distinct TXT API.
        if "ip" not in parameters and "txt" not in parameters:
            raise RuntimeError("Refusing DuckDNS IPv4 autodetection")
        if "ip" in parameters:
            validate_tailnet_ip(parameters["ip"])
        query = dict(parameters, domains=self.settings.hostname.removesuffix(".duckdns.org"),
                     token=self._token())
        connection = http.client.HTTPSConnection(API_HOST, timeout=REQUEST_TIMEOUT)
        try:
            connection.request("GET", "/update?" + urlencode(query))
            response = connection.getresponse()
            body = response.read(MAX_RESPONSE + 1)
            if response.status != 200 or body.strip() != b"OK":
                raise RuntimeError("DuckDNS rejected the update")
        except Exception:
            # Transport exception strings can contain request URLs. Never propagate them.
            raise RuntimeError("DuckDNS HTTPS update failed; response details withheld") from None
        finally:
            connection.close()

    def set_ipv4(self, address):
        self._request({"ip": address})

    def clear_addresses(self, address):
        self._request({"ip": address, "clear": "true"})

    def set_txt(self, value):
        self._request({"txt": value})

    def clear_txt(self):
        self._request({"txt": "", "clear": "true"})
