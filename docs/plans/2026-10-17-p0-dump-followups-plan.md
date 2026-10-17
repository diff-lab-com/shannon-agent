# Fast-follow 实施计划:装配 dump 可达性与保真度(2026-10-17)

> **Track**: PR #429(装配树 dump)遗留跟进项的"桶 1"批次,经 ericdong 2026-10-17 批准执行
> **Date**: 2026-10-17
> **Status**: Approved(用户指令:worktree 实施 → PR 合并)
> **输入**: #429 终审/任务评审的 deferred minors(T2⑦、T3⑨⑩⑪⑬)+ 裁决跟进项(serve 前置)
> **分支/PR**: `feat/p0-dump-followups` → `dev`(基线 62cbc2f1e)

## Global Constraints

1. **行为兼容**:既有路由/旗子/字段语义不变;新增仅限"暴露面扩大"(Task 1 新路由)与"信息更全"(Task 2 字段回退与 notes)。
2. **门禁**(沿用并含上轮补的 doc 关卡):`cargo fmt --check`;`cargo clippy -p <touched>... --all-targets -- -D warnings`;`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p <touched>...`;touched crates 的 `cargo nextest run` 全绿。
3. 每任务 1 个 commit;不跑全仓测试;不派生子代理;不盲信行号(以符号定位)。
4. insta 金样若因结构变更失配,重新 accept 并一并提交 `.snap`。

---

## Task 1 · `shannon serve` 前置 `GET /api/assembly`

**背景**:#429 的 `/api/assembly` 仅在 `ShannonApiServer`(embedder 路径,`crates/shannon-core/src/api_server.rs`,经 `with_extra_routes` + `AssemblyRouteState` + `with_assembly_sources`)暴露;终审确认 `shannon serve` 走的是 `shannon-server` crate 的 axum 栈(`run_with_allow_nonloopback`),**不前置该路由**。远程/headless 用户恰恰是装配 dump 最有用的场景(本地用户已有 `--dump-assembly`)。

**要求**:
1. 先调查 `shannon serve` 的实际 HTTP 面:serve 进程内可用状态有哪些(`ToolRegistry`/`HookManager`/`QueryEngineConfig`/skills 是否构建),shannon-server 栈与 core api_server 栈的关系(并存?替代?)。
2. 在 serve 前置的栈上新增 `GET /api/assembly`,复用 `build_assembly_dump` 与 `AssemblyRouteState` 模式(不重复实现);serve 时可得的来源接线,拿不到的来源照旧降级 `unavailable`+`partial:true`(skills 在 serve 上下文若不可得,如实降级并在 notes 注明,不为此扩构建面)。
3. 鉴权/CORS/origin 中间件与该栈既有路由同层;新路由 smoke 测试(可用来源断言 + 降级路径断言)。
4. 若调查发现 serve 实际也经 core api_server(即终审结论有误),则改为在该暴露路径接线并在报告澄清——以实测为准。

**验收**:serve 前置栈的 `/api/assembly` smoke 测试绿(含降级);既有路由测试全绿;报告说明 serve 状态面与接线决策。

---

## Task 2 · dump 保真度批次(core/engine/cli)

**要求(五项,均小)**:
1. **T3⑨ http/llm 钩子 payload 回退**:`assembly_dump.rs` `hooks_section` 目前只读 `def.command`,`HookDef::new_http/new_llm`(shannon-engine hooks/config.rs)把目标放在 `url`/`prompt_template` → 非命令钩子渲染成空。改:`command` 为空时回退 `url`,再回退 `prompt_template`;字段 doc 注明"命令,或 http/llm 钩子的 URL/prompt 载荷";更新/重受 golden;新增单测(http 与 llm 各一)。
2. **T3⑪ load 失败可见化**:shannon-engine `HookManager::load()`(manager.rs ~159-163)坏文件 warn-and-skip 但无订阅者时静默。改:manager 增加私有 `load_failures: Vec<String>`(文件路径+错误摘要),load 循环记录;新增只读访问器 `load_failures()`;assembly dump 的 hooks 段新增 `load_failures` 数组(经 hooks 来源透出);单测:临时目录放坏 JSON → load_failures 非空、dump 可见。既有行为不变(warn 仍在)。
3. **T3⑬ 旗子优先级 help**:CLI `--dump-config` 增加一行 long help:"与 --dump-assembly 同传时,--dump-config 先处理并退出"。
4. **T2⑦ 语义表措辞**:semantics.rs Notification 行 notes "已枚举但从未在仓库中构建" → "无生产环境生产者(仅测试构造)"(与英文风格一致则用英文,与该文件其余 notes 语言一致)。
5. **T3⑩ dump 披露**:assembly_dump 模块 doc 增加一句:"钩子命令与 URL/prompt 载荷会原文出现在 dump 输出与 `/api/assembly` 中——把 dump 视为可能含钩子配置中的机密。"

**验收**:五处 diff 各有对应测试或 doc 断言;golden 同步;shannon-engine/core/cli 既有测试全绿。

---

## 执行顺序

Task 1(需调查判断)→ Task 2(机械批次)。两任务都触 core,串行。分支 3 个 commit:plan + 2 任务。
