from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from threading import Lock
from urllib.parse import urlparse
from uuid import uuid4

import httpx
from fastapi import BackgroundTasks, FastAPI, HTTPException, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import Response

from crawler.downloader import (
    BILIBILI_HTTP_HEADERS,
    BilibiliDownloadError,
    download_video,
    find_ffmpeg_path,
    search_videos,
)
from crawler.models import DownloadRequest, DownloadTask, SearchPage

app = FastAPI(
    title="Bilibili 视频匹配下载工具",
    description="通过关键词匹配 Bilibili 视频，并下载公开视频。",
    version="0.1.0",
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:5173", "http://127.0.0.1:5173"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

_tasks: dict[str, DownloadTask] = {}
_tasks_lock = Lock()
_executor = ThreadPoolExecutor(max_workers=2)


@app.get("/api/health")
def health_check() -> dict[str, str]:
    return {"status": "ok"}


@app.get("/api/system")
def system_status() -> dict[str, str | bool | None]:
    ffmpeg_path = find_ffmpeg_path()
    return {
        "status": "ok",
        "ffmpeg_available": ffmpeg_path is not None,
        "ffmpeg_path": ffmpeg_path,
    }


@app.get("/api/search", response_model=SearchPage)
def search_endpoint(
    q: str = Query(min_length=1),
    page: int = Query(default=1, ge=1, le=20),
    page_size: int = Query(default=10, ge=1, le=30),
) -> SearchPage:
    try:
        items = search_videos(q, limit=page_size, page=page)
        return SearchPage(
            items=items,
            page=page,
            page_size=page_size,
            has_more=len(items) == page_size,
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except BilibiliDownloadError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


@app.get("/api/image")
async def proxy_image(url: str = Query(min_length=1)) -> Response:
    parsed_url = urlparse(url)
    if parsed_url.scheme not in {"http", "https"}:
        raise HTTPException(status_code=400, detail="图片地址协议不支持")
    if not parsed_url.hostname or not parsed_url.hostname.endswith(("hdslb.com", "biliimg.com")):
        raise HTTPException(status_code=400, detail="只允许代理 Bilibili 图片")

    try:
        async with httpx.AsyncClient(timeout=12, follow_redirects=True) as client:
            image_response = await client.get(url, headers=BILIBILI_HTTP_HEADERS)
            image_response.raise_for_status()
    except httpx.HTTPError as exc:
        raise HTTPException(status_code=502, detail=f"封面图片加载失败：{exc}") from exc

    content_type = image_response.headers.get("content-type", "image/jpeg")
    if not content_type.startswith("image/"):
        raise HTTPException(status_code=415, detail="目标地址不是图片")

    return Response(
        content=image_response.content,
        media_type=content_type,
        headers={"Cache-Control": "public, max-age=86400"},
    )


@app.post("/api/download", response_model=DownloadTask, status_code=202)
def create_download_task(
    request: DownloadRequest,
    background_tasks: BackgroundTasks,
) -> DownloadTask:
    task = DownloadTask(task_id=uuid4().hex, status="queued", progress=0)
    _save_task(task)
    background_tasks.add_task(_executor.submit, _run_download_task, task.task_id, request)
    return task


@app.get("/api/downloads/{task_id}", response_model=DownloadTask)
def get_download_task(task_id: str) -> DownloadTask:
    task = _get_task(task_id)
    if not task:
        raise HTTPException(status_code=404, detail="下载任务不存在")
    return task


def _run_download_task(task_id: str, request: DownloadRequest) -> None:
    _update_task(task_id, status="running", progress=1)

    def update_progress(progress: dict) -> None:
        if progress.get("status") == "downloading":
            percent = _percent_from_progress(progress)
            _update_task(task_id, status="running", progress=percent)
        elif progress.get("status") == "finished":
            _update_task(task_id, status="running", progress=95, filename=progress.get("filename"))

    try:
        filename = download_video(
            request.url,
            output_dir=request.output_dir,
            format_selector=request.format,
            progress_hook=update_progress,
        )
        _update_task(task_id, status="succeeded", progress=100, filename=filename, error=None)
    except Exception as exc:  # noqa: BLE001
        _update_task(task_id, status="failed", error=str(exc))


def _percent_from_progress(progress: dict) -> float:
    total = progress.get("total_bytes") or progress.get("total_bytes_estimate")
    downloaded = progress.get("downloaded_bytes") or 0
    if not total:
        return 5
    return round(min(94, max(1, downloaded / total * 94)), 2)


def _save_task(task: DownloadTask) -> None:
    with _tasks_lock:
        _tasks[task.task_id] = task


def _get_task(task_id: str) -> DownloadTask | None:
    with _tasks_lock:
        return _tasks.get(task_id)


def _update_task(task_id: str, **changes: object) -> None:
    with _tasks_lock:
        current = _tasks[task_id]
        _tasks[task_id] = current.model_copy(update=changes)
