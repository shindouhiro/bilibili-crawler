from __future__ import annotations

import logging
import re
import subprocess
from collections.abc import Callable
from pathlib import Path
from shutil import which
from typing import Any

import httpx
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


# yt-dlp BiliBili extractor 不支持的 URL 路径前缀
_UNSUPPORTED_BILIBILI_PATHS = (
    "/cheese/",   # 付费课程
    "/read/",     # 专栏文章
    "/audio/",    # 音频
    "/opus/",     # 动态长文
)


def is_supported_bilibili_url(url: str) -> bool:
    """判断 URL 是否是 yt-dlp BiliBili extractor 支持的视频链接。"""
    from urllib.parse import urlparse

    parsed = urlparse(url)
    host = parsed.hostname or ""
    if not host.endswith("bilibili.com"):
        return False
    for prefix in _UNSUPPORTED_BILIBILI_PATHS:
        if parsed.path.startswith(prefix):
            return False
    return True


def normalize_bilibili_url(url_or_bvid: str) -> str:
    value = url_or_bvid.strip()
    if not value:
        raise ValueError("视频地址不能为空")
    if value.startswith(("http://", "https://")):
        if not is_supported_bilibili_url(value):
            raise ValueError(
                f"不支持此类型的 Bilibili 链接：{value}\n"
                "目前仅支持普通视频（/video/）和番剧（/bangumi/），"
                "不支持付费课程（/cheese/）、专栏（/read/）等。"
            )
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

    try:
        items = _search_bilibili_api(query, page=safe_page, page_size=safe_limit)
    except Exception as exc:  # noqa: BLE001
        raise BilibiliDownloadError(f"Bilibili 搜索失败：{exc}") from exc

    return [
        _api_item_to_search_result(item)
        for item in items
        if _is_supported_api_item(item)
    ]


def _search_bilibili_api(
    keyword: str,
    page: int = 1,
    page_size: int = 10,
) -> list[dict[str, Any]]:
    """通过 Bilibili 官方搜索 API 获取视频列表（与 Rust 端保持一致）。"""
    params = {
        "search_type": "video",
        "keyword": keyword,
        "page": str(page),
        "page_size": str(page_size),
    }
    with httpx.Client(headers=BILIBILI_HTTP_HEADERS, timeout=15, follow_redirects=True) as client:
        # 预热 Cookie（获取 buvid 等必要 Cookie）
        client.get("https://www.bilibili.com")
        response = client.get(
            "https://api.bilibili.com/x/web-interface/search/type",
            params=params,
            headers={"Referer": "https://search.bilibili.com/"},
        )
        response.raise_for_status()
        data = response.json()

    if data.get("code") != 0:
        message = data.get("message") or "Bilibili 搜索 API 返回错误"
        raise BilibiliDownloadError(message)

    result_list = (data.get("data") or {}).get("result") or []
    return result_list


def _is_supported_api_item(item: dict[str, Any]) -> bool:
    """过滤搜索结果中不支持下载的 Bilibili 内容类型。"""
    url = item.get("arcurl") or ""
    if url and not is_supported_bilibili_url(url):
        logger.debug("跳过不支持的搜索结果：%s", url)
        return False
    return True


def _api_item_to_search_result(item: dict[str, Any]) -> SearchResult:
    """将 Bilibili 搜索 API 的原始数据转换为 SearchResult。"""
    bvid = str(item.get("bvid") or "")
    title = _clean_html(str(item.get("title") or "未命名视频"))
    url = item.get("arcurl") or ""
    if not url and bvid:
        url = f"https://www.bilibili.com/video/{bvid}"

    # 封面图可能以 // 开头，需要补全协议
    thumbnail = item.get("pic") or None
    if thumbnail and thumbnail.startswith("//"):
        thumbnail = f"https:{thumbnail}"

    # duration 格式为 "MM:SS" 或 "HH:MM:SS"，需要转换为秒
    duration = _parse_duration(item.get("duration"))

    return SearchResult(
        id=bvid or str(url),
        title=title,
        url=str(url),
        uploader=item.get("author"),
        duration=duration,
        view_count=item.get("play"),
        thumbnail=thumbnail,
    )


_HTML_TAG_RE = re.compile(r"<[^>]+>")


def _clean_html(value: str) -> str:
    """移除 Bilibili 搜索结果标题中的 HTML 高亮标签。"""
    cleaned = _HTML_TAG_RE.sub("", value)
    return (
        cleaned.replace("&quot;", '"')
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
    )


def _parse_duration(value: Any) -> float | None:
    """解析 'MM:SS' 或 'HH:MM:SS' 格式的时长为秒数。"""
    if value is None:
        return None
    if isinstance(value, (int, float)):
        return float(value)
    text = str(value).strip()
    if not text:
        return None
    parts = text.split(":")
    try:
        total = 0
        for part in parts:
            total = total * 60 + int(part)
        return float(total)
    except (ValueError, TypeError):
        return None


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


