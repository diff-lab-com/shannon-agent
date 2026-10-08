// Usage: node shot.mjs <html path> <out png> [width] [height]
// playwright 从主仓库 desktop/ui 解析(设计稿目录无 node_modules)
import { chromium } from 'file:///home/ed/workspace/app/work/shannon/shannon-mono/desktop/ui/node_modules/.pnpm/playwright@1.63.0/node_modules/playwright/index.mjs';

const [,, htmlPath, outPath, w = '1440', h = '900'] = process.argv;
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: +w, height: +h }, deviceScaleFactor: 2 });
await page.goto('file://' + htmlPath);
await page.waitForTimeout(400);
await page.screenshot({ path: outPath });
await browser.close();
console.log('saved', outPath);
