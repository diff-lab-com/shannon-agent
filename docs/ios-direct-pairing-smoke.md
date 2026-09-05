# iOS 真机直连配对冒烟检查单(A8b)

> 背景:cross-repo-adaptation-spec §A8b —— iOS 的 ATS `NSAllowsLocalNetworking`
> 不覆盖裸 IP,iOS 真机在系统层直接拒绝 `ws://192.168.x.x:port`。因此桌面侧
> 必须以 `<hostname>.local` mDNS 主机名形态提供 endpoint,并广播 `_shannon._tcp`。
> 本检查单对应 shannon-mono 侧的三项落地:gateway mDNS 广播(`gateway/src/mobile/mdns.ts`)、
> QR host 改 `.local`(`desktop/src/commands_mobile_pairing.rs::mdns_hostname`)、
> 默认绑定 `0.0.0.0`(`default_mobile_config`)。

## 前置条件

1. **桌面绑定非 loopback**:desktop 写入的默认 `mobile.host` 现为 `0.0.0.0`。
   若既有配置文件 `~/.shannon/gateway/config.json` 的 mobile 块还写着
   `127.0.0.1`,需要改掉或删除该字段(旧默认会让 gateway 跳过 mDNS 广播并打 warn)。
2. **mDNS responder 就绪**:
   - macOS:系统内建,无需操作;
   - Linux:`avahi-daemon` 运行中(`systemctl is-active avahi-daemon`)。
     桌面主机名须可解析:另一台机器上 `getent hosts <hostname>.local` 应返回地址。
3. **`_shannon._tcp` 广播可见**(gateway 启动后):
   - Linux:`avahi-browse -a | grep shannon` 或 `avahi-browse _shannon._tcp`;
   - macOS:`dns-sd -B _shannon._tcp`;
   - TXT 记录应含 `instanceId`、`version`、`port`(= 配对 WSS 端口,默认 33430)。
4. 手机与桌面同一局域网;UDP 5353(mDNS)与 TCP 33430(配对 WS)未被防火墙拦截。

## 步骤

1. 启动 desktop(gateway supervisor 自动拉起 gateway),确认 gateway 日志出现
   `mobile mDNS: advertising _shannon._tcp.local`(且**没有** loopback warn)。
2. desktop 触发 `mobile_generate_pair_token` 出二维码。
3. iOS 真机打开 app 扫码。**预期:不触发 `rawIpOnIos` 预检错误**(QR host 已是
   `<hostname>.local`,通过了 `isRawIpv4Endpoint` 预检)。
4. 配对完成:app 显示配对成功;desktop `mobile_list_paired_devices` 出现该设备。
5. 连通性验证:app 发起一次 resume/query,走 `ws://<hostname>.local:33430` 正常往返。

## 判定

- **通过**:步骤 3-5 全部满足(不出现裸 IP 预检错误、无 "unreachable" 类 OS 层拒绝)。
- **失败排查**:
  - 仍报 `rawIpOnIos` → QR payload 的 `host` 不是 `.local` 形态,检查 desktop 是否
    用了含本次改动的构建、`mdns_hostname()` 是否拿到 OS 主机名;
  - 主机名解析失败/连接超时(非预检错误)→ 手机与桌面不在同一网段、mDNS responder
    未运行、或 UDP 5353 被拦;先用手机浏览器/其他工具确认 `<hostname>.local` 可解析;
  - 解析成功但 WS 连不上 → TCP 33430 防火墙,或 gateway 实际绑定非预期接口。

## 备注

- **Android 注意**:Android 对 `.local` 主机名的系统级解析不保证(部分版本
  getaddrinfo 不做 mDNS 多播解析)。若 Android 直连回归,需要 mobile 侧引入
  NsdManager/自定义解析回退 —— 属 shannon-mobile 范畴的后续项。
- **安全提示**:默认绑定从 `127.0.0.1` 放宽到 `0.0.0.0` 会把配对 WS 暴露到 LAN。
  访问仍受一次性 pair token(75s TTL)门禁;设备注册/审批另有 Ed25519 签名校验。
- relay 轨(§A8b 所述"不受影响")不依赖本链路,QR v2 的 `relayEndpoint` 为准。
