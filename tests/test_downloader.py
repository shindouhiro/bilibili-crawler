from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from crawler.downloader import (
    BilibiliDownloadError,
    download_video,
    find_ffmpeg_path,
    is_executable_ffmpeg,
    is_supported_bilibili_url,
    normalize_bilibili_url,
    search_videos,
)


def test_normalize_bilibili_url_accepts_bvid() -> None:
    assert normalize_bilibili_url("BV1xx411c7mD") == "https://www.bilibili.com/video/BV1xx411c7mD"


def test_normalize_bilibili_url_rejects_blank() -> None:
    with pytest.raises(ValueError, match="不能为空"):
        normalize_bilibili_url("  ")


def test_is_supported_bilibili_url_rejects_cheese() -> None:
    """付费课程 /cheese/ 链接应被识别为不支持。"""
    url = "https://www.bilibili.com/cheese/play/ss500371271?query_from=0"
    assert is_supported_bilibili_url(url) is False


def test_is_supported_bilibili_url_accepts_video() -> None:
    assert is_supported_bilibili_url("https://www.bilibili.com/video/BV1xx411c7mD") is True


def test_normalize_bilibili_url_rejects_cheese_url() -> None:
    """下载付费课程链接时应提前给出友好错误提示，而非 yt-dlp 内部异常。"""
    with pytest.raises(ValueError, match="不支持此类型"):
        normalize_bilibili_url("https://www.bilibili.com/cheese/play/ss500371271")


@patch("crawler.downloader._search_bilibili_api")
def test_search_videos_filters_unsupported_entries(mock_api: MagicMock) -> None:
    """搜索结果中的付费课程条目应被自动过滤。"""
    mock_api.return_value = [
        {
            "bvid": "BV123",
            "title": "普通视频",
            "arcurl": "https://www.bilibili.com/video/BV123",
            "author": "测试 UP",
            "pic": "//i0.hdslb.com/cover.jpg",
        },
        {
            "bvid": "",
            "title": "付费课程",
            "arcurl": "https://www.bilibili.com/cheese/play/ss500371271",
            "author": "课程作者",
        },
    ]

    results = search_videos("vibe coding", limit=10)

    assert len(results) == 1
    assert results[0].title == "普通视频"


@patch("crawler.downloader._search_bilibili_api")
def test_search_videos_maps_entries(mock_api: MagicMock) -> None:
    mock_api.return_value = [
        {
            "bvid": "BV123",
            "title": "测试视频",
            "author": "测试 UP",
            "duration": "1:30",
            "play": 99,
            "pic": "//i0.hdslb.com/cover.jpg",
            "arcurl": "https://www.bilibili.com/video/BV123",
        }
    ]

    results = search_videos("测试", limit=1)

    assert len(results) == 1
    assert results[0].id == "BV123"
    assert results[0].url == "https://www.bilibili.com/video/BV123"
    assert results[0].uploader == "测试 UP"
    assert results[0].duration == 90.0
    assert results[0].thumbnail == "https://i0.hdslb.com/cover.jpg"


@patch("crawler.downloader._search_bilibili_api")
def test_search_videos_allows_empty_result(mock_api: MagicMock) -> None:
    mock_api.return_value = []

    assert search_videos("没有结果") == []


@patch("crawler.downloader._search_bilibili_api")
def test_search_videos_passes_page_params(mock_api: MagicMock) -> None:
    mock_api.return_value = [
        {"bvid": "BV1", "title": "视频 1", "arcurl": "https://www.bilibili.com/video/BV1"},
        {"bvid": "BV2", "title": "视频 2", "arcurl": "https://www.bilibili.com/video/BV2"},
    ]

    results = search_videos("测试", limit=2, page=2)

    assert len(results) == 2
    mock_api.assert_called_once_with("测试", page=2, page_size=2)


@patch("crawler.downloader.YoutubeDL")
@patch("crawler.downloader.find_ffmpeg_path")
def test_download_video_returns_finished_filename(
    find_ffmpeg: MagicMock,
    youtube_dl: MagicMock,
    tmp_path: Path,
) -> None:
    find_ffmpeg.return_value = "/opt/homebrew/bin/ffmpeg"
    instance = youtube_dl.return_value.__enter__.return_value

    def fake_extract_info(_url: str, download: bool) -> dict:
        assert download is True
        hook = youtube_dl.call_args.args[0]["progress_hooks"][0]
        hook({"status": "finished", "filename": str(tmp_path / "video.f30280.m4a")})
        (tmp_path / "video.mp4").touch()
        return {"title": "video", "id": "BV123", "ext": "mp4"}

    instance.extract_info.side_effect = fake_extract_info
    instance.prepare_filename.return_value = str(tmp_path / "prepared.mp4")

    filename = download_video("BV123", output_dir=tmp_path)

    assert filename == str(tmp_path / "video.mp4")
    assert youtube_dl.call_args.args[0]["ffmpeg_location"] == "/opt/homebrew/bin"


@patch("crawler.downloader.YoutubeDL")
@patch("crawler.downloader.find_ffmpeg_path")
def test_download_video_prefers_requested_download_filepath(
    find_ffmpeg: MagicMock,
    youtube_dl: MagicMock,
    tmp_path: Path,
) -> None:
    find_ffmpeg.return_value = "/opt/homebrew/bin/ffmpeg"
    final_file = tmp_path / "merged.mp4"
    final_file.touch()
    instance = youtube_dl.return_value.__enter__.return_value
    instance.extract_info.return_value = {
        "id": "BV123",
        "requested_downloads": [{"filepath": str(final_file)}],
    }
    instance.prepare_filename.return_value = str(tmp_path / "fallback.m4a")

    filename = download_video("BV123", output_dir=tmp_path)

    assert filename == str(final_file)


@patch("crawler.downloader.find_ffmpeg_path")
def test_download_video_fails_when_ffmpeg_is_not_executable(
    find_ffmpeg: MagicMock,
    tmp_path: Path,
) -> None:
    find_ffmpeg.return_value = None

    with pytest.raises(BilibiliDownloadError, match="ffmpeg 不可用"):
        download_video("BV123", output_dir=tmp_path)


@patch("crawler.downloader.Path.is_file")
@patch("crawler.downloader.is_executable_ffmpeg")
@patch("crawler.downloader.which")
def test_find_ffmpeg_path_falls_back_to_homebrew(
    which_mock: MagicMock,
    is_executable: MagicMock,
    is_file: MagicMock,
) -> None:
    which_mock.return_value = None
    is_executable.return_value = True
    is_file.side_effect = [True]

    assert find_ffmpeg_path() == "/opt/homebrew/bin/ffmpeg"


@patch("crawler.downloader.subprocess.run")
def test_is_executable_ffmpeg_requires_successful_version_check(run: MagicMock) -> None:
    run.return_value.returncode = 1

    assert is_executable_ffmpeg("/opt/homebrew/bin/ffmpeg") is False
