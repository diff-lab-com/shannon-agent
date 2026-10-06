# 状态反馈准则

一页速查：出现"出错了 / 正在加载"时，用哪个组件。所有路径相对
`desktop/ui/src/`。原则：错误分级呈现、加载态与布局结构对齐——不要用
toast 承载页面级错误，也不要用整页 Loading 覆盖已经渲染出的内容。

## 一、错误反馈：三个通道

先判断错误的作用范围，再选通道：

| 范围 | 症状 | 用什么 | 组件路径 |
| --- | --- | --- | --- |
| 页面级 | 整页数据加载失败，页面没有可用的主内容 | `ErrorState`（图标 + 标题 + 描述 + 重试按钮），占据页面主体 | `components/ui/error-state.tsx` |
| 区块级 | 页面主体可用，某个面板/卡片/区块的数据失败 | `Banner`（内嵌在该区块顶部，可带操作按钮），不打断其余内容 | `components/ui/banner.tsx` |
| 操作级 | 用户刚触发的动作失败（保存、发送、切换开关…），或后台任务失败 | toast（sonner，经 `lib/errorToast.ts` 的 `toastError`），不打断操作流 | `Toaster` 挂载于 `App.tsx` + `lib/errorToast.ts` |

判定细节：

- **页面级**：首次加载即失败、无缓存数据可展示 → `ErrorState`。用户需要
  一个明确的"重试"出口（`action` prop）。
- **区块级**：同一页面里其它区块仍然可用 → `Banner`，放在失败区块内部，
  不要滚动劫持、不要遮罩。
- **操作级**：动作有明确发起时机 → toast。失败 toast 必须说清"什么失败"
  与"下一步"（重试 / 检查网络）。后台轮询、webhook 等无操作来源的失败
  也走 toast，不弹对话框。
- 对话框内的表单校验错误不是这三类——留在表单字段旁（`components/ui/form.tsx`），
  不要升级为 toast。

## 二、加载态：Skeleton vs Spinner

按"等的是布局还是等待本身"选择：

| 场景 | 用什么 | 组件路径 |
| --- | --- | --- |
| 结构性布局首次出现（卡片网格、列表页、详情面板骨架已知） | Skeleton —— 按最终布局摆骨架，避免内容闪跳（CLS） | `components/SkeletonLoader.tsx`（`Skeleton` / `CardSkeleton` / `ListSkeleton` / `MetricsSkeleton` / `RowSkeleton`） |
| 行内等待（按钮发起后、局部刷新、聊天流中等待下一片段） | `Spinner` —— 小尺寸行内旋转图标，与触发它的控件同处一行 | `components/ui/loading-state.tsx`（`Spinner`） |
| 整块区域无骨架可用、居中的"加载中"占位 | `LoadingState`（size sm/md/lg，可带 label） | `components/ui/loading-state.tsx` |

判定细节：

- 能画出最终布局的 → Skeleton；画不出的 → Spinner/`LoadingState`。
- 同一页面不要混用两种隐喻表达同一个等待；切换按钮的 pending 态一律
  `Spinner`（或禁用 + aria-busy），不整块换 Skeleton。
- 加载容器带 `role="status"` + `aria-live="polite"`（`LoadingState` 已内置）；
  Skeleton 区域也应有可读的 `aria-busy` 或替代文本。
- 操作成功反馈（"已保存"）走 toast，本文件只管错误与加载。
