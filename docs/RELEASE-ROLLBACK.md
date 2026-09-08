# Release 回滚 runbook

适用场景:已发布的版本被发现严重问题,需要把用户安装源回退到上一个好版本。

四个安装通道与各自的回滚动作:

| 通道 | 指向机制 | 回滚动作 |
|---|---|---|
| GitHub `releases/latest` | 自动指向最新「已发布且非 prerelease」的 release | 把坏版本 release 转回 draft(或删除)→ latest 自动回退(§3) |
| R2 `latest/` 镜像 | release.yml publish job 覆盖(2026-08-19 起仅稳定版覆盖) | `just release-rollback <好版本号>`(§2) |
| GitHub release 归档页 | 永久 | 无需回滚;可在坏版本 notes 加警告 |
| 已安装用户 | `shannon update` / 包管理器 | 发布修复版本是最快路径(§4) |

## 1. 判定

- 2026-08-19 起 publish 前有冒烟测试(资产存在性 + linux CLI `--version` + deb 元数据),**冒烟失败的版本不会发布** —— 先确认问题是否发生在发布之后。
- prerelease tag(rc/beta)不会写 R2 `latest/`(B11 防护),坏 prerelease 无需回滚 latest/,直接删 release 即可。

## 2. R2 `latest/` 回滚

前置:`gh` 已登录、Cloudflare 凭据在环境(与 release.yml publish job 相同的 secrets):

```bash
export R2_BUCKET=<bucket>              # 见 repo variables → R2_BUCKET
export CLOUDFLARE_API_TOKEN=<token>
export CLOUDFLARE_ACCOUNT_ID=<id>
```

执行(把 `latest/` 重新指向 0.10.0):

```bash
just release-rollback 0.10.0
```

脚本行为:从 GitHub 下载 `v0.10.0` 的全部资产 → 逐个 `wrangler r2 object put $R2_BUCKET/latest/<name>` 重传。

注意:
- 坏版本仍保留在 `$R2_BUCKET/v<坏版本>/`(版本化路径不动,供追溯)。
- `install.sh` / `install.ps1` 默认走 GitHub `releases/latest`,仅当设置了 `SHANNON_CDN_URL` 指向 R2 时才走镜像 —— 两处都要回滚才算完成。

## 3. GitHub `releases/latest` 回退

`releases/latest` 只看「已发布且非 prerelease」的 release。两个选项:

```bash
# 推荐:转回 draft(保留资产供排查)
gh release edit v<坏版本> --draft

# 或:删除 release(保留 tag 与代码历史)
gh release delete v<坏版本> --yes
```

注意:release.yml 的 `create-release` 对同 tag 幂等 —— **回滚期间不要重推该 tag**(重推会重建 draft 并走完整发布流程)。

## 4. 用户侧

- **install.sh / R2 用户**:§2 + §3 完成后,新安装即刻拿到好版本。
- **已装坏版本的 CLI 用户**:`shannon update` 与 GitHub latest 对齐,但在修复版本发布前它不会主动降级;若坏版本影响面大,发布一个修复版本(`just release-prep <fix>`)是最快路径。
- **包管理器(brew/scoop/winget/AUR)**:清单目前钉版本、未接 CI 自动更新,不受 `latest/` 影响;若坏版本已写入清单需手动改 `packaging/` 下的 version 后提交。

## 5. 演练

test tag 演练(Phase B §B9)时同步演练本 runbook:发布测试 tag → 确认 `latest/` 被覆盖 → 执行 `just release-rollback` 回上一个正式版 → 确认 `install.sh` 拿回旧版。
