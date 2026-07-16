# 新手用户视角审查报告

> 本报告由一位完全没有 AI 使用经验的普通用户视角撰写。我刻意不查文档、不学概念，只凭直觉去"摸"这个产品。每一条困惑和不满都是真实的,没有为了客气而美化。

---

## 我的画像

**姓名**:王秀梅(Sue)
**年龄**:52 岁
**职业**:高中语文老师,兼职帮丈夫的自由职业生意做点文字整理(写朋友圈文案、整理客户名单、排版合同)
**技术背景**:
- 会用微信、淘宝、Word、Excel(只会基本功能)、PPT
- 用过百度翻译、讯飞输入法,从来没碰过 ChatGPT、Kimi、文心一言这类 AI 工具
- 不知道什么叫"prompt",不知道什么是"大模型",更不知道"API key"是什么
- 手机和电脑都设了大字体,英文只认识常见单词
**这次想用 Shannon 做什么**:
1. 帮我润色一下家长群发言,别让语气显得太生硬
2. 把一份杂乱的客户名单 Excel 整理成分类清晰的版本
3. 帮我起草一份校运会开幕词
4. 看看能不能自动整理一下我电脑里乱七八糟的文件夹(朋友说 AI 能做)

**我怎么听说这个软件的**:朋友的儿子(程序员)推荐的,说"现在最好用的 AI 工具"。

---

## 第一印象(打开应用 30 秒)

### 看到的第一个界面

打开应用后是一个全屏的引导页(后来知道叫 Welcome),上面写着:

> **Choose your AI provider**
> You can change this any time in Settings → Models.

下面四个卡片选项:**Anthropic**、**OpenAI**、**Ollama**、**DeepSeek**。

每个卡片下面有一行英文小字解释,比如:
- "Claude Sonnet 4.6 — recommended for coding"
- "GPT-4o / o1 — strong reasoning, multi-modal"
- "Local models, no API key needed"
- "Cost-effective, long-context tasks"

### 我能理解这是什么产品吗?

**完全不能。**

我愣在原地至少 30 秒,脑子里冒出 5 个问题:
1. "provider"是什么?中文应该叫"提供商"吗?为什么要让我选?我以为是来用 AI 的,怎么变成选厂商了?
2. Anthropic / OpenAI / Ollama / DeepSeek,这四个名字我只听过 OpenAI(因为新闻里见过)。其他三个完全陌生。Claude Sonnet 4.6、GPT-4o 这些版本号我更看不懂——这跟选手机套餐似的,但价格呢?
3. "recommended for coding"——**coding 是写代码?我不是程序员啊,这软件不适合我?** 这是我心里第一个"想关掉"的念头。
4. "API key"那个输入框下面写着 "Stored locally via Shannon's config. Get a key from your provider's dashboard."——我连 dashboard 是什么都不知道,更别提去"provider 的 dashboard"拿 key。我应该怎么付钱?一个月多少钱?能微信支付吗?**全部没有**。
5. 唯一不需要 API key 的是 Ollama,描述是"Local models, no API key needed"——但"Local models"是什么意思?要不要钱?要不要装别的东西?没有任何解释。

### 我下一步会做什么

我有两个选择:硬着头皮填 OpenAI 的 API key(但我没有,也不知道怎么搞),或者点右上角的"Skip →"。

**我点了 Skip。**

跳过之后是主界面。左侧多了一列菜单,顶部有个标题栏,但我一眼看过去就**懵了**——下面详说。

> 第一印象总结:这个软件第一句话就在劝退我。它假设我已经知道什么是 LLM provider、已经知道怎么搞 API key、已经知道为什么要在四个英文公司之间选一个。**它不是给"我"用的。**

---

## 逐页试用记录

### Chat

**看到什么**:左侧一列会话历史(空的),中间是空白的对话区,上面有一个大标题"What can I help with?"和四个例子卡片。右下角一个输入框,placeholder 是"Ask Shannon anything..."。右侧(电脑宽屏才有)是一块"Context panel",显示 Usage、Active Tools、Context Files。

**理解成什么**:这个我能看懂,跟朋友给我看过的 ChatGPT 截图差不多。中间那个"What can I help with?"是友好的。

**疑惑点**:
- 四个例子卡片全是写代码的:**Write code**("Help me build a REST API endpoint in Rust")、**Debug an issue**("Debug why my React component is re-rendering infinitely")、**Explain a concept**("Explain how async/await works in Rust")、**Refactor**("Refactor this function to be more idiomatic")。**没有一个是给我这种普通用户的**。我想问"帮我润色一段话",连个例子都没有,只能自己琢磨怎么问。
- 右侧"Context panel"里写着"Input tokens / Output tokens / Cost / Context Window"——tokens 是什么?Context Window 是什么?为什么一开始就给我看"Context Window 0%"这种进度条?**这些信息对我毫无意义,反而让我紧张**(我是不是用得太多了?要不要钱?)。
- 底部状态栏(Layer out 里的 footer)显示 "0 tokens · $0.0000"——又是 tokens 和美元。我不能用人民币看吗?
- 输入框左边有个回形针图标(attach file),我试着点了一下,弹出了系统文件选择框——可以选文件,但**选完之后没有任何提示告诉我"已添加"**,后来才发现文件名出现在了输入框上方一个小条里,字体很小。
- 输入框右边那个紫色发送按钮是向上箭头(↑)。**不是回车符号也不是纸飞机**,我第一次以为是"回到顶部"按钮。我盯着它看了 5 秒才敢点。

**情绪反应**:这是整个产品里**唯一**让我觉得"也许能用"的页面。但它还是处处暗示"我是给程序员用的代码助手"——例子的语气、tokens 这种术语、美元计价。

---

### Goals

**看到什么**:左侧一个"Active Tasks / Completed"列表(空的),中间是"Task Management"标题加一条空的"task 树",右边一个"Task Summary"卡片。底部一个输入框,placeholder 是"Ask about this goal..."。还有个紫色 ✨ 按钮弹出三个选项:Suggest Next Steps、Summarize Progress、Identify Risks。

**理解成什么**:完全不理解。**"Goal"是什么?跟 Chat 有什么区别?** 我以为 Goal 是"目标",那我应该来这里"设定一个目标"让 AI 帮我完成?但页面上没有"新建目标"的按钮,只有一个"Ask about this goal..."的输入框——可我现在还没有任何 goal 啊,问什么?

**疑惑点**:
- 标题写着"Task Management"——那为什么侧边栏菜单叫 Goals?**名字和内容对不上**。
- 左侧那个搜索框 placeholder 是"Search tasks..."——又是 task 不是 goal。
- 中间空状态写着"No tasks yet. Create tasks via the Tasks page or background execution."——它让我去 Tasks 页面创建,那我为什么要先来 Goals?Goals 到底是干嘛的?
- 右侧 Task Summary 显示"Active 0 / Pending 0 / Completed 0"——一进来全是 0,没有任何引导告诉我"接下来该做什么"。
- 底部那个 ✨ 按钮的三个选项:Identify Risks(识别风险?)、Summarize Progress(总结进度?)——我连一个 task 都没有,它能给我识别什么风险?

**情绪反应**:**这是第一个让我彻底懵掉的页面。** 我不知道它的存在意义是什么。如果 Chat 已经能问问题,如果 Tasks 已经能建任务,那 Goals 这一层是夹在中间做什么的?**我关掉了这个 tab,以后再也没回来过。**

---

### Scheduled (Tasks)

**看到什么**:顶部一排按钮:Filters、Calendar、DAG、New Task、Schedule。下面三个 tab:Active / History / Worktrees。中间一个空的任务列表,右边一长串面板:Calendar Sidebar Widget、Efficiency Card、Agent Allocation、Hook Task Pipeline、Schedule DAG View、Task Execution Log。

**理解成什么**:这是个**任务管理界面**。但我点进来是想"让 AI 帮我做点事",不是"管理一堆任务"——这两个完全是两件事。

**疑惑点**:
- "**DAG**"是什么?是缩写吗?全称是什么?点了一下,出来一个图形化的视图(空的)——还是看不懂。后来查了才知道是"有向无环图"(Directed Acyclic Graph)。**普通用户根本不需要知道这个**。
- "**Worktrees**"是什么?字面意思是"工作树",但跟任务管理有什么关系?点开看是一个文件路径列表——后来知道这是 Git 的概念。**为什么 Git 概念会出现在一个"AI 助手"里?**
- "**Hook Task Pipeline**"——Hook 是钩子?Pipeline 是管道?这是什么工业软件?
- "**Agent Allocation**"——Agent 是什么?代理人?为什么 AI 助手有"代理人"?
- "New Task"按钮点开是 NewTaskForm,让我输入"prompt"——又是这个英文词,我不知道该写什么。Placeholder 也是空的。
- "Schedule"按钮点开是 ScheduleForm,要填什么 trigger_type、webhook、cron 表达式——**cron 是什么?webhook 是什么?** 我朋友说 AI 能帮我自动整理文件夹,我以为点两下就行,结果让我填 cron 表达式。
- 底部状态栏显示我的模型名字"claude-3-5-sonnet-20241022"——这种版本号让我想起 Windows 注册表,完全是给程序员看的。

**情绪反应**:**这一页让我感觉这个产品根本不是 AI 助手,而是一个项目经理工具。** 我只是想问 AI 一个问题,为什么要面对一整个 Trello + 日历 + DAG + Git 工作树?**这是第二个让我想关掉的瞬间。**

---

### Mission Control

**看到什么**:顶部写着"Mission Control — Aggregated view across 0 tasks from all teams"。下面五列大看板:Queued、In Progress、Blocked、Completed、Failed,每列都是空的。

**理解成什么**:**Mission Control 是 NASA 用的词吗?太空指挥中心?** 这名字夸张到我笑出了声。

**疑惑点**:
- "Aggregated view across all teams"——我哪有什么 team?我就一个人用。
- 五列名字:Queued(排队)、In Progress(进行中)、Blocked(阻塞)、Completed(完成)、Failed(失败)——前四个我大概能猜,但 "Queued" 我得想一下才知道是"排队等候"。
- 右下角的卡片每个都显示 "smart_toy" 图标 + assignee + team + due_date——这些都是项目管理术语,**一个普通用户用不到**。
- 这页跟 Tasks 页、跟 Goals 页、跟下面的 OPC 页,**功能高度重叠**——都是某种"任务/项目看板"。我作为新手根本分不清应该在哪一页做什么。

**情绪反应**:这页是只读的"观察面板",但**我没东西可观察**。进来看到五个空列,我不知道下一步该做什么。这页的存在感几乎是负数——多一个名字就多一份混乱。

---

### Triage

**看到什么**:顶部"Triage — Items needing your attention"。下面一行筛选条:Kind(All Kinds / Failed Run / Budget Exceeded / Needs Review / Timeout)、Read(all/unread/read)、Sort、Hide Archived。中间一个列表(空的)。

**理解成什么**:Triage 这个词我**完全不认识**。后来查了字典才知道是"分诊、优先级排序"(医院急诊室用的词)。为什么要用一个医院术语?

**疑惑点**:
- 四种"kind":Failed Run、Budget Exceeded、Needs Review、Timeout——**全是程序员/运维词汇**。Budget Exceeded 是"预算超支"?谁的预算?Timeout 是"超时"?
- "Items needing your attention"——那这跟"通知中心"有什么区别?为什么不直接叫 Notifications?
- 整个页面没有任何"我能做什么"的提示,空状态写着"All clear."——好,既然没问题,我为什么要进来?

**情绪反应**:**Triage 这个名字本身就在告诉我"这个软件不是给你用的"。** 一个普通用户看到 Triage 三个字,第一反应是查字典,而不是开始用产品。

---

### Extensions (Skills / My Agents / Data Sources)

**看到什么**:三个 tab。

**Skills**:一排卡片,按"category"分组(Coding、Research 之类),每个 skill 有名字、描述、trigger。我看到了 /commit、/pdf、/help 这种名字。

**My Agents**:agent 卡片,显示 status、model、task、progress,还有一个"performance metrics"。

**Data Sources**:表单,让我填 server name、command、args,下面是一列已连接的 server,显示 connection status 和 tool count。

**理解成什么**:
- "Skills" 我猜是"技能",也就是 AI 会做的事情。但卡片上写的全是英文命令(/commit、/pdf)——这是给程序员用的快捷命令吧?
- "My Agents"——**我没有任何 agent**,而且我也不知道 agent 是什么。是 AI 的"分身"?它跟 Chat 里那个 AI 有什么区别?
- "Data Sources"——数据源?是要让我接入数据库吗?那个表单要填"command"和"args",这是 Linux 命令行啊。**MCP** 这个缩写在 Data Sources 里到处出现(MCP server、MCP tool count),但我没有任何地方解释 MCP 是什么。

**疑惑点**:
- "Create Agent"按钮——我点了,弹出来一个表单要写"agent description"。我不知道该写什么。**没有任何例子,没有任何模板**。
- "Add Source"按钮——让我填 "command" 和 "args"。这是要让我装软件吗?我连命令行都不会用。
- 这页跟核心需求"问 AI 一个问题"距离最远。**它是给会配置 MCP server 的开发者用的,不是给我用的。**

**情绪反应**:**Extensions 是让我最清楚地意识到"这软件不是给我用的"的一页。** 整个页面假设我知道什么是 MCP、什么是 agent、什么是 skill command。我什么都不会,只能默默关掉。

---

### Automation (Routines / Hook Events / Profiles)

#### Routines

**看到什么**:顶部"Routines — Triggered routines fire automatically on Shannon events (PostToolUse, PreCompact, WorktreeCreate, …)"。"New Routine"按钮。空列表。

**疑惑点**:
- 触发器选项(PostToolUse、PreToolUse、SubagentStart、SubagentStop、SessionStart、SessionEnd、PreCompact、PostCompact、TaskCreated、TaskCompleted、WorktreeCreate、WorktreeRemove、ConfigChange、InstructionsLoaded)——**14 个触发器全是英文术语,我一个都不懂**。SubagentStart?PreCompact?InstructionsLoaded?这每一个词都得让我去查文档。
- 表单里还有 "Matcher"、"Pattern (Regex filter)"——**Regex?** 普通人怎么可能写正则表达式?
- Placeholder 是 "lint-after-edit"、"pnpm lint"、"\.py$"——**全是程序员的例子**。没有一个例子是"每天早上 9 点提醒我交作业"这种普通人能理解的。

**情绪反应**:这一页**100% 是给程序员写的**。我关掉了。

#### Hook Events

**看到什么**:一个"hook event 目录",可以按 category 筛选:Tools、Session、Prompt、Context、Agents、Worktree、Permissions。每个 event 显示 name、category、description、payload_fields。

**疑惑点**:
- "Shannon can run shell commands on N lifecycle events"——**shell command 是什么?lifecycle event 是什么?**
- payload_fields 显示的是字段名(tool_name、tool_input、session_id、cwd)——这分明是 API 文档。**为什么 API 文档会出现在用户界面里?**
- 这页的存在意义是什么?我能在这里做什么?**没有"新建"按钮**,只是浏览。但它和 Routines 是分开的两个菜单——可是 Routines 页又写"to wire a command to one of them, head to /routines"。那这一页就是纯目录,跟普通用户毫无关系。

**情绪反应**:**Hook Events 这一页彻底击垮了我。** 我看了一分钟,完全不知道自己为什么会被带到这里。这是开发者文档伪装成用户界面的典型。

#### Profiles

**看到什么**:"Permission Profiles — Switch profiles via the /profile command in chat. Custom profiles are loaded from .shannon/profiles/."。下面三个内置 profile 卡片,每个卡片显示 Read/Write/Bash/Delete/Network 五个权限开关(auto / needs approval)。还有"New Profile"表单,要填 Auto-approve、Confirm、Deny 三个工具列表。

**疑惑点**:
- 五个权限(Read、Write、Bash、Delete、Network)——Bash 是什么?(后来知道是 Linux 命令行)Network 是什么?
- "Auto-approve (comma-separated)"——让我用逗号分隔工具名:"Read, Glob, Grep, LS"——**Glob 是什么?Grep 是什么?LS 是什么?** 这些都是 Linux 命令。
- 内置 profile 的描述全是英文,而且暗示"风险等级"——但我作为新手,根本不知道哪种组合是"安全"的,哪种是"危险"的。

**情绪反应**:Profiles 是给"知道自己在做什么的开发者"用的。**普通人一辈子都不应该看到这一页。**

---

### OPC (One Person Company)

**看到什么**:顶部一块"Strategic Focus"横幅,默认文字是"Anthropic Agent Orchestration — autonomous task execution with multi-agent coordination."。下面是 Analytics Dashboard、Agent Swarm、Kanban Board(5 列:To Do / Pending / Doing / Done / Deprecated)。

**理解成什么**:**OPC 是 One Person Company(一人公司)的缩写。** 这个名字让我以为这是一个"创业工具"——帮我运营一人公司?但我不是创业者啊。

**疑惑点**:
- 侧边栏在 OPC 旁边还加了个橙色 "Experiment" 小标签——**这是个实验性功能,为什么要给新手看?**
- "Strategic Focus" 默认文字是"Agent Orchestration"——这词太"商业咨询"了,完全脱离我的使用场景。
- Kanban 五列跟 Mission Control 的五列**含义不一样**:这里叫 To Do / Pending / Doing / Done / Deprecated,而 Mission Control 叫 Queued / In Progress / Blocked / Completed / Failed。**为什么同一个产品有两套不同的看板术语?** 作为用户我彻底混乱了。
- Agent Swarm(群体智能?)这种科幻词汇让我觉得这个产品在卖概念,不是在解决问题。

**情绪反应**:**OPC 是把混乱推向顶点的一页。** 它加上 Mission Control、Tasks、Goals,**四个页面都是某种"任务/项目/工作流管理",但每一个都不一样,而且都用不同的英文术语。** 我作为新手,已经完全放弃理解它们之间的区别了。

---

### Quick Fix

**看到什么**:"Quick Fix Launcher — Paste a diagnostic to ask the language server for code actions. The server binary must be on PATH (rust-analyzer, typescript-language-server, gopls, pylsp)."。表单要填 File path、Start line (0-indexed)、Start character (0-indexed)、Diagnostic message、Language(rust/typescript/go/python)。

**理解成什么**:**完全看不懂。** "language server"是什么?"binary on PATH"是什么?rust-analyzer、gopls 这些是程序员工具。

**情绪反应**:这一页**直接关掉**。它在我眼里跟" BIOS 设置"差不多——给我看我也不敢动。

---

### Editor

**看到什么**:"Code Editor — Load a source file to view it with syntax highlighting. Diagnostics auto-fetch from the language server — add manual squiggles to annotate. Click any squiggle to ask the language server for quick-fixes."。让我输入一个文件路径加载。

**疑惑点**:
- "Load a source file"——什么是 source file?源代码文件?
- "add manual squiggles"——squiggle 是"波浪线"?为什么要我手动加波浪线?
- "diagnostics"——诊断?这是医疗软件吗?(程序员用 diagnostics 指"代码诊断/错误检查",但**普通人看不懂这个词**。)

**情绪反应**:又一个**程序员专用页**。我连源代码都不写,这页跟我毫无关系。

---

### Performance

**看到什么**:"Performance — Developer panel: analyze tracing JSON captured from `SHANNON_LOG_FORMAT=json`."。一个 textarea 让我粘贴 JSON,然后 Analyze。

**疑惑点**:
- "Developer panel"——**人家自己就承认了这是开发者面板**。那为什么放在普通用户的主菜单里?
- "tracing JSON"、"SHANNON_LOG_FORMAT=json"——这些环境变量和日志格式跟我有什么关系?
- 输出是 "p50 / p95 latency"、"span close events"、"tool_name counts"——这是性能工程师看的指标,**我连 p50 是什么百分位数都得回忆一下统计学**。

**情绪反应**:这一页**纯纯粹粹是开发者工具**。它出现在主侧边栏,就是在告诉我"这个产品的目标用户根本不包括我"。

---

### Settings

**看到什么**:五个子菜单:General、Theme、Models、Usage & Billing、Advanced。

**General**:一个 5 档"Approval Mode"滑杆:Suggest / Confirm / Plan / Auto Edit / Full Auto。

**Theme**:四个主题卡片。这个我能看懂。

**Models**:Anthropic/OpenAI/Ollama/DeepSeek tab,每个 tab 一堆 model 名字(claude-3-5-sonnet-20241022 这种),还有 Temperature、Max Tokens 滑杆。

**Usage & Billing**:当前 plan、Token 使用环形图、Cache hit rate、Cost analysis bar chart、Billing history。**Change Plan** 按钮打开 Free / Pro / Enterprise 选择。

**Advanced**:Memory management toggle、Clear session cache、Data privacy toggles(telemetry、encryption)、Debug console、View System Logs、Manage API Keys、Factory reset。

**疑惑点**:
- General 里那个 "Approval Mode" 的 5 档——Suggest / Confirm / Plan / Auto Edit / Full Auto。**"Approval"是什么意思?批准什么?** 我后来才明白这是"AI 在动手改我的文件之前要不要问我"。**但 UI 上没有一句中文解释这个核心概念**,默认是哪一档我也不知道(后来发现默认 Confirm)。Full Auto 听起来很危险,但我也没看到任何警告。
- Models 里 Temperature 滑杆——温度?AI 还有温度?Max Tokens——tokens 又来了。
- Usage & Billing 显示"$0.0000"——我想看人民币。Change Plan 显示 Free / Pro / Enterprise 但**价格是空的**,完全不知道订阅要多少钱、怎么付。
- Advanced 里 "Factory reset"——恢复出厂设置,但旁边没说会删什么。
- 整个 Settings **没有任何"账户"概念**:没有登入登出,没有头像,没有"我的资料"。Header 右上角那个人头图标点不动。

**情绪反应**:Settings 散落着各种**只有工程师才关心的开关**。真正重要的"我的账户、我的订阅、我的数据"反而找不到。

---

## 我完全看不懂的术语表

下面列出我作为新手**完全不懂、只能瞎猜**的术语:

| 术语 | 我猜它是什么 | 真正意思 | 是否该在用户界面出现 |
|------|------|------|------|
| Provider | 提供商?电信运营商? | 大模型厂商(Anthropic/OpenAI 等) | 不该,应叫"AI 模型" |
| API Key | 钥匙?密码? | 调用 AI 接口的认证字符串 | 不该让普通用户接触 |
| Tokens | 代币?积分? | LLM 处理文本的最小单位 | 不该放在底部状态栏 |
| Context Window | 上下文窗口? | 模型一次能处理的最大 token 数 | 完全不该暴露 |
| Agent | 代理人?中介? | 自主执行的 AI 子任务实例 | 应叫"AI 助手"或"分身" |
| Subagent | 子代理人? | Agent 派生的下级 Agent | 程序员词 |
| Skill | 技能? | 命令模板(/commit 这种) | 应叫"快捷命令"或"模板" |
| MCP | ?? | Model Context Protocol,扩展协议 | **必须**解释,不能裸用 |
| LSP | ?? | Language Server Protocol | 不该出现在普通用户界面 |
| Diagnostic | 医学诊断? | 代码错误检查 | 不该 |
| Squiggle | 波浪线? | 编辑器错误下划线 | 不该 |
| Hook | 钩子? | 在生命周期事件上挂脚本 | 程序员词 |
| Hook Event | 钩子事件? | 可挂脚本的生命周期事件 | 不该 |
| Routine | 例行公事? | 触发式或定时的自动化任务 | 可保留,但触发器要中文化 |
| Trigger | 扳机?触发? | 启动 routine 的事件 | 可保留 |
| Matcher / Pattern (Regex) | 匹配器?正则? | 工具名过滤 / 正则过滤 | 不该让普通用户写 |
| Profile | 个人资料? | 一组权限规则的命名预设 | 词义冲突,应叫"权限预设" |
| Approval Mode | 批准模式? | AI 改文件前的确认级别 | 应叫"AI 自主程度" |
| Full Auto | 全自动? | AI 完全自主,不需确认 | 危险,需警告 |
| DAG | ?? | 有向无环图(任务依赖关系) | 不该 |
| Worktree | 工作树? | Git worktree(独立工作目录) | 不该让普通用户接触 |
| Mission Control | 太空指挥? | 跨团队只读任务看板 | 名字夸张,应叫"全局看板" |
| Triage | 分诊? | 待处理事件队列 | 不该用医院词 |
| OPC | ?? | One Person Company(实验功能) | 缩写无任何提示 |
| One Person Company | 一人公司? | 实验性的多 agent 编排工作台 | 跟字面意思完全不符 |
| Kanban | ?? | 看板(任务管理术语) | 可保留但需解释 |
| Agent Swarm | 群体? | 一组协同 agent | 不该,科幻词 |
| Strategic Focus | 战略焦点? | 项目目标文本框 | 太"咨询",应叫"项目目标" |
| Shell command | ?? | 操作系统命令行指令 | 不该让普通用户写 |
| Cron | ?? | Unix 定时任务表达式 | 不该 |
| Webhook | ?? | HTTP 回调 URL | 不该 |
| Provider dashboard | ?? | 模型厂商的网页控制台 | 不该假设用户知道 |
| p50 / p95 latency | ?? | 百分位延迟 | 完全是 SRE 词汇 |
| Span / Span close | ?? | tracing 里的一段计时区间 | 不该 |
| Bash | ?? | Linux 命令行解释器 | 不该作为权限名给普通用户 |
| Glob / Grep / LS | ?? | 文件名匹配/文本搜索/列目录 | Linux 命令,不该 |
| Payload fields | ?? | 事件数据字段 | API 文档词汇 |
| Local model (Ollama) | 本地模型? | 在自己电脑跑的模型 | 需解释"占内存/要装别的" |

**总数**:35+ 个普通用户根本看不懂的术语,**全部直接出现在主用户界面**。

---

## 让我想关掉的 3 个瞬间 (Top 3 阻断点)

### 阻断点 #1:Welcome 页让我选 Provider 并填 API Key

打开应用 5 秒,我就被一道墙挡住了:
- 4 个英文名字(Anthropic / OpenAI / Ollama / DeepSeek),没有"这是什么"的解释。
- "recommended for coding" 一句话直接告诉我"这不是给你用的"。
- 要我填 API Key,但不知道 key 怎么拿、要花多少钱、能不能微信支付。

**这一刻我差点直接关掉软件。** 如果不是朋友的儿子推荐,我绝对不会再打开第二次。

**根本问题**:产品假设"用户已经知道自己想要哪个 AI 提供商,而且已经准备好 API key"。这对完全的新手是**致命**的——我们既不知道选什么,也不知道怎么搞 key,更不知道要花多少钱。

### 阻断点 #2:左侧菜单 11+ 个一级/二级项,全是英文术语

侧边栏从上到下:New Chat、Chat、Goals、Scheduled、Mission Control、Triage、Extensions(Skills / My Agents / Data Sources)、Automation(Routines / Hook Events / Profiles)、OPC(One Person Company)、Quick Fix、Editor、Performance、Settings(General / Theme / Models / Usage & Billing / Advanced)。

**14+ 个菜单项,没有一个中文,至少 8 个术语我不认识**(Triage、OPC、Hook Events、Profiles、DAG、Worktrees、Mission Control、Agent)。

**根本问题**:菜单的设计目标是"覆盖所有功能",而不是"引导新手完成第一件事"。普通用户面对这个菜单的**第一反应是 overwhelmed**——我不知道该点哪个,也不知道这些名字之间的关系。

### 阻断点 #3:Tasks / OPC / Mission Control / Goals 四页功能高度重叠又互不相同

四个页面都跟"任务管理"有关:
- **Tasks**:任务 CRUD + 日历 + DAG + 工作树 + 历史。
- **OPC**:Strategic Focus + Kanban + Agent Swarm。
- **Mission Control**:只读 5 列看板(Queued / In Progress / Blocked / Completed / Failed)。
- **Goals**:Task Management 标题 + 任务树 + Agent Reasoning + AI 输入框。

**问题**:
1. 四个页面用了**四套不同的列名/术语**(Mission Control 是 Queued/In Progress/Blocked...,OPC 是 To Do/Pending/Doing...,Tasks 里又是 status/priority)。
2. 我作为新手**完全不知道该在哪一页做什么**——建任务去 Tasks?还是 Goals?OPC 里那个 Quick Task 输入框也能建任务,那它跟 Tasks 有什么区别?
3. Goals 页底部有个"Ask about this goal..."输入框,这个输入框跟 Chat 页的输入框**有什么区别**?它发的消息去哪了?

**根本问题**:产品没有清晰的"信息架构"。同样的概念在不同页面用不同名字出现,产品没有告诉我**主路径是什么**(我猜主路径应该是 Chat → 让 AI 帮我做事 → 看任务在 Tasks 里执行),但它把所有页面平铺在侧边栏,让用户自己挑。

---

## 我作为新手最想做的 5 件事,能不能做?

### 1. 问 AI 一个问题(润色一段家长群发言)

**能不能做**:✅ 勉强能。

**路径**:打开应用 → Skip 掉 Welcome → 自动跳到 Chat → 输入框里打字 → 发送。

**问题**:
- Welcome 这道墙我得先 Skip 掉,但 Skip 之后**没有任何模型配置**(因为我没填 API key),后面发送消息会失败(报错信息是英文的、说"provider not configured")。
- 即使配好了,Chat 页空状态的 4 个例子全是写代码的,**没有一个能让我这种人快速上手**。我应该怎么问 AI 才能得到好结果?没有任何引导。

### 2. 把 Excel 整理成分类清晰的版本

**能不能做**:❌ 几乎不能。

**问题**:
- Chat 输入框能"attach file",我试着拖一个 .xlsx 进去,**不知道 AI 是否能真的读取**。
- 就算 AI 给了我整理后的结果,**我不知道怎么把结果导出回 Excel**——没有"导出"按钮,没有"另存为"。
- 这一类的"实际办公任务"在所有例子里、所有 skill 里、所有 routine 模板里**都没有任何体现**。

### 3. 起草一份校运会开幕词

**能不能做**:✅ 能,但**全靠自己琢磨**。

**路径**:Chat → 打"帮我写一份校运会开幕词" → 等 AI 回复 → 复制粘贴到 Word。

**问题**:
- 这事实际上跟 ChatGPT/Kimi 没有差别。Shannon 没有提供任何"模板库"、"写作场景"、"风格预设"来让我这种新手得到比 ChatGPT 更好的体验。
- 没有提示词建议,我得自己想清楚要什么(字数、语气、是否要文言文色彩)。

### 4. 自动整理我电脑里的文件夹

**能不能做**:❌ 不能,而且差得很远。

**问题**:
- 我朋友说 AI 能"自动做事",我以为点几下就行。
- 实际上要做这件事,我需要:
  1. 去 Automation → Routines → New Routine,填一个 shell command(我不会写)。
  2. 或者在 Tasks → Schedule 里填一个 cron 表达式(我不会写)。
  3. 或者在 Extensions → My Agents 里"Create Agent"(我不知道写什么描述)。
- **没有任何"模板"或"向导"告诉我:"想让 AI 帮你整理文件夹?点这里。"** ScheduleTemplates 这个组件存在,但所有模板都是程序员场景(lint-after-edit 这种)。

### 5. 设置一个"每天早上 9 点提醒我做某事"的定时任务

**能不能做**:⚠️ 理论上能,实际上几乎不可能。

**问题**:
- 路径是 Tasks → Schedule → 填一个表单。表单里要选 trigger_type(interval / cron / webhook),还要填 cron 表达式或者 interval 秒数。
- **没有"每天 9 点"这种自然语言输入框**(虽然后端据我朋友儿子说支持 NL cron,但 UI 上看不到)。
- 没有"提醒"的语义——Shannon 把一切任务都视为"运行 AI prompt",没有"通知我"这种纯粹的提醒。
- 我作为新手根本不知道这一功能在 Tasks 页的 Schedule 按钮里——我会去 Settings 找,Settings 里没有;我会去 Routines 找,Routines 全是触发式不是定时式。

---

## 改进建议(按优先级 P0/P1/P2)

### P0 - 不改就没法用

**P0-1. 砍掉 Welcome 的 Provider/API Key 选择,改成"免费试用 / 我已有账号"二选一**
- 完全新手的入门路径应该是:打开 → 看到一个聊天框 → 直接开始用(可以用免费额度或者产品方提供的共享 key)。
- 进阶用户才需要去 Settings 配自己的 Provider。
- 现在 Welcome 这一关直接劝退所有非技术用户。

**P0-2. 侧边栏按"角色"裁剪,默认只露 3-4 个菜单**
- 普通用户默认只看到:Chat、Tasks(改名)、Settings。
- Extensions / Automation / OPC / Quick Fix / Editor / Performance 这些放进一个折叠的"开发者模式"(Settings → 切换开发者模式),默认关闭。
- Goals / Mission Control / Triage / OPC 至少合并成一页(或者干脆默认隐藏——它们都依赖"用户已经有 agent 在跑",新用户进来全是空状态)。

**P0-3. 整体术语中文化(或至少做术语悬停解释)**
- Triage → 待处理 / 通知中心
- OPC → 删除(或藏起来)
- Hook Events → 自动化触发条件
- Profiles → 权限预设
- Goals / Mission Control → 合并为"我的任务"
- Skill → 模板/快捷命令
- Agent → AI 助手
- Approval Mode → AI 自主程度
- 在所有保留的术语旁边加一个小问号图标,悬停显示中文解释 + 例子。

**P0-4. Chat 空状态的 4 个例子换掉,改成普通用户场景**
- "帮我润色一段话"
- "整理这份 Excel"
- "起草一份发言稿"
- "翻译这段中文成英文"
- 写代码的例子留作"开发者模式"开启后才显示。

**P0-5. 把 tokens / Context Window / 模型版本号 / 美元金额从普通视图隐藏**
- 默认状态栏只显示"已连接"或"未连接"+ 一个抽象的"今日用量"图标。
- 详细用量进 Settings → Usage 才看得到,而且**显示人民币**而不是美元。

### P1 - 改了会好很多

**P1-1. 引入"场景模板库"作为新手主入口**
- 第一次进入 Chat 时,顶部显示一行场景卡:写作 / 翻译 / 总结 / 整理数据 / 头脑风暴。
- 点其中一个 → 弹出一个针对该场景的引导式表单(我要的字数、风格、目标读者)。
- 让新手不需要会写 prompt 也能得到好结果。

**P1-2. Routines 表单加"自然语言"输入**
- 现在的 Routines 表单要选 trigger + 写 shell command + 写 regex——**完全是给开发者**。
- 应该加一个"我想做什么"的自然语言输入框:"每天晚上 10 点帮我备份桌面截图"。后端解析成 cron + 命令。
- 把 14 个英文触发器用中文分组:"使用工具时"、"对话过程中"、"任务状态变化"、"文件操作"。

**P1-3. Tasks / Goals / Mission Control / OPC 四页合并**
- 新手用户只需要一个"任务"页:正在跑的 AI 任务 + 历史记录。
- "看板视图"、"日历视图"、"DAG 视图"、"工作树"这些**全部折叠到右上角"切换视图"按钮里**,而且默认不显示 DAG/Worktree(那是开发者视图)。
- 把 OPC 的"Strategic Focus"改名"我的目标",并默认放在 Tasks 页顶部作为可选填写项。

**P1-4. Profiles / Permission 系统简化为"AI 自主程度"3 档**
- 不要让普通用户填 "Auto-approve: Read, Glob, Grep" 这种工具列表。
- 改成 3 档预设:
  - 谨慎(每次都问我)
  - 平衡(只读不写自动,改动问我)
  - 自主(全权代办,危险操作除外)
- 进阶用户才能在 Settings → 高级 里看到逐工具配置。

**P1-5. 把"内置中文输入法友好"做扎实**
- Chat 输入框目前用 Enter 发送——但**中文输入法在选词时按 Enter 应该是确认候选词,不是发送**。我没测试过 Shannon 是否处理了 compositionstart/compositionend 事件,但这种细节对中文用户是**致命**的。如果没处理好,新手根本没法用中文输入长文本。

**P1-6. Settings 增加"账户 / 订阅 / 付款"**
- 普通用户期望 Settings 里有:我的账号、登出、订阅状态、付款方式、发票。
- 现在 Settings 完全没有这些——只有"模型参数""权限""高级"。这跟消费者产品的预期**严重不符**。

### P2 - 锦上添花

**P2-1. 字体大小可调**
- 中老年用户希望全局放大字体。Theme 里加一个字号选项。

**P2-2. 把"导出对话"做成主功能**
- 现在会话列表 hover 才出现"Export"图标。普通用户的核心需求之一是"把 AI 给我的内容存成 Word/PDF"。建议在每条 AI 回复上加"导出"按钮,支持 .docx / .pdf / .md。

**P2-3. 加一个"我刚学会了什么"的进度记录**
- 给新手一种成就感:你今天第一次用了 Chat、第一次创建了一个任务、第一次设置了自动化。
- 这种 onboarding 心理设计在消费者产品里非常重要。

**P2-4. "AI 自主程度"切换时加强警告**
- 切到 Full Auto 时弹一个明确的红色警告:"AI 将自主执行所有操作,包括修改/删除你的文件。确定吗?"
- 现在这一档的危险性在 UI 上完全没体现。

**P2-5. 把 Performance 页移出主菜单**
- 它自己都说"Developer panel",那就放进 Settings → 开发者,不要污染主侧边栏。
- Quick Fix / Editor 同理。

**P2-6. 删除 Goals 页或彻底改造**
- Goals 这一页的存在意义最小:跟 Tasks 重复、跟 Chat 重复。要么删,要么彻底重新设计成"长期目标 → 分解任务 → AI 协助"的真正目标管理工具。

---

## 总结评分(各维度 /10)

| 维度 | 分数 | 说明 |
|------|------|------|
| **第一印象友好度** | 2 / 10 | Welcome 页直接要 API Key + 选 provider,4 个英文公司名,"recommended for coding"。新手 5 秒想关。 |
| **上手难度** | 2 / 10 | 没有引导式 onboarding,没有场景模板,所有功能平铺在 14+ 个菜单项里,新人不知道从哪开始。 |
| **术语友好度** | 1 / 10 | 35+ 个普通用户看不懂的术语直接出现在主界面(Triage、OPC、Hook、MCP、LSP、DAG、Worktree、tokens、p95 latency...)。术语本身没问题,问题是**没有任何解释**就裸用。 |
| **视觉引导** | 4 / 10 | Chat 空状态友好;但 Goals/Tasks/Mission Control/Triage/OPC 的空状态全是"什么都没有"的空白,没有任何"下一步做什么"的提示。 |
| **容错/探索安全感** | 2 / 10 | 大量按钮让我害怕:Quick Fix 要我填 0-indexed 行号、Routines 要我写 shell 命令、Performance 要我粘贴 JSON、Profiles 让我配置权限工具列表。**每一个都让我担心"点错了会不会出事"**。Full Auto 这种危险档位没有任何警告。 |
| **针对目标用户(普通消费者)的契合度** | 1 / 10 | 整个产品从命名(Shannon Code / AI Code Assistant)、到例子(全是写代码)、到术语(全是程序员词)、到默认功能(Editor / Quick Fix / Performance / Worktree / DAG),**处处都在说"我是给程序员用的"**。 |
| **作为已有产品的潜力(转型之后)** | 6 / 10 | 底层能力(多 provider、流式、agent、自动化、权限)是扎实的。如果做一次彻底的"消费者化"重构——砍菜单、中文化、加场景模板、改 onboarding——这个产品**有可能**变成好用的消费者 AI 助手。 |

### 一句话总结

> **现在的 Shannon Desktop 不是"AI 桌面助手",而是"AI 代码助手 + 一堆项目管理工具"的混合体。它的每一寸界面都在告诉我"我是给会写代码、懂 Linux、会用 Git 的人用的"。作为一个完全没用过 AI 的 52 岁老师,我在它面前感觉自己像个闯进手术室的患者——到处都是我不认识的器械,没有任何人告诉我应该躺在哪张床上。**
