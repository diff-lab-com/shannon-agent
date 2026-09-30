# 12 主题对照表（chat 页实拍）

> 生成方式：`pnpm demo` + Playwright 逐主题设置 `localStorage.shannon-theme` 后截图。
> 每张图均为 /chat 页 1280×800 实拍（玻璃 composer + 侧栏 + 消息流可见，界面语言固定 zh-CN）。
> 位置：[screenshots/themes/](./screenshots/themes/)

| 深色主题 | 浅色主题 |
|---|---|
| [`tokyo-night`](./screenshots/themes/chat-tokyo-night.png)（**默认**） | [`material`](./screenshots/themes/chat-material.png) |
| [`catppuccin`](./screenshots/themes/chat-catppuccin.png) | [`tokyo-night-light`](./screenshots/themes/chat-tokyo-night-light.png) |
| [`nord`](./screenshots/themes/chat-nord.png) | [`ember`](./screenshots/themes/chat-ember.png) |
| [`solarized`](./screenshots/themes/chat-solarized.png) | [`slate`](./screenshots/themes/chat-slate.png) |
| [`dracula`](./screenshots/themes/chat-dracula.png) | [`solarized-light`](./screenshots/themes/chat-solarized-light.png) |
| [`gruvbox`](./screenshots/themes/chat-gruvbox.png) | [`gruvbox-light`](./screenshots/themes/chat-gruvbox-light.png) |

材料说明：全部主题共享同一 Liquid Glass token 公式（`--glass-tint-alpha` 按 mode 0.62/0.48），仅 base 色随主题变化——玻璃质感与可读性由 CI 的 contrast-audit（AA）与 token 门禁共同保证。

由 `pnpm gallery:shoot` 生成于 2026-09-30。
