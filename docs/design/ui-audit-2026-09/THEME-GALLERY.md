# 12 主题对照表（chat 页实拍）

> 生成方式：`pnpm demo` + Playwright 逐主题设置 `localStorage.shannon-theme` 后截图。
> 每张图均为 /chat 页 1280×800 实拍（玻璃 composer + 侧栏 + 消息流可见）。
> 位置：[screenshots/themes/](./screenshots/themes/)

| 深色主题 | 浅色主题 |
|---|---|
| [tokyo-night](./screenshots/themes/chat-tokyo-night.png)（**默认**） | [tokyo-night-light](./screenshots/themes/chat-tokyo-night-light.png) |
| [catppuccin](./screenshots/themes/chat-catppuccin.png) | — |
| [dracula](./screenshots/themes/chat-dracula.png) | — |
| [nord](./screenshots/themes/chat-nord.png) | — |
| [solarized](./screenshots/themes/chat-solarized.png) | [solarized-light](./screenshots/themes/chat-solarized-light.png) |
| [gruvbox](./screenshots/themes/chat-gruvbox.png) | [gruvbox-light](./screenshots/themes/chat-gruvbox-light.png) |
| [ember](./screenshots/themes/chat-ember.png) | — |
| [slate](./screenshots/themes/chat-slate.png) | — |
| [material](./screenshots/themes/chat-material.png) | material（同一 token 块，mode:light） |

材料说明：全部主题共享同一 Liquid Glass token 公式（`--glass-tint-alpha` 按 mode 0.62/0.48），仅 base 色随主题变化——玻璃质感与可读性由 CI 的 contrast-audit（AA）与 token 门禁共同保证。
