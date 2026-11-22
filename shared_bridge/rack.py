"""Minimal RackAI public-API client. The bearer token is never logged."""
import json
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


class RackAIError(RuntimeError):
    """A RackAI call that did not return a usable result."""

    def __init__(self, status, reason):
        super().__init__(reason)
        self.status = status
        self.reason = reason


def _reason(body):
    if isinstance(body, dict):
        detail = body.get("detail")
        if isinstance(detail, dict) and detail.get("reason"):
            return str(detail["reason"])
        if body.get("error"):
            return str(body["error"])
    return "rackai_error"


class RackAI:
    """The five existing RackAI operations the bridge is allowed to use."""

    def __init__(self, config):
        self._url = config.runtime_url
        self._token_path = config.token_path
        self._timeout = config.request_timeout

    def _call(self, payload):
        token = self._token_path.read_text(encoding="utf-8").strip()
        request = Request(
            self._url,
            data=json.dumps(payload).encode("utf-8"),
            headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
        )
        try:
            with urlopen(request, timeout=self._timeout) as response:
                body = json.load(response)
        except HTTPError as error:
            try:
                body = json.loads(error.read())
            except (ValueError, OSError):
                body = {}
            finally:
                error.close()
            raise RackAIError(error.code, _reason(body)) from None
        except (URLError, OSError) as error:
            raise RackAIError(0, "rackai_transport_failed") from error
        return body["result"]

    def discover(self):
        return self._call({"operation": "discover"})

    def reserve(self, request):
        return self._call({"operation": "reserve", "request": request})

    def inspect(self, reservation_id):
        try:
            return self._call(
                {"operation": "inspect_reservation", "reservation_id": reservation_id}
            )
        except RackAIError as error:
            if error.status == 404:
                return None
            raise

    def release(self, reservation_id):
        return self._call(
            {"operation": "release_reservation", "reservation_id": reservation_id}
        )

    def cancel(self, reservation_id):
        """Close the owned reservation without retaining its backend warm."""
        return self._call(
            {"operation": "cancel_reservation", "reservation_id": reservation_id}
        )
