# Provider/Model R2 复审方案 —— 对抗性审查(红队)

- 日期:2026-10-05
- 审查对象:[2026-10-05-provider-model-config-review-r2.md](2026-10-05-provider-model-config-review-r2.md)(v1.0 初稿,当日评审产出)
- 审查方法:以 dev @ `d9cc1428b` 代码 + 竞品官方文档为事实基准,对 R2 文档的高危论断与方案项逐条证伪;红队目的 = 在用户批准前消灭方案的事实错误、严重度通胀、规模低估与遗漏项。全部新证据由主会话直接回仓核实(非转述探查报告)。
- 处置:事实性错误已直接修订进主文档(v1.1);需要用户拍板的增补决策点见 §3。

---

## 0. TL;DR

| # | 发现 | 级别 | 处置 |
|---|---|---|---|
| 1 | **P0 标签通胀**:§3 五项"P0 信任损伤"经攻击后**无一项满足 journey R2 的 P0 定义**(说谎 UI/UI 承诺不成立)——全仓 provider 域已无说谎 UI,这本身是 R1-R5 修复成效的证明。建议整体降级 P1(高/中),**S1 批次范围与排期不变** | 高 | 已修订主文档 |
| 2 | **P-N12① InlineLegacy 暴露面为空**:生产代码**零构造点**(`unified_config.rs:997` 为测试 fixture `deepseek_profile()`;全仓仅 build.rs schema 重声明/export match 臂/定义/读取)。A1 决策下 providers.toml 从不存明文,该风险仅剩手编 TOML 理论面。真实缺口只剩 extra_headers 一项 | 高 | 已修订主文档 |
| 3 | **P-N3 "注释说谎"指控不成立**:`honest metadata` 注释(`commands_chat.rs:119`)描述的是** tier 徽章**(活的、行为属实),只是物理位置在 `dynamic: None` 上方两行;不构成"注释掩饰 no-op"。死徽章本身属实,修辞框架撤回 | 高 | 已修订主文档 |
| 4 | **S2-1 TOML 污染风险**:把 fetch 固化模型写进 providers.toml SSOT——OpenRouter 一次 fetch 数百条目,与"config as code"手编用户直接冲突,且撑爆导出文件。需显式多选+软上限+分区策略,picker 合并优先级未定义 | 高 | 修订 S2-1,新增决策点⑥ |
| 5 | **S3-3 规模低估**:`AuxRole`/`auxiliary` 在 schema 存在但**全仓零消费者**(所有引用均为 `HashMap::new()` 构造,provider_config_service.rs:1860-2302 等全是测试/构造点)。"复用既有槽位"实为新建消费链(L 尺寸);且若把 utility 槽塞进 R5-5 优先级链会引入第四维度,应正交化 | 高 | 修订 S3-3,新增决策点⑦ |
| 6 | §2 P2 表**漏 P2-19 行**(effort 折叠),统计"3 FIXED/2 PARTIAL/1 未修"与表格 5 行对不上 | 中 | 已补行 |
| 7 | TL;DR"P0×6 全部真实闭环"对 **P0-1 过度声明**:回落声明落在日志级(`unified_config.rs:640`),未达 09-30 路线图"对话内显式声明"的验收字面 | 中 | 已修订措辞 |
| 8 | **S1-1 非 S 尺寸**:结构化错误 kind 需改引擎 query:failed 载荷(engine+desktop+i18n+e2e 四层);且"复用 TestConnectionResult 单一实现"必须落在引擎侧(探活分类源于 HTTP status,查询路径桌面看不到 status) | 中 | 已修订 S1-1 |
| 9 | **S1-3 首选方案需 wire 变更**:`merge_static_and_dynamic`(`model_registry.rs:42`)无来源标记,接真数据=改 ModelInfo wire;S 尺寸的选项是"删徽章"。默认应翻转 | 中 | 已翻转默认 |
| 10 | **S1-4 探测热路径**:`detect_env_provider` 被 `get_provider_status` 共享(门控 ApiKeyBanner/Welcome),加 TCP 探测=门控路径引入网络 I/O;需 TTL 缓存或仅 Welcome 命令探测 | 中 | 已加护栏 |
| 11 | **S2-4 规模低估**:capability 新位(catalog.rs 位标志是文件局部 const)+wire 字段+工具门控+"切换建议"引擎,合计 M-L;应拆 a(vision UX 修复,S)/b(tool 位+门控,M-L) | 中 | 已拆分 |
| 12 | **S2-6 Azure 前置缺验证**:wire 支持确实存在(`types.rs:62,186` `/openai/deployments/` 路径——S2-6 不是陷阱),但 api-version 查询参数/deployment 名=模型 id 的语义未验证;且 `is_probeable_kind` 排除 Azure→表单内 Test 会重演 gemini 死胡同 | 中 | 已加前置 |
| 13 | **S3-6 DoD 不可测**:"估算与 Usage 页实结误差钉测"无 ground truth 定义;应改为"与计费同源计数实现"的一致性钉测 | 中 | 已修订 |
| 14 | **§3 漏项**:走查报告已发现的「gemini 连接 Test/Fetch 恒 not supported、表单无前置提示」未收录进问题清单 | 中 | 已补 P-N25 |
| 15 | P-N11「6 个 UI 面」计数虚高:chip/Header 按路由分区(同一概念两个路由)、Provider Activate 是 provider 级非模型级;诚实口径 = 4 个模型级面+2 个相邻操作 | 低 | 已修订口径 |
| 16 | P-N1「没有任何指向」过强:provider 原始报文常自含原因(DeepSeek "Insufficient Balance" 等);准确缺口 = 无分类/无深链/无建议动作 | 低 | 已修订措辞 |
| 17 | 竞品表证据分级不严:❌ 多处实为"官方文档未提及"(Cursor 自定义端点等);Codex 会话级切换为二手证据。对比表 ❌ 应读作"未发现" | 低 | 图例已注明 |
| 18 | backlog 缺项:per-provider 网络鲁棒性参数(Codex 式 request_max_retries/stream_idle_timeout;Shannon `quirks` 字段已有位可扩展)未列入远期 | 低 | 已补入明确不做/远期 |

**结论**:R2 文档的核心事实底座(suppress_failover、dynamic None、OLLAMA_HOST 矛盾、quick-fill 过时 id、P1-12 裁决、failover/轮换机制描述、竞品格局判断)**经攻击存活**;需修正的是严重度标定、两处修辞框架、一处安全风险范围,以及 S 系列中 6 个项的规模/前置/DoD。主文档已修订为 v1.1,S1-S4 批次结构与排期不变。

---

## 1. 逐条详证

### 1.1 [发现#1] P0 标签通胀 —— 严重度体系被本次文档自己破坏

**攻击**:journey R2 对 P0 的定义是「说谎 UI」——UI 报成功/承诺能力而实际不成立。逐项检验 §3 原 P0 五项:
- P-N1(402/429 无分类):用户看到的是 **provider 原始报文**(如 "Insufficient Balance"),不假、不谎,只是不帮忙 → P1。
- P-N2(pin 静默关 failover):行为与代码注释一致、自洽,缺的是呈现 → P1。
- P-N3(dynamic 死徽章):徽章从不点亮=从不承诺;发现#3 撤回"注释说谎"后,只剩死 UI → P2~P1。
- P-N4(Ollama 不探测):Welcome 仍可手填,是摩擦不是死路 → P1 高(伤害面大)。
- P-N5(quick-fill 过时):指向的 id 大概率仍可用(厂商极少删模型),是"过时"不是"虚假" → P1 中。

**反方辩护**(记录在案):#154 把「401 深链」当 P0 级修了,#154 把「同族 402 不深链」留白——按仓库自身的先例,402 可以主张 P0 一致性。但 09-29 评审当时把 401 归 P0 的理由是「错误**指路错误**」(文案指向不存在的出口=主动误导),402 现状是「无指路」(被动缺失),两者在"说谎"定义上确实不同档。

**裁决**:五项全部降 P1(标注高/中),§3 小节改题「P1 高——快速信任/卫生修复」。**S1 批次不变**——快速 PR 的排期理由(小、独立、高感知)不依赖 P0 标签。

### 1.2 [发现#2] InlineLegacy —— 一个被夸大的安全风险

**攻击**:P-N12① 称「`InlineLegacy{masked}` 实携真键,迁移未完成的 profile 被导出时明文出门」。回仓核实:
- 全仓构造点仅 `unified_config.rs:997` —— 上下文是 `/// A deepseek-style profile ... so the tests never touch process env`,**测试 fixture**。
- 其余引用:类型定义(provider_config.rs:77)、build.rs schema 重声明(:508)、导出 match 臂(commands_providers.rs:1450)、resolver 读取(provider_resolver.rs:254,276)。
- 生产写入路径不存在:A1 决策下 `/connect`、桌面保存、CLI、迁移向导均写 `CredentialRef::Env/Keyring/Store`,从不写 InlineLegacy。

**结论**:暴露面 = 用户手编 TOML(把 `masked = "sk-real-key"` 当字段名提示误导而填真键)——理论面。撤销 P0/P1 级安全警告,降为 S4 加固项(导出时遇 InlineLegacy 拒绝并提示迁移),extra_headers 缺口(P-N12②)维持不变且升为该项主体。

### 1.3 [发现#3] "honest metadata 注释说谎" —— 修辞错误

**攻击**:P-N3 称「上方注释还宣称 honest metadata——注释掩饰 no-op 反模式再现」。核实 `commands_chat.rs:115-135`:注释 `(honest metadata)` 位于 `:119`,语义是「tier 解析不出就**不渲染**徽章而非猜一个」——描述 tier 徽章(活的、且行为与其宣称一致);`dynamic: None` 在 `:130`,两者间隔 10 行,注释并未声称 dynamic 有数据。

**结论**:死徽章(P-N3 的核心事实)维持;「注释说谎/掩饰 no-op」框架撤回。准确表述:「tier 修了,dynamic 半边没修,且没有任何注释承认 dynamic 是 no-op——读者从相邻的 honest-metadata 注释获得相反印象」。这是呈现问题,不是诚信问题;降 P2。

### 1.4 [发现#4] S2-1 把数百条 fetch 结果写进手编 TOML

**攻击**:S2-1 让 fetch models 多选后固化进 `providers.toml` v2 `models: Vec<ModelSpec>`。三个未回答的问题:
1. **容量**:OpenRouter `/models` 返回 400+(调研 §1.9),全选写入 = 数百条 TOML 条目;文档自己引用了 Open WebUI「数千模型拖慢 10-15s」的教训,却把同样的量级引进 SSOT 文件与导出文件。
2. **手编冲突**:providers.toml 的既有用户群是"config as code"人群(R4-2 导出/导入就是为他们做的);UI 批量写入会让手工 diff/审查变成灾难。
3. **合并优先级未定义**:picker 目录 = 静态 catalog + models.dev overlay + (新增) profile.models 三源合并,谁过滤谁、徽章如何取值,方案未写。

**修订**:S2-1 增加约束——①显式逐项多选,默认零选中,「全选」需二次确认;②软上限(建议 50,超限警告);③写入段加「UI 管理」注释头;④优先级钉:**profile.models 为过滤白名单(选中才进 picker),catalog/overlay 为元数据供给**——一条钉测。新增决策点⑥(上限数值与是否允许全选)。

### 1.5 [发现#5] S3-3 "复用 AuxRole" —— schema 存在 ≠ 管线存在

**攻击**:S3-3 称「复用 R3-3 的 phase tier 配置结构与 auxiliary 槽位概念(引擎 `AuxRole` 已在 schema)」。核实:`AuxRole` 枚举与 `auxiliary: HashMap<AuxRole, ActiveTarget>` 仅存在于类型定义(provider_config.rs:54,294);全仓消费点 grep 只命中**构造空 HashMap** 的代码(provider_config_service.rs:1860,1905,1980,2004,2248,2302 等全为测试/序列化构造),**零读取、零写入 UI、零解析语义**。

**结论**:S3-3 是从零新建消费链(解析→查询时按角色解析目标→未配置回退),尺寸从"复用" implied 的 S/M 修正为 **L**;且必须裁决 utility 槽与 R5-5 优先级链的关系——建议**正交化**(utility 槽只服务后台任务通道,不进 session>phase>global 的交互链,不触碰 unattended 钉测表),否则一次改动同时动最敏感的优先级契约。新增决策点⑦。

### 1.6 [发现#8/9/10] S1 批次的三个规模/护栏修正

- **S1-1**:错误 kind 直通需要改引擎 `query:failed` 载荷(events.rs 现在拿的是 Display 字符串)——engine+desktop+i18n+e2e 四层,非 S 尺寸;「与 TestConnectionResult 单一实现」的落点必须在引擎(探活分类的 HTTP status 桌面侧不存在)。修订:表述改为「引擎为分类单源,探活与查询两路消费」,S1 批次仍可容纳但标注 M。
- **S1-3**:`merge_static_and_dynamic`(model_registry.rs:42-65)合并后不保留来源标记,「首选接真数据」实为 ModelInfo wire 变更(=S2 尺寸)。翻转默认:**删徽章(含 i18n 键清理)为 S1 默认**,「接真数据」作为 S2-1 来源徽章体系的一部分(反正 S2-1 要做来源徽章,一次做对)。
- **S1-4**:`detect_env_provider` 的 doc 注释言明它同时服务 `detect_provider_from_env`(Welcome)与 `get_provider_status`(ApiKeyBanner/Layout 门控)(commands_config.rs:1059-1062,1105-1108)。给它加 TCP 探测 = 门控热路径引入网络 I/O。护栏:结果带 TTL 缓存(建议 30s)或仅在 Welcome 命令路径探测、`get_provider_status` 读缓存。

### 1.7 [发现#12] S2-6 Azure —— 不是陷阱,但有两个前置

核实 Azure wire 支持真实存在(types.rs:62 枚举、:128-129 URL 反推、:186 `/openai/deployments/` 端点路径、:219 默认 host)。S2-6 可做,但:①api-version 查询参数与「deployment 名=模型 id」语义是否有测试覆盖未验证——catalog 条目上线前先补;②`is_probeable_kind` 排除 Azure(commands_config.rs:1306-1313)→ 表单内 Test 会给 Azure 用户 Unknown verdict,**重演 gemini 死胡同**(见发现#14)。前置:is_probeable 对 azure/gemini 的排除必须在表单内有前置提示,或把两家纳入可探测集。

### 1.8 [发现#13] S3-6 的 DoD 在物理上不可写

「估算与 Usage 页实结误差钉测」:估算发生在发送前(输入 token 未知、输出未知),Usage 实结在完成后——两者不可能相等,钉测要么 flaky 要么空洞。修订 DoD:**估算与计费共用同一 token 计数实现**(同一 tokenizer/同一计数函数),钉测断言一致性(同输入→同数字),误差问题定义性消解;输出侧按 max_tokens 上限区间展示。

### 1.9 [发现#6/7/14/15/16/17/18] 文档卫生簇

- §2 补 P2-19 行(effort 折叠,部分修——header/composer 路由分区解决了双 picker 分裂,effort 仍嵌模型下拉)。
- TL;DR 措辞:P0-1「回落声明降为日志级(`unified_config.rs:640`),弱于路线图验收字面(对话内显式声明)」。
- 补 P-N25(gemini/Azure 探测死胡同无前置提示——走查报告原有,初稿漏收;并发现#12 合并处理)。
- P-N11 口径修正:4 个模型级切换面(chip、Header、Settings 快速切换器+目录行、profile)+2 个相邻操作(Set-as-default、Provider Activate)。
- P-N1 措辞修正(见 1.1)。
- §4.3 图例注明 ❌=「官方文档未发现」,非「确认不存在」;Codex 会话级切换为二手(🟡)。
- 「明确不做/远期」补:per-provider 网络鲁棒性参数(`quirks` 已有字段位,等真实用户诉求再暴露)。

---

## 2. 复议后的严重度重贴

| 原标签 | 项 | 新标签 | 理由 |
|---|---|---|---|
| P0 | P-N1 聊天 402/429 无分类 | **P1 高** | 信息非零(原始报文),缺的是分类/深链/建议;与 09-29 P0-3「指路错误」不同档(无指路≠错指路) |
| P0 | P-N2 pin 静默关 failover | **P1 高** | 语义自洽、注释在案;缺呈现 |
| P0 | P-N3 dynamic 死徽章 | **P2** | 从不点亮=从不承诺;撤销说谎框架后是纯打磨 |
| P0 | P-N4 Ollama 不探测 | **P1 高** | 手填可绕过,摩擦非死路;但伤害典型画像,维持 S1 快修 |
| P0 | P-N5 quick-fill 过时 | **P1 中** | id 大概率仍可用;首屏观感问题 |
| P1 | P-N6..P-N18 | 维持 | 经攻击存活 |
| — | P-N25 gemini/Azure 探测死胡同 | **P2(新)** | 初稿漏收,红队补回 |
| — | P-N12① InlineLegacy | **P2 加固(范围收窄)** | 生产零构造点;extra_headers 部分维持 P1 |

**批次影响**:无。S1 内容不变(五项全保留,只是不叫 P0);S2-1/S2-4/S2-6/S3-3/S3-6 按上文修订;S1-3 默认翻转为删徽章、接真数据并入 S2-1 来源徽章体系。

## 3. 增补决策点(在原①-⑤之上)

> **裁定(2026-10-05)**:⑥-⑨ 已全部按建议拍板;①-⑤ 同日按建议拍板。全量裁定记录(含理由)见主文档 §6「决策点裁定记录」。

| # | 决策点 | 建议 |
|---|---|---|
| ⑥ | S2-1 模型仓容量策略 | 软上限 50 + 「全选」二次确认 + UI 管理段注释头 |
| ⑦ | S3-3 utility 槽与优先级链关系 | 正交化(只走后台任务通道,不进 R5-5 交互优先级链,不动 unattended 钉测) |
| ⑧ | S2-4b(tool 位+门控)放 S2 还是 S3 | 建议 S2 尾部或 S3 头(依赖 S2-1 的来源徽章;vision 部分即 S2-4a 不依赖) |
| ⑨ | 严重度重贴是否接受 | 建议接受(本文 §2);不影响任何批次范围与排期 |

## 4. 攻击存活清单(经红队验证仍成立的关键论断)

- 会话 pin 置 `suppress_failover=true` 且零 UI 提示(`commands_chat.rs:495-508` 直读)✓
- `dynamic: None` 硬编码、徽章恒不亮(`commands_chat.rs:130` 直读)✓(修辞修正,事实保留)
- `detect_env_provider` 只查 `OLLAMA_HOST`、与 doc 注释矛盾(`commands_config.rs:1055-1094` 直读)✓
- quick-fill 五个 id 过时(`add-provider-modal/types.ts:26-36` 直读)✓
- 表单内 Test 存在、save≠test(P1-12 已修;两报告冲突以代码裁决)✓
- 导出 redact walk 不覆盖 `extra_headers`(`commands_providers.rs:1222-1252`)✓(P-N12 主体)
- Azure wire 路径存在(`types.rs:186`)✓ —— S2-6 非陷阱(红队反向证明)
- 401 深链/轮换通知 i18n 残留/Provider Tabs 单 tab/Profile 无覆盖提示(直读)✓
- 竞品格局判断:Shannon 引擎面领先、桌面结构达标、四项剩余差距、tier→utility 槽位机会 ✓(证据分级修正后维持)
