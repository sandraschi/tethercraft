# Threat model (PRD section 8, one line per test target)

Derived from RESEARCH.md section 4. Each numbered item is a test target.

1. Device identity: Ed25519 key per device, generated locally; peers pin each other on first pairing (fingerprint shown as words on both sides). Tests: `net::keys` roundtrip, `net::words` fingerprint stability, pairing pin mismatch rejected.
2. Password logins via PAKE (SPAKE2 default); no offline guessing from a captured exchange. Host-side rate limit and lockout. Tests: `net::pake` message flow, wrong password fails, rate limiter locks after N attempts.
3. Transport: QUIC with TLS 1.3 using pinned raw public keys; no plaintext after the first packet, including heartbeats and rendezvous messages. No downgrade path. Tests: `proto` envelope version reject, `net::transport` pinned-key config requires exact key, downgrade attempt rejected.
4. The host enforces every permission on every message. The controller UI is never trusted. Tests: `host::policy` stranger denies input/file/terminal/privacy, per-message check cannot be skipped.
5. Remote configuration is never applied unless signed by a key the host operator pinned, and can never lower local security settings. Tests: unsigned config rejected, signed config that lowers security rejected.
6. URI handler and any IPC only pre-fill a connect dialog; they cannot change settings, import configs or start sessions without a local click. Tests: URI parse never auto-connects.
7. Config exports: AEAD plus Argon2id passphrase; signed. Tests: export format versioned (placeholder until Tier 2 crypto lands).
8. Updates: signed metadata and signed binaries, verified before download and before install. Tests: placeholder (Tier 4).
9. Rendezvous and relay servers see only encrypted payloads; relay cannot read or inject frames. Tests: placeholder (Tier 3, server crate returns NotImplemented).
10. Fuzzing of every parser in CI; path traversal tests for file transfer. Tests: `proto` decode rejects trailing bytes and unknown versions; file path validator rejects `..` and absolute paths (Tier 2 manager reuses it).
11. Scam R1: onboarding gate cannot be skipped in consumer builds; yes or not-sure forces 24 h outgoing-only. Tests: `core::onboarding` fresh install requires answer, yes starts lock window.
12. Scam R2: every new peer is Stranger with view-only, no files/terminal/privacy/blanking/input-blocking/unattended, 30-minute cap, local recording on. Tests: `host::policy` stranger defaults, cap enforced, promotion impossible from inside a session.
13. Scam R7: consumer builds always run R1; no quick-support single exe that skips onboarding. Tests: consumer build flag asserts onboarding present.
