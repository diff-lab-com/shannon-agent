# tree-sitter Repo Map (P1-4)

> 状态:Planning · 估时:2–3w 单人(Rust 优先)· 优先级:P1-4 编码线最高 · 依赖:无
> 父计划:[`docs/improvement-plan-2026-08.md` §2 P1-4](../improvement-plan-2026-08.md)
> 关联 P2-1(Compact 多策略,需 repo map 辅助摘要)· 不依赖 P1-5(auto-test loop 是写路径,本任务是只读)

## 1. 目标

Shannon 当前缺一个 repo-wide 符号地图给 LLM 当系统 prompt 上下文。竞品(Claude Code / Codex CLI / OpenCode / Reasonix)都有,这是继 auto-test loop 之后的第二大缺口。本计划交付一个 `shannon-repomap` crate:

- 用 **tree-sitter** 解析工作区源码,产出 **SymbolMap**(类 / trait / 函数 / 类型 / 调用签名)
- 按 **token 预算**(默认 2–4k,可在 `.shannon.toml` 调)用 **Aider-style PageRank + token pack** 选最相关符号
- **增量** 监听(`notify`)+ **mtime 缓存**,首次扫全量,之后只重解析变更文件
- 注入到 `shannon-core::QueryEngine` 的 system prompt,使 REPL / CLI / headless 三端都受益
- **语言优先级**:Rust(主)→ TypeScript → Python → Go,后续可扩 Java / C / C++

**非目标**(本计划不做):
- 跨语言 LSP `textDocument/definition` 联动(交给 P3-5 Deep LSP)
- 自动测试 / 自动修代码(那是 P1-5)
- IDE 内符号 hover / go-to-def(走 LSP,本任务只产"地图",不做编辑器)

---

## 2. 架构(高层)

```
┌────────────────────────────────────────────────────────────────────────────┐
│  Filesystem events  ─────►  notify::Watcher  ─────►  invalidation queue   │
│                                                       │                   │
│                                                       ▼                   │
│  .rs/.ts/.py/.go  ─────►  tree-sitter parser (per-language grammar)        │
│                          │  rust  │  typescript  │  python  │  go          │
│                          ▼                                                   │
│                     SymbolNode(kind, name, signature, span, refs)          │
│                          │                                                   │
│                          ▼                                                   │
│                   SymbolMap  (per-file, cached)  ──►  mtime-keyed LRU        │
│                          │                                                   │
│                          ▼                                                   │
│                   PageRank over call/import graph (whole workspace)         │
│                          │                                                   │
│                          ▼                                                   │
│                   TokenBudgetSlicer  ──►  ~2–4k tokens  ──►  formatted text  │
│                                                                             │
│  query_engine.execute(prompt)  ──►  shannon-core::ContextInjector hook       │
│          │                              │                                   │
│          └──── append_system_prompt ◄────┘                                  │
└────────────────────────────────────────────────────────────────────────────┘
```

**数据流**(单次 query):
1. `QueryEngine::execute(user_prompt)` 触发已有 `ContextInjector` 链(已在 `engine.rs:670` 接管 project instructions / preference memory)
2. 新增 `RepoMapInjector`(同 lifecycle): 读 `RepoMap::for_workspace(cwd)`(缓存命中直接返回;否则后台先返回空图,下次 query 重试)
3. `append_system_prompt(&repo_map_slice)` 注入到 `engine.config.system_prompt`
4. 默认开启,可由 `[repo_map] enabled = false` 在 `.shannon.toml` 关闭

---

## 3. 文件锚点

### 3.1 新增 crate:`crates/shannon-repomap/`

| 文件 | 职责 | ~LOC |
|---|---|---|
| `Cargo.toml` | workspace 成员声明;依赖见 §3.2 | 60 |
| `src/lib.rs` | 公开 API:`RepoMap`, `SymbolMap`, `Language`, `TokenBudget` re-exports | 80 |
| `src/language.rs` | `Language` 枚举(Rust/TypeScript/Python/Go),按文件扩展名 dispatch | 120 |
| `src/parser.rs` | per-language grammar 入口;产出 `ParsedFile` | 200 |
| `src/symbol_tree.rs` | `SymbolNode`(kind/name/sig/span/file_id),`SymbolMap` 聚合 | 250 |
| `src/cache.rs` | mtime-keyed LRU(`hashbrown` 或 `lru` crate);持久化 toml/sled 见 §7 决策 | 200 |
| `src/watcher.rs` | `notify` wrapper,debounce 100ms,channel 推 invalidation | 180 |
| `src/pagerank.rs` | 在 call/import 边上跑 PageRank 收 100 轮;只针对 top-N 节点 | 150 |
| `src/budget.rs` | token 预算 slicer(估算: 1 token ≈ 4 chars);按 PR 排序贪心 | 200 |
| `src/inject.rs` | 格式化为 markdown 文本(system prompt 子段) | 120 |
| `src/queries/{rust,typescript,python,go}.scm` | tree-sitter query 文件(参考 nvim-treesitter 模板) | ~200/语 |
| `tests/fixtures/rust_small/` | 1 个 fixture repo 用于单测(symbol 数 < 200) | 4–6 文件 |
| `tests/repomap_integration.rs` | 端到端:解析 → 预算 → 注入文本断言 | 200 |
| `tests/bench_cache.rs` | 增量 vs 全量 benchmark(`criterion` 可选) | 100 |

**总规模**:~1900 LOC(不含 fixture)。

### 3.2 Cargo.toml 依赖

```toml
[dependencies]
# workspace 基础
tokio = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
parking_lot = "0.12"
hashbrown = "0.15"

# 解析
tree-sitter = "0.25"
tree-sitter-rust = "0.23"
tree-sitter-typescript = "0.23"  # 拆出 typescript + tsx 子 crate
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"

# 文件监听(workspace 内已用)
notify = "6.1"
notify-debouncer-mini = "0.4"

# LRU 缓存
lru = "0.12"

# 路径 / glob(workspace 已用 ignore crate)
ignore = "0.4"
walkdir = "2.5"

[dev-dependencies]
tempfile = { workspace = true }
insta = "1.41"            # snapshot for formatted inject output
mockito = "1.6"           # 不需要,纯解析无 HTTP
criterion = "0.5"         # benchmark(可选)
```

> **依赖收紧**:tree-sitter 三方 grammar 体积约 1–2MB 编译产物(可接受)。**不开** C 动态链接,全部静态拉入避免运行时 dlopen。

### 3.3 修改清单

| 文件 | 变更 | 行为 |
|---|---|---|
| `Cargo.toml`(workspace 根) | `members` 数组加 `"crates/shannon-repomap"` | crate 编译进工作区 |
| `crates/shannon-core/Cargo.toml` | 加 `shannon-repomap = { workspace = true }` | 引擎可调 |
| `crates/shannon-core/src/query_engine/engine.rs` | `QueryEngine::new()` / `with_config()` 增加 `with_repo_map(RepoMapConfig)` builder;`execute()` 钩到 ContextInjector 链 | 注入 |
| `crates/shannon-core/src/query_engine/context_injector.rs` | 新增 `RepoMapInjector` 实现(同 trait);`QueryEngine` 初始化时按配置装载 | 钩子 |
| `crates/shannon-core/src/config/unified_config.rs` | `[repo_map]` 段 schema:`enabled: bool`, `max_tokens: usize`, `ignore: Vec<String>`, `languages: Vec<Language>` | 用户可配 |
| `crates/shannon-core/src/lib.rs` | `pub use shannon_repomap` re-export(按 feature flag,可关) | 控制可选性 |

**不修改**:
- `shannon-ui` / `shannon-cli` / `shannon-agent` —— 注入只发生在 `QueryEngine`,REPL/CLI 走既有路径自动受益
- 任何 LLM provider 代码 —— prompt 注入是 `shannon-core` 内,透明

---

## 4. 实施步骤(分两阶段)

### Phase A — Rust only spike(1w)

> 目标:用最小可工作切片证明"tree-sitter 能解析 + 预算算法管用 + 能注入并被 LLM 看到"。所有多语言、Persistence、SSE 监听都留到 Phase B。

#### Day 1–2 · crate scaffold + Rust 解析

1. **建 crate**:`crates/shannon-repomap/Cargo.toml` + `src/lib.rs`;workspace 根 `members` 加成员。`cargo check -p shannon-repomap` 应通过空 lib。
2. **依赖**:拉 `tree-sitter` + `tree-sitter-rust`(其它 grammar 暂不拉,验证体积假设)。
3. **写 `Language` 枚举 + 扩展名 dispatch**(`src/language.rs`):
   - `Language::Rust`,`from_path(&Path) -> Option<Language>`:仅识别 `.rs`。
   - 单测:5 个 fixture path(包含 `.rs`/`.toml`/`.md`)返回正确。
4. **写 `parser.rs`**:
   - `parse_file(path: &Path, lang: Language) -> Result<ParsedFile>`:tree-sitter `Parser::new()` + `set_language(...)` + `parse(source)`,跑 Rust 专用 query(`src/queries/rust.scm`,参考 nvim-treesitter)。
   - `ParsedFile { tree, source, symbols: Vec<SymbolNode> }`。
5. **写 `symbol_tree.rs`**:
   - `SymbolNode { kind: SymbolKind, name: String, signature: String, file: PathBuf, span: Range, refs: Vec<RefEdge> }`
   - `SymbolKind = Function | Struct | Enum | Trait | Impl | Const | Static | Use | TypeAlias | Module`
   - `SymbolMap { files: HashMap<PathBuf, FileSymbols> }` 聚合。
6. **Fixture repo**:`tests/fixtures/rust_small/` —— 6 个 .rs,涵盖 fn / struct / enum / trait / impl / use,`pub`/`pub(crate)`/`fn`(无 receiver)/`fn(&self)` 都覆盖。
7. **单测**:`tests/parser_test.rs` —— 解析 fixture,断言 symbol 数 + 名称 + 签名,固定 snapshot。
8. **Day 1 末验证**:`cargo test -p shannon-repomap` 全绿;`cargo clippy --workspace -- -D warnings` clean。

#### Day 3 · PageRank + token budget

9. **写 `pagerank.rs`**:
   - 输入:`&SymbolMap`,从 `use` / `impl Trait for X` / 函数体里的 identifier 引用粗略构造有向图(精确 call graph v2 再做,Phase A 只用 import + visibility 拓扑)。
   - 算法:经典 PageRank(damping=0.85, 100 iter),起点为 `pub` 且在 `src/lib.rs` / `src/main.rs` 出现的符号。
   - 输出:`Vec<(SymbolId, f64)>` 按 PR 降序。
10. **写 `budget.rs::TokenBudget::slice(sym_map, ranked, max_tokens)`**:
    - 估算每个符号签名 + 一行摘要 ≈ `signature.chars() / 4` tokens(Aider 经验值)。
    - 贪心:按 PR 序加入,直至超预算;最后做"重要性"或"group by file"压缩(参考 Aider pack 的 render)。
    - 输出 `BudgetSlice { text: String, used: usize, dropped: usize }`。
11. **单测**:`tests/budget_test.rs` —— 6 文件 fixture,~30 symbol,设 `max_tokens=2000`,断言 `used <= 2000` 且 `dropped > 0`(说明算法工作)。

#### Day 4 · mtime 缓存 + 增量

12. **写 `cache.rs::SymbolCache`**:
    - `HashMap<PathBuf, (SystemTime, FileSymbols)>` + `parking_lot::RwLock`。
    - `get_or_parse(path, lang)`:比对 mtime,命中即返,否则解析并更新。
    - 持久化(Phase A 用 in-memory;Phase B 决策 sled/leveldb 落盘)。
13. **微 benchmark**:`tests/bench_cache.rs`(轻量,`std::time::Instant`):
    - 场景:6 文件 fixture,全量解析 → 时间 `T_full`;重复 10 次同样输入,带 mtime cache → 时间 `T_cached`。
    - 断言:`T_cached < T_full / 5`(目标 5x 加速)。

#### Day 5 · 接 query_engine + 开关

14. **改 `shannon-core/src/query_engine/engine.rs`**:
    - 在 `QueryEngineBuilder` 加 `with_repo_map(RepoMapConfig)` 与 `with_repo_map_disabled()`。
    - `execute()` 路径中,若开启,调 `RepoMap::for_workspace(cwd, &config)`,得到 `BudgetSlice`,通过 `append_system_prompt(&slice.text)`。
    - `[repo_map] enabled = false` 时(默认 true for Rust-only)跳过。
15. **改 `context_injector.rs`**:
    - 新增 `pub struct RepoMapInjector`,实现既有 `ContextInjector` trait(参 `engine.rs:670` 现有用法)。
    - `QueryEngine::new()` 初始化时按 config 装载。
16. **改 `unified_config.rs`**:
    - 加 `[repo_map]` 段: `enabled: bool = true`, `max_tokens: usize = 3000`, `ignore: Vec<String> = ["target/**", "node_modules/**", ".git/**"]`, `languages: Vec<String> = ["rust"]`。
    - 单测覆盖默认值与 override。
17. **集成测试**:`crates/shannon-core/tests/repomap_inject.rs`:
    - 起一个 `QueryEngine` 配 `with_repo_map`,mock provider(用 mockito + `tests/api_integration.rs` 已有的 server 模式)收首条 message 的 system prompt。
    - 断言 system prompt 含 "## Repository Map" 节 + Rust fixture 中至少一个 symbol 名(`MyStruct`)。
18. **End-of-week 验收**:
    - `cargo nextest run -p shannon-repomap`:全绿,≥20 单测。
    - `cargo nextest run -p shannon-core --test repomap_inject`:全绿。
    - `just dev`(`cargo clippy -- -D warnings` + `cargo fmt -- --check` + `cargo nextest run --workspace --exclude shannon-desktop`)clean。
    - 手测:`cargo run -- repl` 在 shannon-mono 自己目录,问"where is the `RepoMap` configured?",首条 system prompt 应包含 `crates/shannon-repomap/src/lib.rs:RepoMap` 一行。

### Phase B — 多语言 + 增量优化(1–2w)

> **tree-sitter-versions (Phase B 实施备注)**:实际钉版本在 Phase B 落地时
> 与本计划 §3.2 示例块略有偏差,记录如下,供后续 Phase C / 升级时参考:
>
> - `tree-sitter = "0.24"`(workspace `Cargo.toml` 已是 0.24.7;§3.2
>   示例块的 0.25 是早期草稿,aliyun 镜像未刷新 0.25.x 配套 grammar,
>   0.25 升级推迟到镜像刷新后)。
> - `tree-sitter-rust = "0.23.3"` `tree-sitter-typescript = "0.23.2"`
>   `tree-sitter-python = "0.23.6"` `tree-sitter-go = "0.23.4"` — 全部 0.23.x
>   末班线,统一消费 `tree-sitter-language = "0.1"` FFI bridge;这与
>   `tree-sitter 0.24` runtime 配对,正是 Phase A 的 0.24/0.23 配对。
> - `tree-sitter-typescript` 单 crate 内同时暴露 `LANGUAGE_TYPESCRIPT` 和
>   `LANGUAGE_TSX`;Phase B 把 `.ts`/`.tsx` 都路由到 `LANGUAGE_TYPESCRIPT`
>   (TSX 简化为按 TS 处理,JSX-aware 抽取留 Phase C),`.py`/`.pyi` 都路由
>   到 `LANGUAGE`,`.go` 走 `LANGUAGE`。
> - 未来升级到 0.25.x 配对时,需要先确认 aliyun 镜像已同步
>   `tree-sitter 0.25` runtime + 对应 0.25.x grammar 套件,然后一次性
>   bump 五个 dep;半套升级会出现 FFI 桥版本不匹配的运行时报错。

#### Week 2 · 多语言 + 用户配置

19. **加 tree-sitter-typescript + tree-sitter-python + tree-sitter-go**:
    - 注意 `tree-sitter-typescript` 提供两个 language:`typescript` 和 `tsx`;按 `.ts`/`.tsx` 选。
    - `src/language.rs` 加 `Language::TypeScript(LangVariant)` / `Python` / `Go` + 扩展名映射。
    - 每语配 `.scm` 查询(可借鉴 nvim-treesitter / Helix 的 query,简化为只抽 top-level decl,内嵌引用留给 Phase C)。
20. **fixture 扩展**:`tests/fixtures/{ts_small,py_small,go_small}/` 各 4–6 文件。
21. **per-language 单测**:`tests/parser_ts_test.rs` / `parser_py_test.rs` / `parser_go_test.rs`,断言各语种 symbol 抽取正确。
22. **决策点 1(见 §7)落库**:sled vs leveldb vs plain toml;按决策改 `cache.rs`。
23. **watcher.rs**:`notify` + `notify-debouncer-mini`(100ms debounce),channel 推 invalidation event 到 `SymbolCache`。
    - **进程模型**:`RepoMap` 持有后台 `tokio::spawn` 任务(用 `tokio::select!` 退出),user 调 `shutdown()` 优雅停。
    - 单测:改动 fixture 一个文件,等 200ms,断言缓存中 mtime 已更新。
24. **用户配置**:`.shannon.toml` `[repo_map]` 增加 `languages` 列表(默认 `[rust]`),用户可写 `languages = ["rust", "typescript", "python"]`。
25. **集成测试**:`tests/repomap_multilang.rs` —— 4 个 fixture 同时存在,断言 4 种语言都进了 budget slice。
26. **insta snapshot**:`tests/snapshots/repomap_inject_*.snap` —— 锁住 inject 输出,防重构改变 prompt 格式后 LLM 行为漂移。
    - CI 加 `cargo insta test --check`(同 `P2-3 insta` 计划,但本任务先局部铺)。

#### Week 3 · 调优 + 文档 + Wave 3 衔接

27. **token 预算调优**:
    - 用 shannon-mono 自身 + 3 个开源 Rust 仓库做基线测,记录"PR top-30 / top-50 / top-100"各自的 token 数 vs 信息密度,选默认 `max_tokens=3000` 拍板。
    - 输出 `docs/perf/repo-map-tokens.md`(单页 200 字,数据驱动)。
28. **大仓库首扫体验**:
    - `RepoMap::for_workspace` 若首扫未完成,先返回 `BudgetSlice { text: "[repo map warming up...]" }`,后台 `tokio::spawn` 继续扫,扫完替换。
    - 防"用户敲了第一行 prompt 时空白等 5 秒"问题。
29. **Wave 3 衔接(本计划交付)**:
    - 暴露 `RepoMap::stats()` 供 P2-1(Compact 多策略)用:摘要时可"基于 repo map 选重要符号进入压缩后 context"。
    - 暴露 `RepoMap::slice_for(ident)` 供 P1-5(auto-test loop)用:LLM 报告失败位置时,只把相关符号推上去,不全量 re-token。
    - **本计划不实现** P2-1 / P1-5 的钩子,只留 API;两个计划各自 P0 时直接调。
30. **文档**:
    - `crates/shannon-repomap/README.md`:设计 + 公开 API 示例。
    - `docs/integrations/repo-map.md`:用户面,讲 `[repo_map]` 配置 + 关闭开关 + 调试(`SHANNON_REPOMAP_LOG=debug` 打印 parse 耗时)。
    - `CHANGELOG.md` 1 行条目:`feat(repomap): tree-sitter repo map, Rust-first, opt-in via [repo_map]`。

---

## 5. 验收

### 5.1 Phase A 出口

- [ ] `crates/shannon-repomap/` 编译通过,workspace 成员
- [ ] Rust fixture(6 文件)解析出 ≥ 30 symbols,签名正确
- [ ] token 预算:`max_tokens=2000` 时输出 `used ≤ 2000` 且 `dropped ≥ 1`(说明算法真在裁剪)
- [ ] 增量比全量快 > 5x(同输入 10 次对比)
- [ ] `QueryEngine::with_repo_map(...)` 注入后,mock provider 收到 system prompt 含 `## Repository Map` 节
- [ ] 单元测试 ≥ 20 条全绿;`cargo clippy --workspace -- -D warnings` clean
- [ ] `just dev` 在 shannon-mono 根绿
- [ ] REPL 手测:问"where is the `RepoMap` configured?" 时,LLM 回答引用 `crates/shannon-repomap/src/lib.rs`(说明 prompt 注入了项目符号)

### 5.2 Phase B 出口

- [ ] TS / Python / Go fixture 各自解析正确(per-language 单测绿)
- [ ] `.shannon.toml` `[repo_map] languages = ["rust", "typescript", ...]` 配置生效
- [ ] notify watcher 改动文件后 200ms 内 cache invalidation
- [ ] insta snapshot `repo_map_inject_*.snap` 4 张(每语 1)锁住输出
- [ ] `docs/perf/repo-map-tokens.md` 有 ≥ 3 个仓库的 token 数据
- [ ] 文档齐:`crates/shannon-repomap/README.md` + `docs/integrations/repo-map.md` + CHANGELOG 一行
- [ ] 工作区测试门 100% 绿,无新 clippy warning
- [ ] P2-1 / P1-5 钩子(`stats()` / `slice_for(ident)`)已实现,签名稳定

### 5.3 全局门槛(同 v2 计划总规约)

- [ ] `just dev`(check + clippy + test,见 [`CLAUDE.md`](../../CLAUDE.md))clean
- [ ] `cargo nextest run --workspace --exclude shannon-desktop` 全绿
- [ ] 触及用户可见文案的(en/zh 提示、日志信息)en/zh 同步
- [ ] 任何新增 `pub` API 有 docstring + 至少 1 个单测
- [ ] 大型依赖(tree-sitter 三方 grammar)体积记录在 `docs/perf/repo-map-deps.md`,CI 缓存命中 < 30s

---

## 6. 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 多语言 grammar 维护成本(tree-sitter 三方 grammar 偶发 breaking change) | 中 | 中 | Phase A 只 Rust,跑通后增量加其它;锁版本到 patch 级,跟 tree-sitter ~0.25 主版本 |
| token 预算调优难(信息密度 vs 噪声;PR top-30 选错符号) | 高 | 中 | Aider pack 算法经验值 + 调优周(Phase B 第 3 周)+ 用户反馈循环(开 issue 收集);不达预期时退回简单"按 name 字典序 top-N" |
| 大仓库首扫慢(>10k 文件) | 中 | 低 | 后台异步扫 + "warming up" 占位提示 + 可由 `[repo_map] max_files` 限流 |
| 注入位置不对导致 LLM 误用(把 repo map 当 source of truth 引用过时内容) | 低 | 高 | 明确 prompt section 标题 `## Repository Map (auto-generated, may be stale)` + 时间戳 + 单元测试断言 section 在 system prompt 第 1 段后 |
| tree-sitter 解析时间消耗导致 query 延迟增加 | 中 | 中 | 预热(watcher 后台常驻)+ 缓存命中同步返;若仍超 100ms 给出 `SHANNON_REPOMAP_ASYNC=1` 强制走后台 |
| `notify` 跨平台差异(Linux inotify / macOS FSEvents / Windows ReadDirectoryChangesW) | 低 | 低 | `notify-debouncer-mini` 已抽象;CI 三平台跑 watcher 单测 |
| sled/leveldb 决策失误导致引入大依赖 | 低 | 低 | Phase A 先用 in-memory,Phase B 决策点推迟到收尾;最差退到 plain `bincode` 文件 |
| 与 P1-5 auto-test loop 并行开发时 API 错位 | 低 | 中 | 本计划在 §4 Week 3 暴露 `slice_for(ident)` 钩子,P1-5 直接调;若 P1-5 提前开工,需提前 merge 该 API |

---

## 7. 决策点(需要你审)

> 这些点会在执行过程中按顺序回到你这里确认,不必现在全定。

1. **缓存后端**(Phase B Week 2,Day 1)
   - **选项 A**:`sled`(纯 Rust,无 C 依赖,易 embed,体积 ~2MB)
   - **选项 B**:`leveldb`(`rusty-leveldb` 纯 Rust,或 `leveldb-sys` C 绑,体积更小但 C 绑)
   - **选项 C**:plain `bincode` 文件(零依赖,自己写 LRU-on-disk)
   - **倾向**:C 起步,Phase B 末若磁盘 LRU 命中率 < 80% 再升级;理由:首版不要被存储层绑架
   - **决策**:**你定**(影响 Cargo.toml 依赖,小幅)

2. **多语言优先级**
   - 建议序:Rust(主)→ TypeScript → Python → Go
   - **待确认**:你日常在哪些语言里更痛?Rust/TS/Python/Go 之外,要先加 Java / C++ / Ruby 吗?

3. **与 P1-5 auto-test loop 的边界**
   - repo map 是**只读**注入;auto-test 是**写**循环;两者解耦,无 API 冲突
   - **本计划**在 §4 Week 3 暴露 `slice_for(ident)` 给 P1-5 用,P1-5 端调它就能只把相关符号推上去
   - **待确认**:你希望 repo map 的预算/语言配置与 auto-test 的 `[auto_test]` 段合并(单文件 `[context]` 段),还是保持独立?目前计划是**独立**(单一职责,YAGNI)
   - **决策**:**你定**(影响 config schema,微)

4. **是否默认开启**(本计划默认 Phase A 仅 Rust 时开,Phase B 多语言后开?)
   - 倾向:**Phase A 默认开**(Rust 用户占 Shannon 自用 100%,几乎零成本);**Phase B 多语言后默认开**(覆盖率提升,值得默认)
   - **决策**:**你定**(影响开箱即用体验)

5. **大仓库(>10k 文件)的"不打扰用户"策略**
   - 倾向:首扫时返回空 map + 后台 warmup(§4 Day 28)
   - 备选:首扫阻塞 5–10 秒,期间显示进度
   - **决策**:**你定**(影响 UX)

---

## 8. 参考

- **Aider repo map**(`aider/repo.py`):PageRank over import graph + `pack_tokens` greedy slicer + `identify` for 1000+ lang detection。本计划直接复刻思想而非代码(license 兼容)。
- **tree-sitter docs**:`https://tree-sitter.github.io/tree-sitter/`(query syntax, grammar 注册)
- **nvim-treesitter queries**:`https://github.com/nvim-treesitter/nvim-treesitter/tree/main/queries` —— Phase B 多语言 `.scm` 模板来源
- **shannon-core 现有预算参考**:`crates/shannon-core/src/compact.rs` —— Compact 已用 `chars() / 4` 估 token,本计划的 `budget.rs` 与其保持口径一致
- **shannon-core ContextInjector**:`crates/shannon-core/src/query_engine/context_injector.rs` —— 本计划新增 `RepoMapInjector` 实现同 trait,挂在 `QueryEngine::new()` 初始化链
- **Aider pack 算法的中文综述**:`docs/competitor-feature-matrix.md` §2.4(各竞品对比)
- **本计划 ID 与 PRD 锚点**:`docs/improvement-plan-2026-08.md` §2 P1-4(v2 增量为本文件;v1 无文件锚点,v2 增补)

---

## 9. 关键提交契约(预计里程碑)

| 提交 | 内容 | 验证 |
|---|---|---|
| `chore(repomap): scaffold shannon-repomap crate` | Day 1 末:crate + 空 lib + workspace member | `cargo check -p shannon-repomap` |
| `feat(repomap): rust parser + symbol tree` | Day 2 末:parser + symbol_tree + fixture + 单测 | `cargo test -p shannon-repomap` ≥ 6 |
| `feat(repomap): pagerank + token budget` | Day 3 末:PR + greedy slice | `cargo test -p shannon-repomap` ≥ 12 |
| `feat(repomap): mtime cache + 5x speedup` | Day 4 末:cache + bench | bench 断言 5x |
| `feat(core): query_engine injects repo map` | Day 5 末:engine 钩 + 集成测 | `cargo nextest run -p shannon-core --test repomap_inject` |
| `docs(plans): sign off Phase A` | 本计划 Phase A 签收 | `just dev` clean |
| `feat(repomap): typescript/python/go support` | Week 2:三语 + 配置 | per-language 单测全绿 |
| `feat(repomap): notify watcher + cache backend` | Week 2 末:watcher + 决策点 1 落地 | watcher 单测 |
| `feat(repomap): insta snapshots + perf doc` | Week 3:4 张 snapshot + perf 数据 | `cargo insta test --check` clean |
| `docs(plans): sign off Phase B` | 本计划签收 + CHANGELOG + 文档 | `just dev` + 手测 |

---

*Plan complete. Awaiting ericdong sign-off on §7 决策点(尤其缓存后端 / 默认开启 / 与 P1-5 边界)before starting Phase A Day 1.*
