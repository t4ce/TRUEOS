# SQLite service foundation measurements

The physical capture `trueos-baremetal.0.log` contains a 34.558-second remote
Execute, almost entirely waiting for its reply header. Other contemporaneous
kernel measurements show delayed consumption of completed work. This does **not**
attribute those 34 seconds to SQLite CPU or image publication. The service is a
native Blueprint in VM 1 and advertises its expected loopback protocol.

An independent synthetic service probe identified unnecessary work: Prepare and
plain read queries copied the entire database image twice, even without a
mutation. The isolated Linux service measurements below use the same host debug
profile, 20 measured requests per operation, and full numeric timing capture.
Both compared versions already coalesce transport frames; this excludes the
Linux transport delay from the image-size-dependent snapshot comparison.

| Image payload | Prepare before | Prepare after | Plain read before | Plain read after |
| --- | ---: | ---: | ---: | ---: |
| 1 MiB | 0.249 ms | 0.092 ms | 0.248 ms | 0.107 ms |
| 16 MiB | 3.459 ms | 0.090 ms | 3.724 ms | 0.119 ms |
| 64 MiB | 48.112 ms | 0.035 ms | 48.749 ms | 0.041 ms |

The 64 MiB case's numeric service trace records **67,186,688 bytes before and
after each request**, or 134,373,376 snapshot bytes, falling to zero on the
optimized plain-read path. Results and exact host executable hashes are in
`bld/sqlite-foundation/framing.json` and `final.json`. These are host service
measurements, not a physical CLI before/after claim.

The conservative path remains available for all potentially stateful compiler
actions. SQLite's authorizer initially permits only Read, Select and Recursive.
Any PRAGMA, function, write, DDL or unknown action rejects preparation before its
effects, captures the original image, and retries the same batch cursor under
the existing policy. This preserves PRAGMA effects that can occur during
preparation and preserves scripts whose later statements depend on earlier DDL.
All SQL functions retain the conservative snapshot cost. No SQL text classifier
or function-name whitelist is used.

Explicit transactions retain their original image until completion. A successful
outer commit or autocommit mutation still publishes before acknowledgement;
failed publication restores the original image. Beginning a transaction moves
its snapshot into session ownership instead of cloning it again. Owner
reclamation remains bounded to at most 64 pages per eligible request; the
synthetic 72 MiB freelist case reclaimed one page and retained a valid image.

Client and server use the public alloc-only JSON serializer, then make one
overlapping payload move within the same Vec to insert the four-byte big-endian
length. They issue one complete frame write. This adds one payload move and
avoids a second frame buffer. The old Linux service's roughly
41 ms ping floor fell below 0.1 ms in this fixture. Native kernel sockets already
disable Nagle, so that Linux floor is not evidence about the physical 34-second
wait. Coalescing still removes a separate write boundary in both directions.

Optional unsigned connection/request identifiers correlate client and service
timing; older requests and peers remain supported. The service emits one fixed
numeric line per admitted sample, without SQL, filenames, values or errors.
Normal capture admits the first 16 requests; subsequent slow/error or periodic
candidates share a 250 ms gate, with suppressed candidates reported as `skipped`.
AppTrace does not bypass that gate. Explicit `TRUEOS_SQLITE_TIMING_ALL=1` is used
by the synthetic benchmark.

The final validation includes:

- Six service unit tests: compiler read effects, PRAGMA/function capture,
  mixed-script retry, legacy/current frames, identifier types and sampling.
- Nine remote adapter tests, including truncated response bodies, persistent
  upgraded connections and unchanged setup deadlines.
- Real service tests for mixed-script failure rollback, PRAGMA preparation and
  execution followed by failure, interruption, publication failure, read-only
  sessions, bounded reclamation, oversized frames and EOF during a transaction.
- Existing SQLx typed values, nested/drop/disconnect rollback, concurrent writes,
  all six Codex migration sets, restart and integrity checks using the legacy
  client, plus current-client history/checkpoint/deletion integration.

The initially conservative compiler guard exposed a no-op `DROP TABLE IF EXISTS`
case with no authorization action but `stmt.readonly() == false`. The whole SQLx
suite caught its trusted snapshot being blocked by the guard's own internal
page-count PRAGMA. Both snapshot paths now remove the temporary guard before
trusted serialization, while retaining the existing authorization policy. The
mixed-script regression explicitly starts with that no-op statement.

Run the bounded synthetic benchmark with:

```sh
python3 tools/testpy/test_sqlite_service_foundation.py --service /path/to/sqlite --report /tmp/sqlite-foundation.json
```

The probe runs against new synthetic databases and does not access the physical
rig or account data. Its artifact JSON is machine-readable and contains sizes,
operation labels, timings, executable hashes and contract results.
