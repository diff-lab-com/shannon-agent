# ADR-0008 交互式 QA 清单(合并前过堂)

> 配套:`provider-model-command-remediation.md` 签收块。这 20 条是代码路径已确认、但需在交互式 REPL 里做人肉验证的**纯行为项**。预计 ~15 分钟。
>
> **怎么用**:逐条跑命令,观察结果。通过就 `[x]`;失败就 `[ ]` 并在下方记现象,回来修。
> 行号锚点指向 `provider-model-command-remediation.md`(本目录)。

---

## Block A — 无需凭证(启动 REPL 即可)

启动:`cargo run`(或 `shannon`)。空白首屏。

- [ ] **A1 · 首屏卡片真实 tier**(plan L84):首屏 StatusCard 对 catalog 内模型显示真实 tier——`claude-haiku-4-5`→`fast`、`claude-sonnet-4`→`standard`、`claude-opus-4-8`→`pro`。不再是 `?`。
- [ ] **A2 · StatusBar 与卡片 tier 一致**(plan L124):对同一模型,底部 StatusBar 胶囊 `[provider/model · tier]` 的 tier 与首屏卡片 tier 字面一致。
- [ ] **A3 · `/model <id>` 即时刷新卡片**(plan L101):空白首屏执行 `/model claude-sonnet-4`,回车后卡片 provider/model/tier **立即**更新(无需重启/重开)。
- [ ] **A4 · catalog 内模型无 warning**(plan L245):`/model claude-haiku-4-5` 无 `⚠ not in catalog` 提示。
- [ ] **A5 · `/model typo-id` 有 warning 仍设置成功**(plan L244):`/model some-typo-id` 出现 `⚠ 'some-typo-id' is not in the catalog; using as-is`,且模型仍被设置(state 切到它)。
- [ ] **A6 · `--tierfoo` 不误进 tier handler**(plan L305):`/model --tierfoo xxx`(或 `--tier=foo` 的畸形式)不被当成 `--tier` 处理——应报未知参数/原样走,而非切到某 tier。
- [ ] **A7 · `/disconnect <p>` 变未连接**(plan L195):对某个已连接 provider 执行 `/disconnect <p>` 后,首屏卡片与 `/connect` dashboard 显示该 provider 为**未连接**。
- [ ] **A8 · 断开当前 provider 自动切换**(plan L196):对**当前正在用**的 provider 执行 `/disconnect`,自动切到下一个已连接 provider(或回到未配置态),不卡死。
- [ ] **A9 · 连接状态词汇一致**(plan L164):对同一 provider,`/connect`、`/provider`、首屏卡片三处显示的连接状态词**完全相同**(来自同一 `ProviderConnectionStatus` 枚举,不再各自造词)。
- [ ] **A10 · `/profile` preset 别名仍可用**(plan L215):`/profile list`(或 `/profile` help)仍工作;help 文案把旧 `profile` 叫法标注为已废弃/重命名。
- [ ] **A11 · 静态 catalog 行为不变**(plan L358):未连网/未 refresh 时,首屏与 picker 仍列出静态 `MODEL_CATALOG` 内的全部 catalog 模型。
- [ ] **A12 · 切换后行为对齐**(plan L261):切换模型后,`/context`(或状态显示)的 context_window、preferences、首屏卡片三者一致。(注:此项核心已被单测覆盖,这里只看可观察表面是否打架。)

---

## Block B — 需要已配置 provider + 真实 key + 联网

前置:`~/.shannon/credentials/<service>.json` 有有效 key,网络可达。

- [ ] **B1 · `/connect` 后卡片显示新连接**(plan L102):空白首屏执行 `/connect anthropic sk-ant-...`,关掉模型 picker 后,卡片显示新连接的 provider/model。
- [ ] **B2 · `/connect` 后立即用新 key 发消息**(plan L143):`/connect <p> <key>` 完成后,**不重启**直接发一条消息,能正常收到回复(凭证已热加载);成功提示无"restart"字样。
- [ ] **B3 · `/model refresh` 不阻塞输入**(plan L180):`/model refresh` 立即返回(输入框不卡),后台跑完后在 chat 区输出刷新结果。
- [ ] **B4 · refresh 后首屏列表与 picker 一致**(plan L357):`/model refresh` 完成后,首屏卡片模型清单与 `/model` picker 的清单一致(都含动态层 models.dev 结果)。

---

## Block C — 需要断网

- [ ] **C1 · 离线静默回退**(plan L182):断网后执行 `/model refresh`(或触发首屏加载),**不报错崩溃**,静默回退到静态 catalog;界面仍可用。

---

## Block D — CLI(终端,非 REPL)

- [ ] **D1 · CLI providers 子命令行为不变**(plan L321):`shannon providers list` / `shannon providers add <p>` / `shannon providers remove <p>` 行为与整改前一致(写入经 `ProviderConfigService` 单一路径,`providers.toml` round-trip 正确)。
- [ ] **D2 · 行为不变(同一 toml 解析)**(plan L276):`providers.toml` 经新 `connected_provider_slugs` 路径解析的结果与旧实现一致。(注:本质是单测覆盖项;CLI 侧可观察 `list-providers` 输出无差异即可。)

---

## 完成后

- 全 `[x]` → 回 `provider-model-command-remediation.md` 把对应 20 条勾上,更新签收块"21 条"→"0 条待验",开 PR(`fix/provider-model-command-remediation` → `dev`)。
- 任一 `[ ]` 失败 → 记现象,修代码 + 新提交,重验该条。
