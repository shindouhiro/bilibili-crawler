from __future__ import annotations

import logging
import subprocess
from collections.abc import Callable
from pathlib import Path
from shutil import which
from typing import Any

from yt_dlp import YoutubeDL

from crawler.models import SearchResult

logger = logging.getLogger(__name__)

ProgressHook = Callable[[dict[str, Any]], None]

BILIBILI_HTTP_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
        "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36"
    ),
    "Referer": "https://search.bilibili.com/",
}

# 尝试使用浏览器 Cookie 的优先级列表
_BROWSER_COOKIE_CANDIDATES = ("chrome", "edge", "firefox", "safari")


def _get_cookie_options() -> dict[str, Any]:
    """尝试从本地浏览器获取 Cookie 配置，解决 Bilibili 412 风控问题。

    按优先级依次尝试 Chrome → Edge → Firefox → Safari。
    如果项目根目录下存在 cookies.txt 则优先使用该文件。
    """
    cookies_file = Path("cookies.txt")
    if cookies_file.is_file():
        logger.info("使用 cookies.txt 文件作为 Cookie 来源")
        return {"cookiefile": str(cookies_file)}

    for browser in _BROWSER_COOKIE_CANDIDATES:
        try:
            # 快速检测该浏览器是否可用（yt-dlp 内部会处理）
            opts: dict[str, Any] = {"cookiesfrombrowser": (browser,)}
            logger.info("使用 %s 浏览器 Cookie", browser)
            return opts
        except Exception:  # noqa: BLE001
            continue

    logger.warning("未找到可用的浏览器 Cookie，请求可能因 412 风控而失败")
    return {}

FFMPEG_CANDIDATE_PATHS = (
    "/opt/homebrew/bin/ffmpeg",
    "/usr/local/bin/ffmpeg",
    "/usr/bin/ffmpeg",
)


class BilibiliDownloadError(RuntimeError):
    """下载或解析失败时抛出的业务异常。"""


def normalize_bilibili_url(url_or_bvid: str) -> str:
    value = url_or_bvid.strip()
    if not value:
        raise ValueError("视频地址不能为空")
    if value.startswith(("http://", "https://")):
        return value
    if value.upper().startswith("BV"):
        return f"https://www.bilibili.com/video/{value}"
    raise ValueError("请输入 Bilibili 视频链接或 BV 号")


def search_videos(keyword: str, limit: int = 10, page: int = 1) -> list[SearchResult]:
    query = keyword.strip()
    if not query:
        raise ValueError("搜索关键词不能为空")

    safe_limit = max(1, min(limit, 30))
    safe_page = max(1, min(page, 20))
    fetch_limit = safe_limit * safe_page
    options: dict[str, Any] = {
        "quiet": True,
        "no_warnings": True,
        "http_headers": BILIBILI_HTTP_HEADERS,
        "skip_download": True,
        **_get_cookie_options(),
    }
    try:
        with YoutubeDL(options) as ydl:
            info = ydl.extract_info(f"bilisearch{fetch_limit}:{query}", download=False)
    except Exception as exc:  # noqa: BLE001
        raise BilibiliDownloadError(f"Bilibili 搜索失败：{exc}") from exc

    entries = (info or {}).get("entries") or []
    start = (safe_page - 1) * safe_limit
    end = start + safe_limit
    return [_to_search_result(entry) for entry in entries[start:end] if entry]


def download_video(
    url_or_bvid: str,
    output_dir: str | Path = "downloads",
    format_selector: str = "bestvideo+bestaudio/best",
    progress_hook: ProgressHook | None = None,
) -> str:
    url = normalize_bilibili_url(url_or_bvid)
    target_dir = Path(output_dir)
    target_dir.mkdir(parents=True, exist_ok=True)

    options: dict[str, Any] = {
        "format": format_selector,
        "outtmpl": {"default": "%(title).150B-%(id)s.%(ext)s"},
        "paths": {"home": str(target_dir)},
        "merge_output_format": "mp4",
        "windowsfilenames": True,
        "trim_file_name": 180,
        "noplaylist": True,
        "quiet": True,
        "no_warnings": True,
        "http_headers": BILIBILI_HTTP_HEADERS,
        "progress_hooks": [progress_hook] if progress_hook else [],
        **_get_cookie_options(),
    }
    ffmpeg_path = find_ffmpeg_path()
    if not ffmpeg_path:
        raise BilibiliDownloadError(
            "ffmpeg 不可用，无法合并 Bilibili 的音视频分离流。"
            "请执行 `brew reinstall ffmpeg` 修复后重试。"
        )
    options["ffmpeg_location"] = str(Path(ffmpeg_path).parent)
    downloaded_fragment_paths: list[str] = []

    def remember_finished(progress: dict[str, Any]) -> None:
        if progress_hook:
            progress_hook(progress)
        if progress.get("status") == "finished":
            filename = progress.get("filename")
            if filename:
                downloaded_fragment_paths.append(str(filename))

    options["progress_hooks"] = [remember_finished]

    try:
        with YoutubeDL(options) as ydl:
            info = ydl.extract_info(url, download=True)
            final_filename = _resolve_downloaded_filename(
                ydl,
                info,
                target_dir,
                downloaded_fragment_paths,
            )
    except Exception as exc:  # noqa: BLE001
        raise BilibiliDownloadError(f"Bilibili 下载失败：{exc}") from exc

    return final_filename or str(target_dir)


def find_ffmpeg_path() -> str | None:
    resolved_path = which("ffmpeg")
    if resolved_path and is_executable_ffmpeg(resolved_path):
        return resolved_path

    for candidate_path in FFMPEG_CANDIDATE_PATHS:
        if Path(candidate_path).is_file() and is_executable_ffmpeg(candidate_path):
            return candidate_path

    return None


def is_executable_ffmpeg(path: str) -> bool:
    try:
        result = subprocess.run(
            [path, "-version"],
            capture_output=True,
            check=False,
            timeout=5,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return result.returncode == 0


def _resolve_downloaded_filename(
    ydl: YoutubeDL,
    info: dict[str, Any] | None,
    output_dir: Path,
    downloaded_fragment_paths: list[str],
) -> str | None:
    if not info:
        return None

    requested_downloads = info.get("requested_downloads") or []
    for requested_download in requested_downloads:
        filepath = requested_download.get("filepath")
        if filepath and Path(filepath).exists():
            return str(filepath)

    prepared_filename = ydl.prepare_filename(info)
    merged_filename = str(Path(prepared_filename).with_suffix(".mp4"))
    if Path(merged_filename).exists():
        return merged_filename
    if Path(prepared_filename).exists():
        return prepared_filename

    for fragment_path in downloaded_fragment_paths:
        merged_fragment_path = _merged_path_from_fragment(Path(fragment_path))
        if merged_fragment_path.exists():
            return str(merged_fragment_path)

    video_id = str(info.get("id") or "")
    candidates = sorted(output_dir.glob(f"*{video_id}*.mp4"), key=lambda path: path.stat().st_mtime)
    if candidates:
        return str(candidates[-1])

    return prepared_filename


def _merged_path_from_fragment(fragment_path: Path) -> Path:
    stem = fragment_path.stem
    if ".f" in stem:
        stem = stem.rsplit(".f", 1)[0]
    return fragment_path.with_name(f"{stem}.mp4")


def _to_search_result(entry: dict[str, Any]) -> SearchResult:
    video_id = str(entry.get("id") or entry.get("display_id") or "")
    url = entry.get("webpage_url") or entry.get("url") or ""
    if video_id.upper().startswith("BV"):
        url = f"https://www.bilibili.com/video/{video_id}"

    return SearchResult(
        id=video_id or str(url),
        title=str(entry.get("title") or "未命名视频"),
        url=str(url),
        uploader=entry.get("uploader") or entry.get("channel"),
        duration=entry.get("duration"),
        view_count=entry.get("view_count"),
        thumbnail=entry.get("thumbnail"),
    )
