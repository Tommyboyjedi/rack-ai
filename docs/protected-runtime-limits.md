# Protected Runtime Limits

RackAI runtime token, response-byte, and timeout limits are operator-approved execution boundaries. They are not tuning knobs for hiding malformed model behavior, protocol mismatches, response accounting defects, or cancellation defects.

Protected-settings warning:

"Changing token/response/time limits is not a substitute for diagnosing excessive generation, protocol errors, or cancellation defects. Changing operator-approved limits requires explicit operator approval."

Current protected baseline for the local ATHBA/RackAI/JCode workspace path:

- `local-primary.context = 131072`
- `local-primary.max_input_tokens = 65536`
- `local-primary.max_output_tokens = 65536`
- RackAI `limits.max_response_bytes = 3145728`
- Tester and Developer workspace budgets remain the existing 300-second client budgets.

If an operation generates excessive output, exceeds the durable evidence envelope, ignores cancellation, or produces invalid protocol data, diagnose the first incorrect execution boundary. Do not lower token limits, raise response budgets, increase timeouts, add hidden per-turn caps, or reinterpret these values without an explicit operator decision and fresh qualification evidence.
