/**
 * P2-1 — the built-in PWA-style mobile page ("浏览器即手机客户端").
 *
 * One self-contained HTML document (inline CSS + JS, no external assets) served
 * by `MobileServer` on GET / of the mobile `shannon/*` port. It speaks the exact
 * same NDJSON JSON-RPC protocol as the native client over the existing WebSocket:
 *
 *   shannon/pair            (paste the one-time token from the desktop QR)
 *   shannon/device.resume   (auto on reconnect, Ed25519-signed)
 *   shannon/task.dispatch   (派发文本 → IM 同款管线；也用于回复审批 y/n)
 *   shannon/task.list       (最近任务 + 状态)
 *   shannon/event           (approval.request / task.message / task.progress / …)
 *
 * Signing uses the vendored TweetNaCl (see naclSource.ts): SubtleCrypto is only
 * available in secure contexts and the LAN page is plain http://, so pure-JS
 * Ed25519 keeps pairing/审批 available on real phones. Key material stays in
 * this device's localStorage; the gateway only ever sees the public key.
 *
 * Deliberately v1: no service worker / offline cache (添加到主屏幕 still works),
 * no QR camera scan (paste the token), no relay-E2E from the browser (LAN direct
 * only — the relay transport is for native clients). These are documented in
 * docs/integrations/mobile-dispatch.md.
 */

import { NACL_SOURCE } from "./naclSource.js";

/** The full page. Composed so the vendored nacl lands in its own <script>. */
export const MOBILE_PAGE_HTML: string = `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="theme-color" content="#0b1020">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="mobile-web-app-capable" content="yes">
<title>Shannon 移动派发</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: #0b1020; color: #e6eaf2;
    font: 15px/1.55 -apple-system, "PingFang SC", "Noto Sans SC", "Microsoft YaHei", sans-serif;
    padding-bottom: env(safe-area-inset-bottom);
  }
  header {
    position: sticky; top: 0; z-index: 5; display: flex; align-items: center; gap: 8px;
    padding: 12px 16px; background: #0b1020e6; backdrop-filter: blur(6px);
    border-bottom: 1px solid #232a44;
  }
  header h1 { font-size: 17px; margin: 0; flex: 1; }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: #56607a; }
  .dot.on { background: #34c77b; } .dot.pair { background: #3d8bff; }
  .dot.err { background: #ff5d5d; }
  main { padding: 12px 16px 32px; max-width: 640px; margin: 0 auto; }
  section { background: #121a33; border: 1px solid #232a44; border-radius: 12px; padding: 12px; margin-bottom: 12px; }
  section h2 { font-size: 13px; margin: 0 0 8px; color: #93a0c4; font-weight: 600; }
  label { display: block; font-size: 12px; color: #93a0c4; margin: 8px 0 4px; }
  input, textarea {
    width: 100%; background: #0b1020; color: #e6eaf2; border: 1px solid #2c3554;
    border-radius: 8px; padding: 10px; font-size: 15px;
  }
  textarea { min-height: 72px; resize: vertical; }
  .row { display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap; }
  button {
    flex: 1; min-height: 42px; border: 0; border-radius: 9px; font-size: 15px; font-weight: 600;
    background: #2c3554; color: #e6eaf2; padding: 8px 14px;
  }
  button.primary { background: #3d6bff; color: #fff; }
  button.ok { background: #1f9d5b; color: #fff; }
  button.no { background: #b3363c; color: #fff; }
  button.ghost { background: transparent; border: 1px solid #2c3554; color: #93a0c4; }
  button:disabled { opacity: 0.45; }
  .hint { font-size: 12px; color: #76829f; margin-top: 6px; }
  .ok-msg { color: #34c77b; font-size: 13px; }
  .err-msg { color: #ff8484; font-size: 13px; white-space: pre-wrap; }
  #approval { display: none; border-color: #b3363c; }
  #approval .tool { font-weight: 700; }
  #approval .desc { font-size: 13px; color: #c6cde0; margin: 6px 0; white-space: pre-wrap; }
  .badge { display: inline-block; font-size: 11px; padding: 1px 7px; border-radius: 99px; margin-left: 6px; }
  .badge.destr { background: #b3363c; color: #fff; }
  .feed { display: flex; flex-direction: column; gap: 6px; max-height: 52vh; overflow-y: auto; }
  .msg { background: #1a2344; border-radius: 10px; padding: 8px 11px; white-space: pre-wrap; word-break: break-word; }
  .msg.me { background: #24408a; align-self: flex-end; }
  .msg.stamp { color: #aeb9d6; }
  .msg.prog { color: #7fe3a8; font-family: ui-monospace, Menlo, monospace; font-size: 12px; }
  .statusline { font-size: 12px; color: #93a0c4; }
  ul.tasks { list-style: none; margin: 0; padding: 0; }
  ul.tasks li { border-top: 1px solid #232a44; padding: 8px 2px; font-size: 13px; }
  ul.tasks li:first-child { border-top: 0; }
  .chip { float: right; font-size: 11px; padding: 1px 8px; border-radius: 99px; }
  .chip.running { background: #2b4d9b; } .chip.completed { background: #1f6b45; } .chip.failed { background: #7c2d31; }
  .title { color: #e6eaf2; display: block; }
  .terr { color: #ff9d9d; font-size: 12px; white-space: pre-wrap; }
  code.small { font-size: 11px; color: #76829f; word-break: break-all; }
</style>
</head>
<body>
<header>
  <span id="dot" class="dot" aria-hidden="true"></span>
  <h1>Shannon 移动派发</h1>
  <button id="btnTasks" class="ghost" style="flex:0 0 auto;min-height:34px">任务</button>
</header>
<main>
  <section id="conn">
    <h2>连接</h2>
    <label for="wsUrl">网关地址</label>
    <input id="wsUrl" autocapitalize="off" autocorrect="off" spellcheck="false">
    <div class="row">
      <button id="btnConnect" class="primary">连接</button>
    </div>
    <p id="connMsg" class="hint"></p>
  </section>

  <section id="pair">
    <h2>配对</h2>
    <div id="pairIntro">
      <label for="pairToken">配对令牌</label>
      <input id="pairToken" autocapitalize="off" autocorrect="off" spellcheck="false" placeholder="75 秒一次性令牌（桌面端生成）">
      <div class="row">
        <button id="btnPair" class="primary">配对</button>
      </div>
      <p class="hint">在桌面端 设置 → 连接 → 移动派发 点击「生成配对码」，把令牌粘贴到上面（75 秒内有效）。</p>
    </div>
    <div id="pairInfo" style="display:none">
      <p class="ok-msg" id="pairLabel"></p>
      <code class="small" id="pairDevice"></code>
      <div class="row"><button id="btnReset" class="ghost">解除配对并重置</button></div>
    </div>
  </section>

  <section id="approval">
    <h2>审批请求</h2>
    <p><span class="tool" id="apTool"></span><span id="apDestr" class="badge destr" style="display:none">危险操作</span></p>
    <p class="desc" id="apDesc"></p>
    <div class="row">
      <button id="apOk" class="ok">✅ 批准</button>
      <button id="apNo" class="no">❌ 拒绝</button>
    </div>
    <p class="hint">也可以在输入框直接回复 y / n（同钉钉文本审批）。</p>
  </section>

  <section>
    <h2>派发任务</h2>
    <textarea id="taskText" placeholder="要做什么，直接写下来…"></textarea>
    <div class="row">
      <button id="btnDispatch" class="primary">派发</button>
    </div>
    <p id="taskMsg" class="hint"></p>
  </section>

  <section>
    <h2>消息</h2>
    <div id="feed" class="feed"></div>
  </section>

  <section id="tasksPanel" style="display:none">
    <h2>最近任务</h2>
    <ul id="taskList" class="tasks"></ul>
    <p id="taskListEmpty" class="hint">暂无任务</p>
    <div class="row"><button id="btnRefreshTasks" class="ghost">刷新</button></div>
  </section>
</main>
<script>
` + NACL_SOURCE + `
</script>
<script>
'use strict';
// ── tiny helpers ───────────────────────────────────────────────────────────
function $(id) { return document.getElementById(id); }
function b64u(bytes) {
  var s = '', b = new Uint8Array(bytes);
  for (var i = 0; i < b.length; i++) s += String.fromCharCode(b[i]);
  return btoa(s).replace(/\\+/g, '-').replace(/\\//g, '_').replace(/=+$/, '');
}
function ub64u(s) {
  s = s.replace(/-/g, '+').replace(/_/g, '/');
  while (s.length % 4) s += '=';
  var bin = atob(s), b = new Uint8Array(bin.length);
  for (var i = 0; i < bin.length; i++) b[i] = bin.charCodeAt(i);
  return b;
}
function utf8(str) { return new TextEncoder().encode(str); }

// ── persisted device identity ──────────────────────────────────────────────
var LS = 'shannon.mobile.device';
function loadDevice() {
  try { return JSON.parse(localStorage.getItem(LS) || 'null'); } catch (e) { return null; }
}
function saveDevice(d) { localStorage.setItem(LS, JSON.stringify(d)); }
function clearDevice() { localStorage.removeItem(LS); }

function deviceKeypair(d) {
  var seed = ub64u(d.seed);
  var kp = nacl.sign.keyPair.fromSeed(seed);
  return { secretKey: kp.secretKey, publicKey: kp.publicKey }; // secretKey = seed||pub (64B)
}
function deviceLabel() {
  var ua = navigator.userAgent;
  if (/Android/i.test(ua)) return 'Android 浏览器';
  if (/iPhone|iPad|iPod/i.test(ua)) return 'iOS 浏览器';
  return '桌面浏览器';
}

// ── JSON-RPC over the shannon/* WebSocket ──────────────────────────────────
var ws = null, nextId = 1;
var pending = new Map(); // id → {resolve, reject}
var state = { connected: false, deviceId: null };

function wsUrl() {
  var v = $('wsUrl').value.trim();
  if (!v) v = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/';
  if (v.indexOf('ws') !== 0) v = 'ws://' + v;
  return v;
}
function setDot(cls, msg) {
  $('dot').className = 'dot ' + cls;
  if (msg !== undefined) $('connMsg').textContent = msg;
}
function rpc(method, params) {
  return new Promise(function (resolve, reject) {
    if (!ws || ws.readyState !== 1) { reject(new Error('未连接网关')); return; }
    var id = nextId++;
    pending.set(id, { resolve: resolve, reject: reject });
    ws.send(JSON.stringify({ jsonrpc: '2.0', id: id, method: method, params: params || {} }));
  });
}
function connect() {
  try { if (ws) { ws.onclose = null; ws.close(); } } catch (e) {}
  setDot('', '连接中…');
  ws = new WebSocket(wsUrl());
  ws.onopen = function () {
    state.connected = true;
    setDot('on', '已连接到网关');
    var d = loadDevice();
    if (d && d.deviceId) resume();
  };
  ws.onmessage = function (e) {
    var lines = String(e.data).split('\\n');
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].trim();
      if (!line) continue;
      var msg;
      try { msg = JSON.parse(line); } catch (err) { continue; }
      if (msg.id !== undefined && pending.has(msg.id)) {
        var p = pending.get(msg.id);
        pending.delete(msg.id);
        if (msg.error) p.reject(new Error(msg.error.message || ('错误 ' + msg.error.code)));
        else p.resolve(msg.result);
      } else if (msg.method === 'shannon/event') {
        handleEvent(msg.params || {});
      }
    }
  };
  ws.onclose = function () {
    state.connected = false;
    setDot('err', '连接已断开');
    setTimeout(function () { if (!state.connected) connect(); }, 2500);
  };
  ws.onerror = function () { setDot('err', '连接失败：检查地址与网络（需与桌面同一局域网）'); };
}

// ── pairing ────────────────────────────────────────────────────────────────
function pair() {
  var token = $('pairToken').value.trim();
  if (!token) { $('connMsg').textContent = '请先粘贴配对令牌'; return; }
  var seed = new Uint8Array(32);
  crypto.getRandomValues(seed);
  var kp = nacl.sign.keyPair.fromSeed(seed);
  var pubB64 = b64u(kp.publicKey);
  var pop = b64u(nacl.sign.detached(utf8(token + ':' + pubB64), kp.secretKey));
  rpc('shannon/pair', {
    pair_token: token,
    device_public_key: pubB64,
    pop_signature: pop,
    device_label: deviceLabel()
  }).then(function (r) {
    saveDevice({ seed: b64u(seed), pub: pubB64, deviceId: r.device_id, label: r.device_label || deviceLabel() });
    $('pairToken').value = '';
    showPaired();
    pushFeed('msg me', '已配对 ✓');
  }).catch(function (err) {
    $('connMsg').textContent = '配对失败：' + err.message + '（令牌是一次性的，请重新生成）';
  });
}
function resume() {
  var d = loadDevice();
  if (!d || !d.deviceId) return;
  var kp = deviceKeypair(d);
  var ts = Date.now();
  var sig = b64u(nacl.sign.detached(utf8(d.deviceId + ':' + ts), kp.secretKey));
  rpc('shannon/device.resume', { device_id: d.deviceId, timestamp: ts, signature: sig })
    .then(function () {
      state.deviceId = d.deviceId;
      showPaired();
    })
    .catch(function () { clearDevice(); showPairIntro(); });
}
function showPaired() {
  var d = loadDevice();
  if (!d) { showPairIntro(); return; }
  state.deviceId = d.deviceId;
  $('pairIntro').style.display = 'none';
  $('pairInfo').style.display = 'block';
  $('pairLabel').textContent = '已配对：' + (d.label || '设备');
  $('pairDevice').textContent = d.deviceId;
  setDot('pair', '已连接并配对');
}
function showPairIntro() {
  state.deviceId = null;
  $('pairIntro').style.display = 'block';
  $('pairInfo').style.display = 'none';
}

// ── dispatch / approval / list ─────────────────────────────────────────────
var currentApproval = null;

function dispatchText(text) {
  if (!text.trim()) return Promise.resolve();
  return rpc('shannon/task.dispatch', { text: text }).then(function (r) {
    if (r.kind === 'approval') {
      pushFeed('statusline', '已' + (r.choice === 'allow' ? '批准' : '拒绝') + '审批请求');
      hideApproval();
    } else {
      pushFeed('msg me', text);
      $('taskMsg').textContent = '任务已创建：' + (r.task_id || '').slice(0, 8) + '…';
    }
  }).catch(function (err) {
    $('taskMsg').textContent = '派发失败：' + err.message;
    pushFeed('err-msg', '派发失败：' + err.message);
  });
}
function showApproval(ev) {
  currentApproval = ev.request_id;
  $('apTool').textContent = ev.tool_name || '工具调用';
  $('apDesc').textContent = ev.description || '';
  $('apDestr').style.display = ev.is_destructive ? 'inline-block' : 'none';
  $('approval').style.display = 'block';
  $('approval').scrollIntoView({ behavior: 'smooth', block: 'nearest' });
}
function hideApproval() {
  currentApproval = null;
  $('approval').style.display = 'none';
}
function refreshTasks() {
  rpc('shannon/task.list', { limit: 20 }).then(function (r) {
    var ul = $('taskList');
    ul.innerHTML = '';
    var tasks = r.tasks || [];
    $('taskListEmpty').style.display = tasks.length ? 'none' : 'block';
    var chipText = { running: '运行中', completed: '已完成', failed: '失败' };
    for (var i = 0; i < tasks.length; i++) {
      var t = tasks[i];
      var li = document.createElement('li');
      var chip = document.createElement('span');
      chip.className = 'chip ' + t.status;
      chip.textContent = chipText[t.status] || t.status;
      var title = document.createElement('span');
      title.className = 'title';
      title.textContent = t.title || t.text;
      li.appendChild(chip);
      li.appendChild(title);
      if (t.error) {
        var er = document.createElement('span');
        er.className = 'terr';
        er.textContent = t.error;
        li.appendChild(er);
      }
      ul.appendChild(li);
    }
  }).catch(function (err) { pushFeed('err-msg', '任务列表获取失败：' + err.message); });
}

// ── event feed ─────────────────────────────────────────────────────────────
function pushFeed(cls, text) {
  var el = document.createElement('div');
  el.className = cls;
  el.textContent = text;
  var feed = $('feed');
  feed.appendChild(el);
  while (feed.childNodes.length > 200) feed.removeChild(feed.firstChild);
  feed.scrollTop = feed.scrollHeight;
}
function handleEvent(ev) {
  switch (ev.type) {
    case 'approval.request':
      pushFeed('msg stamp', '🔐 审批请求：' + (ev.description || ev.tool_name || ''));
      showApproval(ev);
      break;
    case 'task.message':
      pushFeed('msg' + (/^[🚀✅❌]/.test(ev.text || '') ? ' stamp' : ''), ev.text || '');
      break;
    case 'task.progress':
      if (ev.content) pushFeed('prog', ev.content);
      else if (ev.tool && ev.tool.kind === 'use') pushFeed('prog', '🔧 ' + ev.tool.name);
      break;
    case 'query.started':
      pushFeed('statusline', '▶ 开始执行');
      break;
    case 'query.completed':
      pushFeed('statusline', '✓ 本轮完成');
      break;
    case 'query.failed':
      pushFeed('err-msg', '✗ 失败：' + (ev.error || '未知错误'));
      break;
    case 'query.cancelled':
      pushFeed('statusline', '已取消');
      break;
    default:
      break; // unknown event types are ignored (forward compatible)
  }
}

// ── wire up ────────────────────────────────────────────────────────────────
$('wsUrl').value = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/';
$('btnConnect').addEventListener('click', connect);
$('btnPair').addEventListener('click', pair);
$('btnDispatch').addEventListener('click', function () {
  var t = $('taskText').value;
  $('taskText').value = '';
  dispatchText(t);
});
$('apOk').addEventListener('click', function () { dispatchText('y'); });
$('apNo').addEventListener('click', function () { dispatchText('n'); });
$('btnTasks').addEventListener('click', function () {
  var p = $('tasksPanel');
  var show = p.style.display === 'none';
  p.style.display = show ? 'block' : 'none';
  if (show) refreshTasks();
});
$('btnRefreshTasks').addEventListener('click', refreshTasks);
$('btnReset').addEventListener('click', function () {
  clearDevice();
  showPairIntro();
  pushFeed('statusline', '已解除本机配对（桌面端仍需撤销设备）');
});
if (!window.nacl) {
  $('connMsg').textContent = '客户端脚本加载异常，请刷新页面';
} else if (loadDevice() && loadDevice().deviceId) {
  showPaired();
  connect();
} else {
  connect();
}
</script>
</body>
</html>`;
