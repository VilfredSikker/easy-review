# The desktop pushes revision events; polling is only a safety net

The desktop backend emits an `er://revision` event whenever its state changes, and the frontend polls in response to that event, coalescing concurrent polls into one. Interval polling made freshness a function of timer length, so every latency complaint was answered by shortening a timer that was never the real lever — backend revision invalidation is. A 30-second timer stays, but only to cover events fired before the frontend's listener has attached.

## Consequences

- The timer is a fallback, not the mechanism. Changing its interval will not make the UI fresher. If a view is stale, the fix is an invalidation the backend is not emitting.
- Backend state changes that skip the revision emit are invisible until the fallback timer fires, so staleness that clears itself after roughly thirty seconds is the signature to look for rather than a slow query.
