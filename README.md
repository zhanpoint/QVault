<div align="center">

# QVault

**把 QQ 空间里的记忆，留在自己手边。**

用于归档本人 QQ 空间说说、互动与资源的本地桌面应用。

[![Build and release](https://github.com/zhanpoint/QVault/actions/workflows/release.yml/badge.svg)](https://github.com/zhanpoint/QVault/actions/workflows/release.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

[下载应用](https://github.com/zhanpoint/QVault/releases) · [构建记录](https://github.com/zhanpoint/QVault/actions/workflows/release.yml) · [反馈问题](https://github.com/zhanpoint/QVault/issues)

</div>

## 项目简介

QVault 使用 React、TypeScript 和 Tauri 2 构建界面，通过 Rust 请求 QQ 空间接口，将归档记录写入本地 SQLite。无需部署应用服务器。

项目正在开发中。可读取的内容取决于 QQ 空间接口、账号权限与登录状态；历史通知可能只保留部分内容，不能保证恢复所有已删除说说。

## 功能与数据来源

| 栏目 | 功能 | 数据来源 |
| --- | --- | --- |
| 归档 | 保存本人可见说说，随归档进度展示内容；支持暂停、继续和失败重试 | 本人说说接口 |
| 历史查找 | 从历史通知中查找更早的本人说说片段 | 空间历史通知，入口位于归档页“更多选项” |
| 联系人 | 查看归档中留下互动的联系人、互动记录与时间 | 本地归档记录，不是 QQ 好友列表 |
| 相册 | 分栏查看相册、照片、视频和相册回收站 | 对应 QQ 空间资源接口；回收站可能需要独立验证 |
| 留言板 | 查看当前留言及可识别的历史留言 | 留言板接口与归档中的历史通知 |
| 收藏 | 同步与浏览可读取的收藏内容 | QQ 空间收藏接口 |

归档记录保存在本地，按账号区分。图片与视频的加载、缓存与归档记录保存是不同阶段：**记录归档完成不等于所有媒体已经下载并可永久离线播放**。失效链接、已删除资源和权限限制仍可能导致媒体不可用。

## 下载与安装

在 [Releases](https://github.com/zhanpoint/QVault/releases) 选择版本，根据系统下载附件：

| 系统 | 架构 | 安装包 |
| --- | --- | --- |
| Windows | x64 | `.exe`（推荐）或 `.msi` |
| macOS | Apple Silicon / ARM64 | 文件名包含 `aarch64` 的 `.dmg` |
| macOS | Intel / x64 | 文件名包含 `x64` 的 `.dmg` |
| Linux | x64 | `.deb` 或 `.AppImage` |

- Windows 需要 WebView2 Runtime；安装器可在缺少时联网安装。
- Linux 包在 Ubuntu 22.04 环境构建。DEB 适用于具有相应依赖的 Debian / Ubuntu 系统；AppImage 也需要兼容的系统运行环境。
- 当前工作流未配置 Windows 代码签名、Apple Developer ID 签名和公证，系统可能阻止或提示验证开发者。仅安装可信仓库的构建，不建议关闭系统安全功能。
- 每个正式发布附带 `SHA256SUMS.txt`，可用于核对下载文件是否完整；校验文件不能替代代码签名。

没有发布版本时，可登录 GitHub，在成功的 [Actions 运行](https://github.com/zhanpoint/QVault/actions/workflows/release.yml)底部下载 `QVault-<平台>` 构建产物并解压。日常构建保留 14 天，不代表经过真实账号与跨平台人工验收的稳定版。

## 开始使用

1. 安装并打开 QVault，扫码登录本人的 QQ 空间。
2. 进入“归档”，点击“开始归档”，已保存的内容会随进度展示。
3. 需要查找更早的内容时，打开“更多选项 → 查找更早的说说”。该过程依赖历史通知，可能较慢且内容不完整。
4. 在相册、留言板、收藏页面分别读取所需资源；联系人页面展示已有归档中的互动信息。

登录过期时重新扫码，再继续归档。关闭主窗口会隐藏到系统托盘；需要退出程序时使用托盘菜单“退出应用”。

请仅归档本人或已获授权的内容。QVault 是独立开源项目，与腾讯或 QQ 空间无隶属关系。

## 本地数据

主数据库为应用数据目录中的 `qzone-archive.sqlite3`，已保存图片位于其中的 `images/` 子目录；临时媒体位于系统应用缓存目录。具体位置由 Tauri 按系统解析，应用标识为 `top.ehre.QVault`：

| 系统 | 默认应用数据目录 |
| --- | --- |
| Windows | `%APPDATA%\top.ehre.QVault` |
| macOS | `~/Library/Application Support/top.ehre.QVault` |
| Linux | `$XDG_DATA_HOME/top.ehre.QVault`，通常为 `~/.local/share/top.ehre.QVault` |

当前版本不提供完整数据迁移、加密导出或云端同步功能。反馈问题时不要上传数据库、Cookie、登录二维码或他人私人内容。

## 本地开发

### 环境要求

- Node.js 22 LTS 与 npm；CI 使用 Node.js 22。
- Rust stable 与 Cargo。
- 对应系统的 [Tauri 2 开发环境](https://v2.tauri.app/start/prerequisites/)：Windows 的 C++ 构建工具及 WebView2、macOS 的 Xcode Command Line Tools，或 Linux 的 WebKitGTK 等依赖。

```powershell
git clone https://github.com/zhanpoint/QVault.git
cd QVault
npm ci
```

### 网页样式开发

```powershell
npm run web:dev -- --open
```

此命令启动 Vite 并打开浏览器，支持热更新。`--open` 仅负责打开网页。**浏览器模式没有 Rust 后端，不能执行真实扫码登录、数据库访问和归档任务。**

归档界面的离线演示位于 `http://localhost:1420/tests/archive-preview.html`，深色模式使用 `?theme=dark`。演示数据不代表实际账号内容。

### 完整业务开发

```powershell
npm run tauri dev
```

Tauri 会自动启动 Vite 和桌面窗口；前端热更新，Rust 代码修改后重新编译。不要同时启动另一套占用 1420 端口的 Vite 服务。

### 检查与构建

```powershell
npm run release:check
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
npm run check
npm test
npm run build
```

`check` 执行 TypeScript 检查和 Rust Clippy；`test` 执行 Rust 库测试，包括当前标记为 ignored 的测试；`build` 生成前端静态资源，**不会生成桌面安装包**。

在对应系统上生成安装包：

```powershell
# Windows
npm run tauri:build:windows:all
```

```bash
# macOS
npm run tauri -- build --bundles dmg

# Linux
npm run tauri -- build --bundles deb,appimage
```

默认产物位于 `src-tauri/target/release/bundle/`；指定 `--target` 时位于 `src-tauri/target/<target>/release/bundle/`。跨平台安装包由 CI 的对应系统分别构建。

## CI 与自动发布

工作流文件：[`.github/workflows/release.yml`](.github/workflows/release.yml)。发布目标是 GitHub Releases，不需要另行部署 Web 服务器，也未配置应用内自动更新。

| 触发方式 | 行为 | 下载位置 |
| --- | --- | --- |
| 推送任意分支 / Pull Request | 版本一致性、格式、类型、Clippy、测试及前端构建检查 | 检查日志 |
| 推送 `main` | 检查通过后，并行构建四种桌面安装包 | Actions → 对应运行 → Artifacts |
| 推送 `v<版本号>` 标签 | 检查、四平台构建、生成 SHA-256 校验文件，全部成功后发布 | Releases |
| Actions 手动运行 | 对所选分支构建桌面安装包；选择版本标签时执行版本发布 | Artifacts / Releases |

工作流使用锁文件安装依赖、缓存 Cargo 与 npm 下载，并固定第三方 Action 的提交版本。只有最终发布任务拥有仓库写权限。平台构建失败时不会公开不完整的新 Release；上传失败时保留草稿以便重试。已公开版本不可通过重跑替换，请发布新版本。

### 首次启用

仓库需要启用 GitHub Actions，并允许工作流中声明的 `contents: write` 权限。使用 GitHub 自动提供的 `GITHUB_TOKEN`，无需添加个人访问令牌。组织策略若禁用 Actions、限制所用 Action 或禁止写权限，需要由仓库管理员调整。

将代码推送到 `main` 后即可自动构建：

```powershell
git push origin main
```

### 发布版本

以下位置的版本必须一致：

- `package.json` 与 `package-lock.json`。
- `src-tauri/tauri.conf.json`。
- `src-tauri/Cargo.toml` 与 `src-tauri/Cargo.lock` 中的 QVault 包版本。

例如下一版本为 `1.0.4`：先执行 `npm version 1.0.4 --no-git-tag-version`，再将 Tauri 配置和 Cargo 清单中的版本同步为 `1.0.4`，执行 `cargo check --manifest-path src-tauri/Cargo.toml` 更新 Cargo 锁文件，检查并提交版本变更后执行：

```powershell
npm run release:check
git push origin main
git tag v1.0.4
git push origin v1.0.4
```

标签必须与应用版本完全匹配，否则 CI 会在打包前终止。`v1.0.4-beta.1` 这类带预发布后缀的版本会发布为 GitHub Pre-release；普通版本发布为正式 Release。推送标签不会自动修改应用版本。

## 项目结构

```text
src/
  components/          共享界面组件
  features/            归档、联系人、相册与资源页面
  styles/              样式与主题
src-tauri/
  src/                 登录、QQ 空间请求、归档任务与 SQLite 存储
  icons/               桌面图标与源图
  tauri.conf.json       桌面窗口及打包配置
scripts/               发布版本校验
tests/                 浏览器归档预览
.github/workflows/     自动检查、打包与发布
```

## 反馈与贡献

欢迎提交 [Issue](https://github.com/zhanpoint/QVault/issues) 或 Pull Request。问题报告请包含应用版本、系统及架构、复现步骤、预期结果和脱敏错误信息；接口相关问题请说明重新登录后是否仍能复现。

提交代码前执行上述检查命令，涉及数据解析、断点或请求逻辑的修改请补充相应回归测试。构建成功不等于官方接口实时可用；账号权限、媒体访问与跨平台安装体验需要单独验证。

## 许可证

本项目遵循 [GNU General Public License v3.0](LICENSE)。分发或修改时请遵循许可证要求。
