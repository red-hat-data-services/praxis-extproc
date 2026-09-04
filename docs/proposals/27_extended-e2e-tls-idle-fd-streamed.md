---
issue: https://github.com/opendatahub-io/praxis-extproc/issues/27
discussion: https://github.com/opendatahub-io/praxis-extproc/issues/27
status: proposed
authors:
  - henschwartz
graduation_criteria:
  - How? section added after the What? and Why? direction is accepted
  - Open questions closed in Decisions before How? (long-running test tier
    mechanism, chained ext-proc scope for 3.6)
  - Idle-timeout success bar recorded in Decisions (gate before How?; see
    Decisions below)
  - A separate long-running e2e tier exists, is excluded from default CI,
    has a documented manual or scheduled trigger, a hard maximum runtime,
    retained machine-readable results, and an assigned failure owner
  - >-
    Release qualification requires executing the long-running tier (manual
    run or release pipeline) before tagging; failures block release unless
    explicitly waived with a recorded reason and owner approval
  - >-
    Authenticated TLS with certificate validation is exercised — valid
    handshakes succeed; Envoy rejects untrusted, expired, and wrong-SAN
    server certificates; Praxis rejects untrusted, expired, and missing
    client certificates when mTLS is enabled; client SAN/hostname validation
    is required (CA-chain verification alone is insufficient); each run
    records the effective validation mode; TLS-only runs do not count toward
    production qualification unless explicitly documented as an exception
  - FULL_DUPLEX_STREAMED single ext-proc scenarios assert first-chunk
    delivery before upstream EOS, multiple client chunks, final completion,
    and request-body integrity via an independent oracle across empty,
    single-chunk, multi-chunk, exactly-10-MiB, and above-cap inputs;
    expected behavior when the documented buffer cap is exceeded must be
    defined
  - Stale-idle scenarios establish a connection before the idle window,
    disable retries for the assertion, record connection reuse, send exactly
    one post-idle request per cycle, and sweep idle / pool / keepalive
    variants; qualification pass/fail follows the idle-timeout success bar
    in Decisions (reproduce on reused connection or documented clean matrix;
    root-cause proof not required for v1)
  - >-
    Single-hop qualification targets FULL_DUPLEX_STREAMED (STREAMED as an
    optional fallback); BUFFERED request + FULL_DUPLEX_STREAMED response is
    documented as a fallback/regression scenario only, not a v1 production
    mitigation path
  - Chained FULL_DUPLEX_STREAMED body-integrity scenarios run when the
    declared production topology requires chaining; each run publishes
    `chain=executed` or `chain=deferred` with a reason; qualification fails
    when chaining is required but the chained scenario did not execute
  - Mode comparison across BUFFERED, STREAMED, and FULL_DUPLEX_STREAMED
    request/response combinations documents regression signal for qualification
  - >-
    Each qualification run publishes machine-readable metadata — Envoy,
    Istio, and Praxis versions or image digests; effective TLS and
    certificate-validation configuration; mode matrix; idle thresholds; retry
    policy; and connection-pool settings
stakeholders:
  - crstrn13
  - shaneutt
  - alexsnaps
---

# Extended E2E: TLS Idle Timeout, FULL_DUPLEX_STREAMED, and Chained Ext-Proc

## What?

The Forge-based Kubernetes e2e suite
([#21](https://github.com/opendatahub-io/praxis-extproc/issues/21))
covers baseline ext-proc behavior in CI: plaintext, single ext-proc hop,
and BUFFERED / STREAMED body modes. That is the right default for
fast feedback, but it does not exercise two risks surfaced by the
[praxis-proxy/ai#459](https://github.com/praxis-proxy/ai/issues/459)
spike and tracked in
[#27](https://github.com/opendatahub-io/praxis-extproc/issues/27):

1. **Stale connection failures after long idle** — first requests after
   minutes of quiet traffic can fail while later requests self-heal;
   observed with MaaS-style configs targeting Praxis instead of the Go
   IPP. Root cause is not confirmed (TLS session reuse, upstream pool
   keepalive, intermediate LB idle timeout, and similar hypotheses remain
   open).
2. **Request-body loss with chained `FULL_DUPLEX_STREAMED` ext-proc** —
   when two or more ext-proc filters are chained in full-duplex streamed
   mode, Envoy can deliver `EndOfStream` with zero request bytes to the
   downstream processor
   ([envoyproxy/envoy#44605](https://github.com/envoyproxy/envoy/issues/44605)).
   Response streaming is unaffected. A standalone repro exists at
   [crstrn13/ext-proc-tests](https://github.com/crstrn13/ext-proc-tests);
   praxis-extproc needs qualification in its own topology.

This proposal adds a **second e2e tier**: longer-running, TLS-qualified
scenarios that run **outside regular CI**, built on the
[#21](https://github.com/opendatahub-io/praxis-extproc/issues/21)
harness and
[#3](https://github.com/opendatahub-io/praxis-extproc/issues/3) TLS/mTLS
server support. It does not replace the baseline suite; it extends
coverage for production-adjacent failure modes before MaaS / 3.6 rollout.

### Scope of scenarios (directional)

Operators and maintainers should be able to run (locally or in a
scheduled/nightly job) scenarios that cover at least:

**Stale idle / TLS**

- Reproduce or narrow the post-idle first-request failure with Praxis in
  the e2e cluster (not only the Go IPP baseline).
- Run chat completion and streaming workloads over **authenticated TLS**
  (and **mTLS** when the declared topology requires client certificates)
  between Envoy and Praxis, including negative certificate cases (see
  Goals).
- For each idle / pool / keepalive variant: **establish and record** an
  upstream connection, **disable client and Envoy retries** for the
  assertion, wait through a configurable idle window (on the order of
  5–10 minutes to start), then send **exactly one** request on the reused
  connection. Assert **connection reuse** and request success. Repeat the
  cycle across idle durations and Envoy upstream connection-pool /
  keepalive settings to characterize failure thresholds.

**`FULL_DUPLEX_STREAMED` (single ext-proc)**

- Use a **delayed multi-chunk upstream**. Assert **first-chunk delivery
  before upstream EOS**, observe multiple client chunks, and assert final
  completion through one ext-proc in full-duplex streamed mode (not merely
  eventual delivery after buffering).
- Validate **request-body integrity** through a single full-duplex hop
  using an **independent oracle**: the client generates known **length and
  SHA-256** values; compare them at the processor boundary and against an
  upstream echo or recorded backend payload. Cover **empty**, **single-
  chunk**, **multi-chunk**, **exactly 10 MiB** (per
  [architecture.md](../architecture.md) body cap), and **above-cap**
  inputs; define expected behavior when the documented 10 MiB buffer cap
  is exceeded.

**Chained ext-proc (declared topology)**

- Maintain an explicit **topology fixture or inventory** (single-hop vs
  chained ext-proc, mode matrix). Each qualification run must publish
  `chain=executed` or `chain=deferred` with a reason.
- When the **declared production topology requires chaining**, exercise
  request-body integrity through the chain under conditions that reproduce
  [envoyproxy/envoy#44605](https://github.com/envoyproxy/envoy/issues/44605),
  comparing the client oracle digest at **every hop**. Qualification
  **fails** when chaining is required but the chained scenario did not
  execute.

**Body-mode qualification (v1 migration target)**

- Qualify **single-hop** `FULL_DUPLEX_STREAMED` as the primary mode aligned
  with Go IPP → Rust migration; **`STREAMED`** may be exercised as an
  optional fallback where needed.
- Document **BUFFERED** request + **FULL_DUPLEX_STREAMED** response as a
  **fallback / regression** scenario (for example chained-topology
  workarounds discussed in
  [praxis-proxy/ai#459](https://github.com/praxis-proxy/ai/issues/459)),
  not a recommended production inference path — Envoy buffer limits make
  full request buffering a poor fit for large streaming workloads.
- Compare BUFFERED, STREAMED, and FULL_DUPLEX_STREAMED combinations on
  request and response paths for **regression signal** only.

### Test tier

These scenarios are **long-running** (multi-minute idle waits, longevity
runs, concurrency sweeps). They must **not** gate default PR CI, but they
**must** participate in **release qualification** — executed manually or via
a release pipeline before tagging, with failures blocking release unless
explicitly waived with a recorded reason and owner approval. The exact
mechanism (Cargo feature flag, `#[ignore]`, separate crate, nightly
workflow only, etc.) is an open question for Decisions / How?; the
contract here is **separation from the
[#21](https://github.com/opendatahub-io/praxis-extproc/issues/21) fast
path** plus an **executable, bounded operation contract**:

- A **named command or workflow** invocation (implementation left to How?)
- A **manual or scheduled trigger** policy
- A **hard maximum runtime** per qualification job
- **Retained machine-readable results** and an **assigned owner** for failures

Each run must also publish reproducibility metadata: Envoy / Istio /
Praxis versions or image digests; effective TLS and certificate-validation
configuration; mode combinations exercised; idle thresholds; retry policy;
and connection-pool settings.

### Goals

- Extend the
  [#21](https://github.com/opendatahub-io/praxis-extproc/issues/21) e2e
  harness with a **documented long-running tier** that has a named
  invocation, trigger policy, timeout, retained artifacts, and owner
- Require the long-running tier to run as part of **release qualification**
  (manual or release pipeline) before tagging; failures block release
  unless explicitly waived with a recorded reason and owner approval
- **Reproduce or narrow** stale-connection behavior using connection-reuse
  evidence per the **idle-timeout success bar** in Decisions (not
  burst-and-retry tests that mask pool refresh)
- Exercise **authenticated TLS** with certificate validation: valid
  handshakes; Envoy rejection of untrusted, expired, and wrong-SAN server
  certificates; Praxis rejection of untrusted, expired, and missing client
  certificates when mTLS is enabled; require client SAN/hostname validation
  (CA-chain verification alone is insufficient); record effective validation
  mode per run; exclude TLS-only qualification unless documented as an
  exception
- Qualify **single-hop** `FULL_DUPLEX_STREAMED` (with **STREAMED** as an
  optional fallback) using pre-EOS streaming evidence and **independent**
  request-body integrity (client length + SHA-256 oracle at processor and
  upstream boundaries)
- Exercise **chained** full-duplex streamed ext-proc body integrity when
  the declared topology requires it; publish `chain=executed` or
  `chain=deferred` with reason; fail qualification when chaining is
  required but not executed
- Document **BUFFERED** request + **FULL_DUPLEX_STREAMED** response as a
  **fallback / regression** path only (not v1 production mitigation)
- Compare **BUFFERED**, **STREAMED**, and **FULL_DUPLEX_STREAMED**
  request/response combinations for regression and qualification evidence
- Publish **machine-readable run metadata** so passes are reproducible after
  upgrades
- Produce results maintainers can use for release qualification and for
  upstream Envoy / platform discussions (without fixing Envoy in this repo)

### Non-Goals

- Replacing or slowing the **baseline**
  [#21](https://github.com/opendatahub-io/praxis-extproc/issues/21) CI
  scenarios (BUFFERED / STREAMED, plaintext, single ext-proc)
- Fixing
  [envoyproxy/envoy#44605](https://github.com/envoyproxy/envoy/issues/44605)
  inside praxis-extproc (upstream Envoy defect; tests may document and
  guard against regression)
- Running multi-hour longevity or 1000-stream concurrency sweeps in
  **default** CI (spike-scale runs remain manual or scheduled)
- Implementing new ext-proc protocol features beyond what
  [#3](https://github.com/opendatahub-io/praxis-extproc/issues/3) already
  covers for TLS/mTLS
- Defining MaaS 3.6 production topology
  ([#26](https://github.com/opendatahub-io/praxis-extproc/issues/26));
  this proposal qualifies ext-proc behavior under **declared** topologies
  via fixture/inventory, not production rollout decisions
- Duplicating or relocating this tier into
  [opendatahub-io/opendatahub-tests](https://github.com/opendatahub-io/opendatahub-tests);
  platform-level suites may invoke these scenarios later, but ext-proc
  harnesses, fixtures, and mode matrices belong in this repository

### Open Questions

1. **Long-running tier wiring.** Feature flag vs `#[ignore]` vs separate
   crate vs dedicated GitHub Actions workflow — what is the default
   developer and nightly operator experience? (Named command, timeout, and
   artifact retention are required regardless of mechanism.)
2. **3.6 chained ext-proc scope.** Will production chain two or more
   ext-proc filters in `FULL_DUPLEX_STREAMED` mode? This sets the default
   `chain=executed` vs `chain=deferred` expectation in the topology
   fixture.

### Decisions

Resolved in this proposal (must be accepted before the How? PR begins).

**Idle-timeout success bar.** The stale-idle bug is not yet reproduced in
this harness and its root cause is unconfirmed. For **v1 of this tier**,
**pass** means one of:

1. **Reproduce** the post-idle first-request failure on a **reused**
   connection (retries disabled, connection-reuse evidence recorded), with
   the idle / pool / keepalive / TLS parameter set published; or
2. **Document clean pass** across the configured idle / pool / keepalive
   matrix (failure not observed under those conditions, with the same
   evidence and metadata).

**Pass does not require** eliminating the failure in production configs or
**proving** a single root cause (TLS session reuse vs Envoy pool vs
intermediate LB) for v1 graduation. Narrowing via the parameterized matrix
and published run metadata is sufficient; root-cause attribution may follow
in platform or follow-up work.

How? must implement assertions and reporting against this bar — not an
undefined “first request succeeds” check that retries or fresh connections
can satisfy.

**Ext-proc body modes (v1).** Go IPP → Rust migration targets
**`FULL_DUPLEX_STREAMED`** as the primary qualification mode; **`STREAMED`**
is an optional fallback. **BUFFERED** request + **FULL_DUPLEX_STREAMED**
response is a **documented fallback / regression** scenario (for example when
chained full-duplex exposes
[envoyproxy/envoy#44605](https://github.com/envoyproxy/envoy/issues/44605)),
not a v1 production mitigation — buffer limits make full request buffering
unsuitable for all inference workloads
([praxis-proxy/ai#459](https://github.com/praxis-proxy/ai/issues/459)).

**Repository placement.** Scenario definitions, topology fixtures, and the
[#21](https://github.com/opendatahub-io/praxis-extproc/issues/21) harness
extension stay in **praxis-extproc**. Cross-component platform qualification
in
[opendatahub-io/opendatahub-tests](https://github.com/opendatahub-io/opendatahub-tests)
may consume results later; this proposal does not move ext-proc-specific
fixtures out of this repo.

**Release qualification.** The long-running tier does **not** gate default PR
CI. It **does** gate **release qualification**: maintainers run it manually or
via a release pipeline before tagging. Failures **block** release unless
explicitly waived with a recorded reason and owner approval. How? wires the
exact workflow hook; this decision locks the release contract.

## Why?

### Motivation

The
[praxis-proxy/ai#459](https://github.com/praxis-proxy/ai/issues/459)
spike showed that praxis-extproc handles long AI response streams well in
isolation — BUFFERED, STREAMED, and FULL_DUPLEX_STREAMED modes survived
extended runs, scaled linearly to high concurrency, and respected timeouts.
That confidence does not extend to two production-shaped risks:

**Idle and TLS.** MaaS deployments keep Envoy↔processor connections warm
across long quiet periods. Intermittent “first request after idle fails,
then recovery” behavior is exactly the class of incident that is hard to
catch with short CI tests and expensive to debug in production. A burst
test with retries enabled can pass even when pooled connections are stale,
because Envoy or the client silently opens a fresh connection. TLS
introduces session reuse, certificate validation, and handshake paths that
plaintext
[#21](https://github.com/opendatahub-io/praxis-extproc/issues/21) tests do
not cover. Without reproducible e2e scenarios that record connection reuse
and validation mode, operators cannot tell whether Praxis, Envoy pool
settings, or platform networking is at fault.

**Full-duplex streamed request bodies.** MaaS inference often streams
responses while still sending non-trivial request bodies. Full-duplex
streamed mode is attractive for latency, but a test that only checks
eventual delivery cannot distinguish true streaming from buffering until
EOS. Envoy also has a known defect when **multiple** ext-proc filters use
full-duplex streamed mode on the request path — bodies can vanish silently.
Even if 3.6 uses a single Praxis ext-proc, teams need evidence that
single-hop **FULL_DUPLEX_STREAMED** is safe (pre-EOS chunks, independent
body oracle). **BUFFERED**-request / streamed-response combinations may be
documented as a **fallback** for chained-topology regression, but are not
the v1 production target given buffer limits. Chained tests with an explicit
topology declaration protect against topology drift and prevent qualification
gaps when a second ext-proc is added later.

Baseline
[#21](https://github.com/opendatahub-io/praxis-extproc/issues/21) tests must
stay fast. This work belongs in a **separate tier** with bounded runtime,
retained artifacts, and reproducibility metadata so qualification depth does
not trade off against every PR’s CI time.

### User Stories

These are stakeholder needs derived from
[#27](https://github.com/opendatahub-io/praxis-extproc/issues/27);
they are not separate tracked issues.

- As a maintainer, I want long-running TLS and idle-timeout e2e scenarios
  that prove connection reuse so that we can reproduce or rule out
  stale-connection failures before MaaS rollout.
- As a maintainer, I want `FULL_DUPLEX_STREAMED` qualification with
  pre-EOS chunk delivery and independent request-body oracles so that
  streaming inference configs are evidence-based, not assumed from the
  spike alone.
- As a platform engineer, I want chained ext-proc regression tests with
  an explicit topology declaration (`chain=executed` / `chain=deferred`)
  so that Envoy body-loss behavior is visible before we compose multiple
  processors.
- As an operator, I want **FULL_DUPLEX_STREAMED** qualification evidence
  (with documented **BUFFERED**-request fallback scenarios where needed)
  so that streaming inference configs are evidence-based without assuming
  full request buffering for every workload.
- As a release owner, I want these scenarios outside default CI but required
  for release qualification (manual or pipeline), with named invocation,
  timeouts, retained machine-readable results, and an owner, so that
  qualification runs are repeatable, accountable, and gate tagging when they
  fail
