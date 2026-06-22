# Releasing Shannon Desktop

> **Status: Deferred (2026-06-19).** Sprint R1 only shipped CI
> (`.gitea/workflows/ci.yml`). There is currently **no** automated
> release pipeline, no updater endpoint, and no signing keypair. To
> cut a build today, run `cargo tauri build` manually and upload the
> artifacts by hand. The flow below is the design agreed during R1
> planning, preserved here so the work can be picked up without
> re-deriving it.

## Current state (2026-06-19)

- **CI:** `.gitea/workflows/ci.yml` gates `main` and `dev` on push and PR.
  Branch protection + CODEOWNERS review is the only merge gate.
- **Release workflow:** none.
- **Tauri updater:** `endpoints: []`, `pubkey: ""` (both empty) in
  `tauri.conf.json`. Clients do not poll for updates.
- **Distribution:** manual `cargo tauri build` + manual artifact upload.
  Preferred future channel is S3 + CDN (token-free public reads for the
  updater manifest) rather than Gitea Releases.

## Planned flow (deferred)

The remainder of this document describes the intended release flow once
the work is prioritized. None of it is wired up yet.

### One-time setup

#### 1. Generate the Tauri updater signing keypair

The auto-updater verifies downloaded updates against a public key. The
matching private key is stored as a CI secret and used by the release
workflow to sign artifacts.

```bash
pnpm tauri signer generate -w ~/.tauri/shannon-desktop.key
# → saves private key + prints the public key
```

You'll be prompted for a password (can be empty).

#### 2. Add CI secrets

In Gitea repo settings → Actions → Secrets → New secret:

| Secret name | Value |
|-------------|-------|
| `TAURI_PRIVATE_KEY` | contents of `~/.tauri/shannon-desktop.key` |
| `TAURI_PRIVATE_KEY_PASSWORD` | the password you chose (or empty string) |

Gate these behind a Gitea Environment with required reviewers so a
workflow change can't exfiltrate the key without human approval.

#### 3. Paste the public key into tauri.conf.json

Replace the empty `"pubkey": ""` in `tauri.conf.json` with the base64
public key printed in step 1. Commit and merge.

#### 4. Wire the updater endpoint

Set `plugins.updater.endpoints` to the S3 + CDN URL that will serve
`latest.json`. Example:

```json
"endpoints": ["https://cdn.diff-lab.com/shannon-desktop/latest.json"]
```

### Cutting a release

```bash
# 1. Bump version (Cargo.toml, ui/package.json, tauri.conf.json — keep in sync)
# 2. Update CHANGELOG.md with the new version section
# 3. Commit
git commit -am "release: v0.3.3"

# 4. Tag and push
git tag v0.3.3
git push origin v0.3.3
```

Pushing the tag will trigger the release workflow (once it exists),
which:

1. Builds native installers for macOS (aarch64 + x86_64), Linux
   (x86_64), and Windows (x86_64).
2. Signs each installer with `TAURI_PRIVATE_KEY`.
3. Uploads artifacts to S3; CDN front-ends the bucket.
4. Writes `latest.json` (the updater manifest) to the same bucket —
   this is what installed clients poll against
   `plugins.updater.endpoints[0]`.

### Verifying

After the release workflow finishes:

```bash
# latest.json should be downloadable and parse-able
curl -sSL https://cdn.diff-lab.com/shannon-desktop/latest.json | jq .

# An installed older build should detect the update within the polling
# interval and prompt to install.
```

### Rollback

There is no automatic rollback. To yank a broken release:

1. Delete or overwrite `latest.json` on the bucket — installed clients
   will stop seeing the update.
2. Delete the offending artifact objects (keeps the git tag).
3. Fix forward and cut a new release.

### Troubleshooting

| Symptom | Likely cause |
|---------|--------------|
| Release workflow fails with "no pubkey" | `TAURI_PRIVATE_KEY` secret missing or empty |
| Built artifacts lack signature | `TAURI_PRIVATE_KEY_PASSWORD` mismatch |
| Clients don't see update | `pubkey` in tauri.conf.json doesn't match private key; or `latest.json` not uploaded |
| macOS universal binary missing | Both arch builds must succeed; check `aarch64-apple-darwin` job |
