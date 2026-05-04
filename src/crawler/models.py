from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field, field_validator


class SearchResult(BaseModel):
    id: str
    title: str
    url: str
    uploader: str | None = None
    duration: float | None = None
    view_count: int | None = None
    thumbnail: str | None = None


class SearchPage(BaseModel):
    items: list[SearchResult]
    page: int
    page_size: int
    has_more: bool


class DownloadRequest(BaseModel):
    url: str = Field(min_length=1)
    title: str | None = None
    format: str = "bestvideo+bestaudio/best"
    output_dir: Path = Path("downloads")

    @field_validator("url")
    @classmethod
    def validate_url(cls, value: str) -> str:
        clean_value = value.strip()
        if not clean_value:
            raise ValueError("视频地址不能为空")
        return clean_value


class DownloadTask(BaseModel):
    task_id: str
    status: Literal["queued", "running", "succeeded", "failed"]
    progress: float = 0
    filename: str | None = None
    error: str | None = None
