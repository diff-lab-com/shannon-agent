# Auto-Updater Setup (P0.3)

Shannon Desktop uses `tauri-plugin-updater` with Ed25519 signed manifests.
This doc covers generating the keypair, configuring CI secrets, and
publishing the `latest.json` manifest.

## 1. Generate the signing keypair (one-time, on a trusted machine)

```bash
cargo install tauri-cli --version "^2" --locked
tauri signer generate -w ~/.tauri/shannon-desktop.key
# Prompts for a password. Save both the password and the key file.
```

Output looks like:

```
Private key written to ~/.tauri/shannon-desktop.key
Public key: dW50cnVzdGVkIGNvbW1l...
```

- **Private key + password**: CI secrets (step 2). Never commit.
- **Public key**: goes into `tauri.conf.json::plugins.updater.pubkey`.

## 2. Configure CI secrets

In Gitea repo settings → Settings → Actions → Secrets, add:

| Secret name                       | Value                              |
|-----------------------------------|------------------------------------|
| `TAURI_SIGNING_PRIVATE_KEY`       | Contents of `~/.tauri/shannon-desktop.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you chose          |

`release.yml` reads these env vars when running `cargo tauri build`.
When present, Tauri emits `.sig` files next to each installer and a
`latest.json` manifest under `target/.../bundle/`.

## 3. Replace the pubkey placeholder

After generating the keypair, replace
`UPDATER_PUBKEY_PLACEHOLDER_REPLACE_WITH_TAURI_SIGNER_GENERATE_OUTPUT`
in `tauri.conf.json` with the actual public key string.

Until this is done, **the updater will reject every manifest** as
signature-mismatched.

## 4. Publish a release

After `release.yml` finishes:

1. Download the `latest.json` artifact from the run.
2. Create a Gitea release tagged `latest` (or update the existing one).
3. Upload `latest.json` as a release asset.
4. Upload all platform installers + `.sig` files as assets.

The endpoint in `tauri.conf.json` points at:

```
https://gitea.diff-lab.com/bigdong89/shannon-desktop/releases/download/latest/latest.json
```

## 5. Verify end-to-end

On a test machine with an older Shannon Desktop installed:

```bash
# Trigger the updater from the dev console:
# (or wait for the app's periodic check)
```

Expected: app fetches `latest.json`, verifies signature with the pubkey,
downloads the new installer for the current platform, and applies it.

## Endpoint format

Tauri supports template variables in endpoints:

- `{{target}}` — e.g. `x86_64-unknown-linux-gnu`
- `{{arch}}` — e.g. `x86_64`
- `{{current_version}}` — e.g. `0.3.6`

For a single manifest covering all platforms, omit the variables and
include all platform entries in one `latest.json` (Tauri filters by
`{{target}}` client-side).

## Why this matters

Without the updater, every Shannon Desktop release requires users to
manually download + reinstall. The updater turns releases into a
no-op for users — they get a notification, click "restart", done.
