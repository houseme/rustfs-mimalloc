# Changelog

## 0.5.6 - 2026-10-16

- First standalone release of the sampling allocator, bounded recorder, optional
  live heap tracking, symbolization, executable mappings, and pprof encoder.
- Provide an exclusive `Profiler` session with explicit stop and Drop cleanup.
- Use shared inclusion-probability weights, round after stack aggregation, and
  preserve interval allocation counters through compatible consumers.
- Require reporter-visible admission, count rejected records, use a FIFO TLS
  ring, and keep the unsampled path to a byte countdown.
- Support published allocator 0.5.6 and workspace allocator 0.6 through their
  unchanged `GlobalAlloc` interface. Direct native allocations remain outside
  this Rust wrapper's coverage. Buffer pressure can omit samples.
