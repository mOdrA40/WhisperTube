# Audit remediation — 23 October 2026

This records the implementation following the read-only audit of commit
54f790762f9d922a70199e4604c51b720b4b0b5e. It does not replace the historical
audit or certify the application as free of defects.

| Finding | Implemented correction | Regression evidence |
| --- | --- | --- |
| WT-001 / P1 | Retain the Unix leader identity until process-group cleanup; cancel only registered processes. Observe leader exit before waiting for pipe EOF on both platforms. | Success/failure with redirected descendants; inherited pipes; timeout/cancel; bounded output; Windows shutdown. Linux executes the production process module in an isolated probe. |
| WT-002 / P1 | Normalize Windows verbatim drive/UNC prefixes for volume comparison only; choose the most specific normalized mount. | Canonical temp paths, future outputs, UNC, mixed prefixes, short nested mounts and component boundaries. |
| WT-003 / P2 | Clear the frontend transcript and stale cache state after a potentially partial reset; preserve it for a pre-mutation operation conflict. | Hook tests cover partial cleanup and conflict, including history reconciliation. |
| WT-004 / P2 | Declare the external JavaScript prerequisite; require supported Node/Deno versions before YouTube inspection/download and show a localized error. | Host boundary tests, exact production JS version probes at minimum/unsupported versions, and frontend error handling. README translations, troubleshooting and release notes explain installation. |
| WT-005 / P2 | Publish flushed result, checkpoint and export files with Unix directory synchronization or Windows write-through replacement; synchronize creation of job/checkpoint directories. | Complete-file replacement, failed staging, Windows long paths and existing export ACL/DACL tests; Unix injected post-rename sync failure propagates as failure. |
| WT-006 / P3 | Report yt-dlp's transfer rate rather than cumulative resumed bytes divided by this invocation's elapsed time. | Finite numeric/scientific rates, zero and unavailable/invalid values. |
| WT-007 / P3 | Distinguish an installed supported GPU accelerator from one available for installation. | Vulkan/Metal installed and absent across all three interface languages. |
| WT-008 / P3 | Report CUDA temporary/staging cleanup failures, continue other cleanup, and retain the original installation outcome. | A Windows locked temporary file produces a warning, other staging is removed, and cleanup succeeds after unlocking. |

Cross-review also corrected Windows descendants retaining inherited pipe handles
despite redirected standard streams, long-path support in the native publisher,
and mount specificity when ordinary and verbatim prefixes differ in length.
The Unix libc dependency was already locked transitively; using it directly
avoids hand-written platform-specific siginfo layouts. No crate version changed.

## Verification and limits

- Windows: cargo fmt check, cargo check, cargo clippy with warnings denied,
  and cargo test, all targets and all features; check/clippy/test use the locked
  dependency graph offline. All 138 Rust tests pass.
- Frontend: TypeScript check, 112 Vitest tests across 16 files, production Vite
  build, nine Node distribution/runtime checks and five isolated NSIS cleanup
  tests pass.
- One concurrent full Rust run hit the existing five-second closed-streams
  fixture timeout. The isolated test, three consecutive process-module suites
  (16 tests each), and the subsequent full 138-test run passed with no assertion
  or timeout change. The transient timeout's cause is unproven; it is not
  presented as a diagnosed and fixed production defect.
- Linux: 17 process/durable module tests pass under Ubuntu WSL2 using the exact
  production source copied into a temporary musl probe. This is not a full Linux
  Tauri test/build or GUI test.
- macOS: the same isolated modules type-check for aarch64-apple-darwin, including
  tests. Native execution, Metal, GUI and packaging remain unverified.
- No real media download/inference, GPU driver fault, clean-machine installation,
  signed updater, physical power-cut or storage-device durability test was run.
  File flush/sync ordering and error propagation cannot establish hardware-level
  power-loss guarantees.
- Node/Deno remains a user-installed prerequisite, not a bundled runtime.
  Existing third-party dependency advisories noted in the audit, including the
  unproven application reachability of glib unsoundness, are not resolved by
  these eight corrections.
- Tests use isolated fixtures; real application data was not reset. Changes are
  left in the working tree; no staging, commit, release or publication occurs.
