# ADR 0003: Session and cancellation policy

- **Status:** Proposed default, 2026-11-02. This is a product decision with a
  real trade-off, so the owner should confirm or reverse it.

## Context

Two requirements pull against each other:

- Every opening starts a **fresh, independent, unsaved** session.
- Dismissing the popup **must not silently destroy** an answer that is still
  being generated.

## Decision

1. **Open** creates a new in-memory `Session` with no messages. Nothing is
   written to disk by default.
2. **Dismiss while idle or after a finished answer** drops the session.
3. **Dismiss while streaming** (default policy `finish_in_background`): the
   request keeps running, bounded by the configured timeout and maximum output
   tokens, and the result is held in a single in-memory *detached* slot.
4. **Next open** always shows a fresh session. If a detached answer exists, a
   one-line hint offers `Ctrl+R` to view it. Submitting a new question cancels
   and drops any detached request, so at most one request is ever in flight.
5. The detached slot expires after 10 minutes (configurable) and is dropped on
   quit. It is never written to disk.
6. `Esc`, clicking outside and the global shortcut all **dismiss**. Only the
   **Stop** control (and `Ctrl+.`) **cancels** generation, which aborts the HTTP
   request.
7. Setting `cancel_on_dismiss = true` aborts generation on dismissal instead, for
   users who prefer never paying for output they did not read.
8. Persistent history, reopening the last session and a system prompt are
   opt-in settings, off by default, and out of scope for the MVP.

## Trade-offs

- Background completion can consume tokens the user never reads. It is bounded
  by timeout and max tokens, and `cancel_on_dismiss` is one setting away.
- A detached answer lingers in process memory for up to the TTL. That is the
  cost of not destroying work, and it is never persisted.
- Cancelling on dismiss would be simpler and cheaper but is exactly the silent
  destruction the requirements exclude.

## Testing

Pure state-machine logic in `popupai-core`, covered by unit tests: fresh session
per open, no inheritance of context, detach on dismiss, single in-flight request,
TTL expiry, cancel on Stop.
