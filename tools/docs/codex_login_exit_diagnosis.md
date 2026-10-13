# Codex login and exit on TRUEOS, 2026-10-13

Two faults are supported by this run's logs and the deployed Codex entrypoint.
The earlier pending-reentry explanation does not establish the cause of this
run's Ctrl+C exit failure.

Evidence: `bld/baremetal-logs/LatestOfThree.logs`, resolving to
`trueos-baremetal.0.log`, SHA-256
`3919a0f95d5baf3fb2ae290d500c87c1d6b42ac378046165bfdfba5ac00b6b2c`.
Line references below refer to that capture, before the rotating link changes.

## Login listener failure

The callback server binds `127.0.0.1:1455`. At lines 2839 and 2878,
`tiny_http` reports `Error accepting new client: trueos errno 11 (os error 11)`.
Both browser-login attempts then show `Login server error: Login was not
completed`.

The production call chain is:

1. TRUEOS POSIX `accept4` called the nonblocking Mio accept operation once.
   An empty accept queue returned status -2, translated to EAGAIN, even for a
   blocking listener. The PendingListener-to-MioListener transition discarded
   the descriptor's nonblocking flag.
2. `tiny_http` 0.12.0, `src/lib.rs:340`, logs any accept error, queues it to
   its request receiver, and terminates the accept thread.
3. Codex `codex-rs/login/src/server.rs:212` ends its forwarding thread when
   `server.recv()` fails, dropping the async-channel sender.
4. That channel closing produces `Login was not completed` at line 239.
   The app-server adds `Login server error:`.

This failure precedes the OAuth callback and token exchange. Line 2811 also
records an HTTPS 401 response from the ChatGPT plugin endpoint, establishing
that one HTTPS path worked. It does not verify OAuth token exchange or every
TLS path. Lines 2833 and 2873 report that no local browser was found.

Source inspection found a subsequent compatibility gap: `tiny_http` clones
each accepted TCP stream, while TRUEOS socket fcntl rejected duplication.
That path was not reached in the captured failed accepts.

The shim now retains nonblocking flags, waits through the existing descriptor
poll mechanism for blocking accept/read/write, honors nonblocking operations,
and supports native TCP stream clones. Clones share mode changes and retain
the backend until the final descriptor closes. Codex login logic is unchanged.

## Main returned, but the process did not exit

Lines 2917-2927 record terminal mode restoration. Lines 2928-2933 record
`main returned: Ok(())`. Read-only SSH inspection subsequently showed
`1 codex.bp running`, alongside sqlite and share.

The Blueprint packer injected `#![no_main]` and this entrypoint:

```rust
pub extern "C" fn _start() -> ! {
    main();
    loop { core::hint::spin_loop(); }
}
```

The pre-rebuild object placed `_start` at section 12064, offset zero, matching
the actual runtime entry hint at log line 2204. Its final instructions were
`pause` followed by a jump back to `pause`. Thus returning from main never
reached process exit or the guest runner's post-invocation terminal cleanup.

The generated entrypoint now calls the existing Hull `exit(0)` ABI after
`let _ = main()`. This works for std and no_std entrypoint shims and retains
their existing policy of discarding main's return value. The exit ABI releases
the attached terminal before shutting down the VM. It also avoids returning
into image-release code while background pthread carriers may still run.

## Validation and remaining runtime check

- `test_posix_socket_listener.py`: 16 tests passed, covering blocking waits,
  spurious wakes, nonblocking/DONTWAIT behavior, accepted stream modes, clone
  lifetime and shared flags, plus the existing address/listener checks.
- `test_mio_connect_close.py`: 4 tests passed.
- `test_socket_address_families.py`: 4 tests passed.
- Packer `entrypoint_rewrite` tests: 3 passed, including subprocess execution
  proving that unit- and Result-returning main functions actually terminate.
- `cargo check -p TRUEOS`: passed with existing warnings.
- `TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp codex-cli`: packed the updated
  `TRUEOS-Blueprints/dist/codex.bp`; publication was disabled.

The live rig still runs the original kernel and app. Hardware terminal-return
and authenticated OAuth success remain to be tested with both rebuilt pieces.
For a browser on another machine, the localhost callback also needs a route
to the rig; device-code login avoids that callback routing requirement.
Official guidance: <https://learn.chatgpt.com/docs/auth>.
