# GitHub 事件触发器（P2-7：GitHub webhook → routine → 收件箱）

让 GitHub 仓库里发生的事（新 issue、新评论、新 PR、CI 失败）自动触发一个 Shannon
routine：shannon-server 的 `POST /hooks/github` 端点接收 GitHub webhook 投递，
用 `X-Hub-Signature-256`（HMAC-SHA256）验签后，把投递匹配到
`trigger_type = "github"` 的定时任务并**在 serve 进程内执行**，结果写入
**共享收件箱**（`~/.shannon/inbox.db`）——桌面端与 serve 端看到同一份
收件箱 / 运行历史。

- 端点：`POST /hooks/github`（shannon-server，默认端口 33420）
- 验签：复用全仓唯一的 `shannon-core/src/webhook.rs` 校验逻辑（常量时间比较）
- 不需要 GitHub App、不需要 PAT：**webhook secret 即可**
- 响应契约（冻结）：匹配到 routine → `202 {"runIds":[...]}`；未匹配 → `204`；
  验签失败 → `401`；未配置 secret → `503`

> 前提认知：GitHub webhook 需要一个**公网可达**的 URL。Shannon 本地-first，
> 是否把端点暴露到公网由你决定——§4 给出四种部署形态（含「暂不使用」），
> 全部如实标注安全边界。

---

## 1. 事件映射（第一批，冻结）

routine 通过 `github = { event, repo, action? }` 声明自己关心什么投递；
匹配规则：`X-GitHub-Event` 头 == `event`，`repository.full_name` == `repo`
（`"*"` 表示任意仓库），payload `action` == `action`（省略 = 任意 action）。

| GitHub 投递 | routine 配置 | 说明 |
|---|---|---|
| `issues.opened` | `{ event = "issues", repo = "owner/name", action = "opened" }` | 新 issue |
| `issue_comment.created` | `{ event = "issue_comment", repo = "owner/name", action = "created" }` | issue/PR 新评论 |
| `pull_request.opened` | `{ event = "pull_request", repo = "owner/name", action = "opened" }` | 新 PR |
| `check_run.completed`（仅 `conclusion = "failure"`） | `{ event = "check_run", repo = "owner/name", action = "completed" }` | CI 失败；success/cancelled 等其余 conclusion **不路由**（一律 204） |

其余事件 / action / 仓库组合在第一批**不做映射**：验签通过但没有 routine
匹配时端点返回 `204`，对 GitHub 而言就是成功（不会重试）。

## 2. Secret 配置（两端一致）

1. 生成强随机 secret（≥32 字节十六进制）：

   ```bash
   openssl rand -hex 32
   ```

2. 写入 Shannon 全局配置 `~/.shannon/config.toml`：

   ```toml
   [hooks.github]
   secret = "刚才生成的十六进制串"
   ```

3. 在 GitHub 仓库 **Settings → Webhooks → Add webhook** 里填同一个 secret：
   - Payload URL：`https://<你的公网地址>/hooks/github`（见 §4）
   - Content type：`application/json`
   - SSL verification：启用（tunnel / 反代形态同样应保持 HTTPS）
   - Events：按需勾选 *Issues*、*Issue comments*、*Pull requests*、
     *Check runs*（最小权限：只勾需要的）

未配置 secret 时端点**关闭**（`503` + 文档指引）——这是安全默认。
验签失败一律 `401`，不泄露原因细节。

## 3. routine 侧配置

**方式 A（推荐）：模板实例化。** Templates 库新增
`github-issue-triage.toml`（映射 `issues.opened` 到 issue 分诊汇总
routine）。在 **Tasks → Routines → Browse templates** 里实例化后，
**编辑生成的任务的 `github_repo` 为你自己的仓库**——模板里的
`octocat/your-repo` 只是占位。task.json（`~/.shannon/scheduled-tasks/
<slug>-<id>/task.json`）中会带有：

```json
{
  "trigger_type": "github",
  "github": { "event": "issues", "repo": "你的GitHub用户名/仓库名", "action": "opened" }
}
```

**方式 B：手改 task.json。** 给任意现有任务加上面的两个字段即可
（`enabled` 必须为 `true`；`trigger_type = "github"` 的任务永远不会被
时间调度 `drain_due` 触发，只由 GitHub 投递触发）。

serve 进程在启动时从 `~/.shannon/scheduled-tasks/` 加载一次任务快照；
**启动后新增/修改的 github 任务需要重启 serve 生效**（v1 限制，见 §7）。

执行语义：serve 进程内按既有 `POST /v1/sessions` 同款路径创建会话并运行
routine prompt（自动附上 GitHub 事件上下文：事件类型、仓库、标题、URL），
完成后把结果写入共享收件箱（source=`routine`，可从桌面「收件箱」直接
rerun / continue session），并记录 `routine_runs` 运行记录
（`succeeded` / `failed` 终态；引擎失败或 panic 也保证落到终态）。

## 4. 四种部署形态

| 形态 | 适用 | 安全性 |
|---|---|---|
| A. 公网 VPS serve | 有服务器、要长期稳定接收 | 取决于防火墙/主机加固；务必只开 443 并前置 TLS |
| B. cloudflared / ngrok tunnel | 本机开发、临时演示 | 快速起步；tunnel 提供 TLS |
| C. 反向代理（nginx/caddy） | 已有反代和域名 | 成熟 TLS 运维 |
| D. 暂不使用 | 不想暴露端口 | 最安全；功能关闭，无副作用 |

### 形态 A：公网 VPS 上直接 serve

```bash
shannon serve --host 127.0.0.1 --port 33420
# 机器本身有公网 IP 时，直接让 GitHub 指向 http://<vps>:33420/hooks/github
# 也可以绑 0.0.0.0（需 --allow-non-loopback --auth-token，见下）
```

- 若绑定非回环地址：`shannon serve --host 0.0.0.0 --allow-non-loopback --auth-token <随机串>`。
  `--auth-token` 保护**其它** API（`/v1/*`）；`/hooks/github` 有自己的
  HMAC 验签（GitHub 发不了你的 bearer token），两者互不替代。
- 建议在 VPS 上用 iptables/安全组只放行 443，并以 nginx/caddy 终结 TLS 后
  转发本机 33420（等价于形态 C）。
- `~/.shannon/config.toml`（`[hooks.github] secret`）与任务存储都在这台
  机器上；收件箱也写在这台机器的 `inbox.db` 里。

### 形态 B：cloudflared / ngrok tunnel（本机不动端口）

```bash
# 本机先起 serve（只绑回环即可）
shannon serve --port 33420

# cloudflared（需一个托管在 Cloudflare 的域名，免费）
cloudflared tunnel --url http://127.0.0.1:33420
#   → 输出形如 https://<random>.trycloudflare.com

# 或 ngrok（需 authtoken，免费域名随机）
ngrok http 33420
#   → 输出形如 https://<random>.ngrok-free.app
```

把输出的 HTTPS 地址 + `/hooks/github` 填进 GitHub webhook。tunnel 断开 /
免费地址变化时需要同步更新 GitHub 侧 URL。ngrok 免费层会在请求上加
`ngrok-skip-browser-warning` 之类的页面拦截，浏览器访问需确认，但 webhook
POST 不受影响。

### 形态 C：反向代理

nginx 示例（caddy 同理，自动 HTTPS）：

```nginx
server {
  listen 443 ssl;
  server_name shannon.example.com;
  location /hooks/github {
    proxy_pass http://127.0.0.1:33420;
    proxy_set_header Host $host;
    # 注意：不要改写请求体 —— 验签对象是原始字节
  }
}
```

GitHub webhook URL 填 `https://shannon.example.com/hooks/github`。

### 形态 D：暂不使用

不配置 `[hooks.github] secret` 即可：端点对所有请求返回 `503`，不暴露任何
行为。桌面 loopback 路径（33420 被桌面占用时的另一套触发主路径）不受影响。

## 5. 幂等与重试

- 每个投递带唯一 `X-GitHub-Delivery` id；端点用**有界内存重放缓存**
  （4096 条，先进先出淘汰）去重：重投返回**原 202 runIds，不重复执行**。
- 缓存**不落盘**：serve 重启后忘记已见 id，GitHub 此时的重投可能重复执行
  一次（v1 取舍；收件箱条目会多一条，不会丢）。持久化去重留作后续。
- GitHub 对非 2xx 响应重试（指数退避）。`204` / `202` 都是 2xx，不会引发
  重试风暴；`401` / `503` 不应出现于正常配置。

## 6. 手动验收（无 tunnel 时的最小闭环）

1. 本机起 serve 并配好 secret（§2）：

   ```bash
   openssl rand -hex 32   # → 写入 ~/.shannon/config.toml [hooks.github]
   shannon serve --port 33420
   ```

2. 建一个 github 任务（§3 方式 A/B），`repo` 用 `octocat/hello-world`
   （与下面 fixture 一致），`event = "issues"`、`action = "opened"`。

3. 没有公网地址时用 `ngrok http 33420` 拿到 HTTPS 地址（§4 形态 B），
   填进 GitHub webhook；**或者直接用 curl 模拟 GitHub 投递**：

   ```bash
   # payload.json —— 与真实 GitHub issues.opened 投递同形（可从
   # crates/shannon-server/src/github.rs 的测试 fixture 复制其余三种事件）
   cat > payload.json <<'EOF'
   {
     "action": "opened",
     "issue": {
       "id": 2543928114, "number": 1347,
       "title": "Bug: crash on save with large file",
       "user": { "login": "alice" }, "state": "open",
       "html_url": "https://github.com/octocat/hello-world/issues/1347",
       "body": "Repro: open a 50MB file and hit save."
     },
     "repository": { "id": 1296269, "full_name": "octocat/hello-world" },
     "sender": { "login": "alice" }
   }
   EOF

   SECRET="你的secret"
   SIG="sha256=$(openssl dgst -sha256 -hmac "$SECRET" payload.json | awk '{print $NF}')"

   curl -i http://127.0.0.1:33420/hooks/github \
     -H "Content-Type: application/json" \
     -H "X-GitHub-Event: issues" \
     -H "X-GitHub-Delivery: manual-$(date +%s)" \
     -H "X-Hub-Signature-256: $SIG" \
     --data-binary @payload.json
   ```

   预期：`HTTP/1.1 202 Accepted` + `{"runIds":["…"]}`。

4. 验证闭环：
   ```bash
   # 稍候数秒（引擎失败也会终止运行记录），查询共享收件箱
   sqlite3 ~/.shannon/inbox.db "SELECT id,title,status FROM inbox_items ORDER BY id DESC LIMIT 3;"
   sqlite3 ~/.shannon/inbox.db "SELECT id,task_id,status,error FROM routine_runs ORDER BY started_at_ms DESC LIMIT 3;"
   ```
   桌面端打开「收件箱」页也能看到同一条目（可 rerun / continue session）。

5. 负例自查：篡改签名 → `401`；未配 secret → `503`；换个 `repo` → `204`；
   同 `X-GitHub-Delivery` 重发 → `202` 且 runIds 不变、不重复执行。

## 7. 安全注意事项与已知边界

- **HTTPS 必须**：secret 只防伪造，不防窃听——payload 含 issue 正文。
  三种暴露形态（A/B/C）都应全程 TLS。
- **repo 限定**：给 routine 写精确 `repo`（而非 `"*"`），把影响面压到单仓库；
  `"*"` 只用于明确的全局汇总场景。
- **最小事件集**：GitHub webhook 只勾选需要的事件类型；不需要的别勾。
- **secret 轮换**：GitHub 侧与 `config.toml` 同步更换；泄露迹象出现时立即轮换。
- **serve 内执行边界（与 P0-3 501 裁定的关系）**：本功能是对「serve 进程无
  执行管线」裁定的**定点解除**——只开放「按既有 `POST /v1/sessions` 会话
  创建 + 消息路径执行 routine prompt，结果写共享收件箱」这一条路。
  以下仍然**不在 serve 侧范围**：工具注册（serve 会话是纯 prompt、无工具，
  与 `/v1/sessions` 一致）、超时/重试策略（`ExecutionPolicy.timeout_secs`
  未消费）、任务调度循环、以及 `POST /routines/:id/trigger` 的 501 语义
  （该端点保持原样）。
- **事件不跨进程推送**：`inbox-updated` 是桌面进程内的 Tauri 事件；serve 写
  入的收件箱条目不会实时弹桌面角标，桌面在下次查询（打开收件箱页）时可见。
  数据本身在共享 `inbox.db`，不丢。
- **任务快照**：serve 启动时读一次 `~/.shannon/scheduled-tasks/`；之后新增
  /修改 github 任务需重启 serve。serve 不回写任务（fire_count 等由桌面管理）。
