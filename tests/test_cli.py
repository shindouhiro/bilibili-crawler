from unittest.mock import patch

from typer.testing import CliRunner

from crawler.cli import app
from crawler.models import SearchResult

runner = CliRunner()


@patch("crawler.cli.search_videos")
def test_search_command_outputs_candidates(mock_search) -> None:
    mock_search.return_value = [
        SearchResult(id="BV123", title="测试视频", url="https://www.bilibili.com/video/BV123")
    ]

    result = runner.invoke(app, ["search", "测试"])

    assert result.exit_code == 0
    assert "测试视频" in result.output
    assert "BV123" in result.output


@patch("crawler.cli.download_video")
def test_download_command_accepts_bvid(mock_download) -> None:
    mock_download.return_value = "downloads/video.mp4"

    result = runner.invoke(app, ["download", "BV123"])

    assert result.exit_code == 0
    assert "downloads/video.mp4" in result.output
    mock_download.assert_called_once()

