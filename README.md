# Bilibili 视频匹配下载工具

一个基于 Tauri + Rust 的 Bilibili 视频搜索与下载桌面应用。通过 Bilibili 官方 API 搜索视频，支持流式下载、下载历史管理。

## 技术栈

- **后端**：Rust（Tauri v2 + reqwest + rusqlite）
- **前端**：React 19 + TypeScript + Tailwind CSS
- **桌面打包**：Tauri（macOS / Windows / Linux）

## 快速开始

```bash
pnpm install
```

启动桌面开发模式：

```bash
pnpm dev
```

构建桌面应用：

```bash
pnpm build:desktop
```

构建 Android 应用：

```bash
pnpm build:android
```

## 范围说明

- 仅支持公开视频或当前网络环境匿名可访问的视频。
- 不支持登录绕过、会员内容绕过、付费内容绕过或 DRM 绕过。
- 下载文件默认保存到系统下载目录下的 `BilibiliCrawler/` 文件夹。
