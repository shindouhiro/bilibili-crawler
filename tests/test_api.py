from unittest.mock import patch

from fastapi.testclient import TestClient

from crawler.api import app
from crawler.models import SearchResult

client = TestClient(app)


def test_health_check() -> None:
    response = client.get("/api/health")

    assert response.status_code == 200
    assert response.json() == {"status": "ok"}


@patch("crawler.api.find_ffmpeg_path")
def test_system_status_reports_ffmpeg(mock_find_ffmpeg) -> None:
    mock_find_ffmpeg.return_value = "/opt/homebrew/bin/ffmpeg"

    response = client.get("/api/system")

    assert response.status_code == 200
    assert response.json()["ffmpeg_available"] is True
    assert response.json()["ffmpeg_path"] == "/opt/homebrew/bin/ffmpeg"


@patch("crawler.api.search_videos")
def test_search_endpoint(mock_search) -> None:
    mock_search.return_value = [
        SearchResult(id="BV123", title="测试视频", url="https://www.bilibili.com/video/BV123")
    ]

    response = client.get("/api/search", params={"q": "测试"})

    assert response.status_code == 200
    payload = response.json()
    assert payload["items"][0]["id"] == "BV123"
    assert payload["page"] == 1
    assert payload["page_size"] == 10
    assert payload["has_more"] is False


@patch("crawler.api.search_videos")
def test_search_endpoint_accepts_page_params(mock_search) -> None:
    mock_search.return_value = [
        SearchResult(id=f"BV{index}", title=f"测试视频 {index}", url=f"https://example.com/{index}")
        for index in range(3)
    ]

    response = client.get("/api/search", params={"q": "测试", "page": 2, "page_size": 3})

    assert response.status_code == 200
    assert response.json()["page"] == 2
    assert response.json()["page_size"] == 3
    assert response.json()["has_more"] is True
    mock_search.assert_called_once_with("测试", limit=3, page=2)


def test_download_task_lifecycle() -> None:
    with patch("crawler.api._executor.submit") as submit:
        response = client.post("/api/download", json={"url": "BV123"})

    assert response.status_code == 202
    task = response.json()
    assert task["status"] == "queued"
    assert task["progress"] == 0
    submit.assert_called_once()

    detail = client.get(f"/api/downloads/{task['task_id']}")
    assert detail.status_code == 200
    assert detail.json()["task_id"] == task["task_id"]


def test_proxy_image_rejects_non_bilibili_host() -> None:
    response = client.get("/api/image", params={"url": "https://example.com/cover.jpg"})

    assert response.status_code == 400
