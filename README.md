# Bilibili 视频匹配下载工具

一个基于 `yt-dlp` 的 Bilibili 视频搜索与下载工具。支持命令行和网页界面，默认只下载匿名访问下可用的公开视频。

## 快速开始

```bash
uv sync
pnpm install
```

命令行搜索：

```bash
uv run bilicrawl search "关键词"
```

命令行下载：

```bash
uv run bilicrawl download "https://www.bilibili.com/video/BVxxxx"
```

同时启动后端 API 和前端：

```bash
pnpm dev
```

兼容旧习惯，下面这个命令也会同时启动前后端并自动清理端口：

```bash
pnpm dev:web
```

如果端口被旧进程占用，也可以先手动停止：

```bash
pnpm dev:stop
```

如果只想单独启动某一侧：

```bash
pnpm dev:api
pnpm dev:web-only
```

## 范围说明

- 仅支持公开视频或当前网络环境匿名可访问的视频。
- 不支持登录绕过、会员内容绕过、付费内容绕过或 DRM 绕过。
- 下载文件默认保存到 `downloads/`。
