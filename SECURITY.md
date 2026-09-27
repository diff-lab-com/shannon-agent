# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities **privately** — do not open a public GitHub issue.

**Preferred channel: GitHub private vulnerability reporting** — use the
*"Report a vulnerability"* button at
<https://github.com/diff-lab-com/shannon-agent/security/advisories>. This is
private, tracked, and lets us coordinate a fix and advisory without exposure.

If GitHub reporting is unavailable, open a private discussion with the
maintainers via [GitHub contact requests](https://docs.github.com/en/account-and-profile/setting-up-and-managing-your-github-user-account/managing-access-to-your-personal-repositories) and reference this repository.

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
  an explicit opt-in and an `auth_token`. Even on loopback, any process — or any local
  web page, for browser-borne attacks — that can reach the port can drive the engine
  unless an `auth_token` is configured; WebSocket upgrades reject cross-site `Origin`s.
- LLM provider credentials are stored as `0600` files under `~/.shannon/credentials/`
  on the local machine only. IM channel credentials use the OS keyring. Never commit
  or copy credential files anywhere.

## Supported versions

Only the latest released line receives security fixes.
