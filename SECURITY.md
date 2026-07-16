# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities **privately** — do not open a public GitHub issue.

Email: **security@shannon-agent.dev** (replace with your real address before going public)

Include:
- A description of the issue and its impact
- Steps to reproduce / a proof of concept
- Affected versions or commits
- Suggested fix (optional)

We will acknowledge within 72 hours and aim to publish a fix and advisory within 30 days,
coordinating disclosure with you.

## Scope

Shannon runs shell commands, filesystem operations, and external tool calls on behalf of the
user. By design it executes with the invoking user's privileges. Vulnerabilities that bypass
the permission/approval system, leak secrets across sessions, or allow a chat-platform
message to trigger unapproved destructive actions are **in scope and high priority**.

## Threat model notes

- The gateway bridges external chat platforms to the engine. Only authorized users
  (configured per-platform) may drive the agent; verify your platform allow-lists.
- The `api_server` binds to loopback by default. Binding to non-loopback interfaces requires
  an explicit opt-in and an `auth_token`.
- Secrets (API keys, platform tokens) are stored in the OS keyring, never in the repo or
  plaintext config.

## Supported versions

Only the latest released line receives security fixes.
