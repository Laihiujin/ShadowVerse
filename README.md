# ShadowVerse

[![Build](https://img.shields.io/github/actions/workflow/status/Laihiujin/ShadowVerse/main.yml?label=Build)](../../actions)
[![Release](https://img.shields.io/github/v/release/Laihiujin/ShadowVerse)](../../releases)
[![License](https://img.shields.io/badge/license-MIT-blue)](./LICENSE)

[ShadowVerse](https://github.com/Laihiujin/ShadowVerse) 是参考 [bili-shadowreplay](https://github.com/Xinrea/bili-shadowreplay) 迭代派生的子录播切片工具，在完整保留原版功能的基础上持续扩展。

原版是一款优秀的 B 站 / 抖音直播缓存切片投稿工具，支持实时回放、弹幕压制、封面编辑等完整流程。

## 支持平台

| 平台 | 流格式 | 弹幕 | 登录方式 | 访客模式 |
|---|---|---|---|---|
| **B 站** | HLS (fmp4/ts) | ✅ WebSocket | 扫码 / 手动 Cookie | ✅ |
| **抖音** | HLS | ✅ WebSocket | 扫码 / 手动 Cookie | ✅ |
| **虎牙** | HLS | — | 扫码 / 手动 Cookie | ✅ |
| **快手** | HLS / FLV / RTMP | ✅ WebSocket | 扫码 / 手动 Cookie | ✅ |
| **TikTok（海外）** | HLS / FLV | ✅ HTTP 轮询 | 内嵌 WebView / 手动 Cookie | ✅ |

**访客模式**：无需登录即可录制。各平台对未登录请求有反爬校验，本项目通过逆向工程绕过校验，并对访客 Cookie 实现自动刷新。

**登录模式**：应用内嵌 WebView 弹窗，直接扫码登录获取完整 Cookie，通常可解锁更高分辨率。

---

各平台的具体说明：

**B 站**：支持 4K 最高画质（需登录），同时拉取 AVC/HEVC 双编码，弹幕通过官方 WebSocket 协议实时接收。

**抖音**：通过 `sec_uid` 标识直播间，拉取 HLS origin 流，弹幕通过官方 WebSocket 协议实时接收。扫码登录流程涉及抖音护照系统，配置较复杂。

**虎牙**：通过解析直播页 JS 数据获取签名 HLS 流，暂未接入弹幕。

**快手**：优先 HLS，不可用时自动回退 FLV，FLV 失败再回退 RTMP。弹幕通过 WebSocket 接收，访客模式下自动生成并持久化 `did`（设备 ID）。

**TikTok（海外）**：需要代理访问，优先 HLS，FLV 作备选。弹幕通过 HTTP 轮询接口获取，支持按用户名 `@xxx` 或数字房间 ID 添加。

---

## 相对原版的其他改动

**迁移缓存并重载识别**
- 从旧目录迁移缓存到当前缓存路径,并重建录播记录；
- 支持将其他文件夹的直播间录播记录导入重载m3u8/ts视频片段，避免卸载软件出现直播记录消失；

**账号管理增强**

- 所有平台均支持应用内嵌 WebView 扫码登录，无需手动复制 Cookie；
- 登录账号与访客账号双轨并行，可同时开启；
- 访客cookie 自动刷新；
- 逆向功能二维码api登录及内置浏览器登录；

### 切片与投稿增强
- **B 站投稿开关**：可在设置中关闭 B 站投稿功能，仅保留本地切片

### 其他实用功能
- **重载直播间**：重新加载直播间，减少删除再重新添加的步骤，适用于直播间访问频繁
- **反向代理检测**：TikTok 录制前可检测当前代理是否可用
- **批量直播间链接添加直播间**：（同平台可批量添加直播间，单次最多20个）


## 安装

前往 [Releases](../../releases) 下载对应平台的安装包。

| 文件 | 说明 |
|---|---|
| `ShadowVerse_x.y.z_x64-setup.exe` | Windows |
| `ShadowVerse_x.y.z_aarch64.dmg` | macOS Apple Silicon |
| `ShadowVerse_x.y.z_x86_64.dmg` | macOS Intel |

其余功能（录制、实时回放、弹幕导出、封面编辑、B 站投稿等）与原版一致，详见 [bili-shadowreplay 文档](https://bsr.xinrea.cn/)。

## 致谢

本项目参考 [@Xinrea](https://github.com/Xinrea) 的 [bili-shadowreplay](https://github.com/Xinrea/bili-shadowreplay) 迭代，致敬原作者~
