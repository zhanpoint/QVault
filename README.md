<div align="center">

<img src="public/QVault.svg" alt="QVault 图标" width="88" />

# QVault

**把 QQ 空间里的记忆，留在自己手边。**

归档本人 QQ 空间说说与互动的本地桌面应用。

[![Build and release](https://github.com/zhanpoint/QVault/actions/workflows/release.yml/badge.svg)](https://github.com/zhanpoint/QVault/actions/workflows/release.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/平台-Windows%20%7C%20macOS%20%7C%20Linux-4f7df3)](https://github.com/zhanpoint/QVault/releases)

[下载应用](https://github.com/zhanpoint/QVault/releases) · [反馈问题](https://github.com/zhanpoint/QVault/issues) · [构建记录](https://github.com/zhanpoint/QVault/actions/workflows/release.yml)

</div>

## ✨ 功能

- **说说归档**：保存当前可见的本人说说，再从历史通知中查找更早的内容；支持暂停、续传和失败重试。
- **联系人**：查看归档中留下互动的联系人及互动记录，并非 QQ 好友列表。
- **相册**：浏览相册、照片、视频和相册回收站；回收站可能需要独立验证。
- **留言与收藏**：查看留言及可识别的历史留言，浏览 QQ 空间收藏。

可读取的内容取决于接口、账号权限和登录状态。历史通知可能不完整，不能保证恢复已删除的说说。收藏仅包括 QQ 空间收藏，不包括手机 QQ 的通用收藏。

## 📥 安装

前往 [Releases](https://github.com/zhanpoint/QVault/releases) 下载对应安装包：

| 系统 | 安装包 |
| --- | --- |
| Windows x64 | `.exe` 或 `.msi` |
| macOS Apple Silicon / Intel | 对应架构的 `.dmg` |
| Linux x64 | `.deb` 或 `.AppImage` |

安装包由 GitHub Actions 构建，可在 Releases 页面下载。

## 🚀 使用

1. 打开应用，扫码登录本人的 QQ 空间。
2. 进入“归档”，点击“开始归档”。当前说说同步后会自动查找历史通知；历史扫描可能耗时较长，可随时暂停。
3. 在相册、留言板和收藏页面浏览相应内容。登录过期时重新扫码即可继续。

关闭主窗口会隐藏到系统托盘，可通过托盘菜单退出。请仅归档本人或已获授权的内容。

## 🔒 数据与隐私

归档记录按账号保存在本地，已保存的图片也存放在设备上。

归档记录不代表所有媒体已下载；失效或受权限限制的内容可能无法访问。反馈问题时请注意保护个人隐私。

## 🛠️ 开发

项目基于 React、TypeScript、Tauri 2、Rust 和 SQLite。需要 Node.js 22、Rust stable，以及对应系统的 [Tauri 开发依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
git clone https://github.com/zhanpoint/QVault.git
cd QVault
npm ci
npm run taauri dev
```

只开发网页样式时可运行 `npm run web:dev`；浏览器模式没有 Rust 后端，无法执行真实登录或归档。

```bash
npm run release:check  # 检查版本一致性
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
npm run check          # TypeScript 与 Rust Clippy
npm test               # Rust 库测试，包含 ignored 测试
npm run build          # 前端构建，不生成桌面安装包
```

生成安装包：Windows 运行 `npm run tauri:build:windows:all`，macOS 运行 `npm run tauri -- build --bundles dmg`，Linux 运行 `npm run tauri -- build --bundles deb,appimage`。默认产物位于 `src-tauri/target/release/bundle/`。

<details>
<summary>发布维护</summary>

CI 工作流见 [release.yml](.github/workflows/release.yml)。推送 `main` 后构建各平台产物；推送与应用版本一致的 `v<版本号>` 标签后发布到 GitHub Releases。发布前需同步 `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 和 `src-tauri/Cargo.lock` 中的版本，并运行 `npm run release:check`。

</details>

## 💬 一切的起点

### 把散落在时间里的空间记忆，重新放回手边。

一切开始于一个平常的午后。整理旧照片时，我们忽然想起那些曾经认真写下、后来又随手删掉的动态。那时总觉得日子还长，眼前的心情不值一提，等到多年以后再回头，才发现那已经是再也无法复刻的一天。

我们删掉过青涩的文字，也删掉过一张并不完美的照片。可真正消失的，从来不只是几行内容：还有朋友当年的一句玩笑、深夜里收到的安慰，以及那个愿意坦率表达、尚未学会隐藏情绪的自己。

后来才明白，记忆的价值并不取决于它是否体面。那些看似普通甚至有些笨拙的记录，恰恰标记着我们如何长大、如何爱过，又如何与一些重要的人渐渐走散。时间会带走很多东西，而被认真保存下来的片段，会替我们记住相遇真实发生过。

QVault 因此诞生。它不替回忆下定义，只希望把属于你的动态、照片与互动安静地留在手边。或许某一天再次翻开时，你会看见的不只是过去，也是在漫长岁月里一步步成为今天的自己。

> 归档不是告别，而是为重要的记忆留下一条回去的路。

## 🤝 反馈与许可证

欢迎提交 [Issue](https://github.com/zhanpoint/QVault/issues) 或 Pull Request。报告问题时请提供应用版本、系统、复现步骤和脱敏后的错误信息。

QVault 是独立开源项目，与腾讯或 QQ 空间无隶属关系，采用 [GNU GPL v3.0](LICENSE) 许可证。
