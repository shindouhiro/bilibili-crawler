from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console
from rich.progress import BarColumn, Progress, TextColumn, TimeRemainingColumn
from rich.table import Table

from crawler.downloader import BilibiliDownloadError, download_video, search_videos

app = typer.Typer(help="Bilibili 视频搜索与下载工具")
console = Console()


@app.command()
def search(
    keyword: str,
    limit: Annotated[int, typer.Option(min=1, max=30, help="候选数量")] = 10,
) -> None:
    """按关键词搜索 Bilibili 视频。"""
    try:
        results = search_videos(keyword, limit)
    except (ValueError, BilibiliDownloadError) as exc:
        console.print(f"[red]{exc}[/red]")
        raise typer.Exit(code=1) from exc

    table = Table(title=f"Bilibili 搜索结果：{keyword}")
    table.add_column("序号", justify="right")
    table.add_column("标题")
    table.add_column("UP 主")
    table.add_column("链接")

    for index, result in enumerate(results, start=1):
        table.add_row(str(index), result.title, result.uploader or "-", result.url)

    console.print(table)
    if not results:
        console.print("[yellow]没有找到匹配视频。[/yellow]")


@app.command()
def download(
    url_or_bvid: Annotated[str, typer.Argument(help="Bilibili 视频链接或 BV 号")],
    output_dir: Annotated[Path, typer.Option(help="下载目录")] = Path("downloads"),
) -> None:
    """下载指定 Bilibili 视频。"""
    progress_bar = Progress(
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        TextColumn("{task.percentage:>3.0f}%"),
        TimeRemainingColumn(),
    )

    with progress_bar:
        task_id = progress_bar.add_task("下载中", total=100)

        def update(progress: dict) -> None:
            if progress.get("status") == "downloading":
                total = progress.get("total_bytes") or progress.get("total_bytes_estimate")
                downloaded = progress.get("downloaded_bytes") or 0
                if total:
                    progress_bar.update(task_id, completed=min(95, downloaded / total * 95))
            elif progress.get("status") == "finished":
                progress_bar.update(task_id, completed=95, description="合并中")

        try:
            filename = download_video(url_or_bvid, output_dir=output_dir, progress_hook=update)
        except (ValueError, BilibiliDownloadError) as exc:
            console.print(f"[red]{exc}[/red]")
            raise typer.Exit(code=1) from exc

        progress_bar.update(task_id, completed=100, description="完成")

    console.print(f"[green]下载完成：[/green]{filename}")


if __name__ == "__main__":
    app()
