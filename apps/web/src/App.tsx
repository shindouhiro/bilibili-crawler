import type { DownloadTask, SearchResult } from './types'
import * as Dialog from '@radix-ui/react-dialog'
import * as Progress from '@radix-ui/react-progress'
import { AlertCircle, CheckCircle2, Clock, Download, ExternalLink, Eye, Film, ImageOff, Loader2, PlayCircle, Search, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { createDownload, getDownloadTask, getProxiedImageUrl, searchVideos } from './api'
import logoMarkUrl from './assets/logo-mark.svg'

function formatDuration(seconds?: number | null): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds <= 0)
    return '--:--'
  const minute = Math.floor(seconds / 60)
  const second = Math.floor(seconds % 60)
  return `${minute}:${second.toString().padStart(2, '0')}`
}

function formatViews(count?: number | null): string {
  if (count == null || !Number.isFinite(count) || count <= 0)
    return '暂无数据'
  if (count >= 10000)
    return `${(count / 10000).toFixed(1)} 万`
  return `${count}`
}

function toDomId(value: string): string {
  const normalized = value.replace(/[^\w-]/g, '-')
  return normalized.length > 0 ? normalized : 'unknown'
}

const SEARCH_PAGE_SIZE = 10

function appendUniqueResults(current: SearchResult[], next: SearchResult[]): SearchResult[] {
  const knownKeys = new Set(current.map(video => (video.url.length > 0 ? video.url : video.id)))
  const uniqueNext = next.filter((video) => {
    const key = video.url.length > 0 ? video.url : video.id
    if (knownKeys.has(key))
      return false
    knownKeys.add(key)
    return true
  })
  return [...current, ...uniqueNext]
}

export default function App() {
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<SearchResult[]>([])
  const [activeTask, setActiveTask] = useState<DownloadTask | null>(null)
  const [isSearching, setIsSearching] = useState(false)
  const [isLoadingMore, setIsLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [currentPage, setCurrentPage] = useState(1)
  const [hasMore, setHasMore] = useState(false)
  const [selectedVideo, setSelectedVideo] = useState<SearchResult | null>(null)

  const trimmedQuery = query.trim()
  const canSearch = trimmedQuery.length > 0 && !isSearching
  const canLoadMore = trimmedQuery.length > 0 && hasMore && !isSearching && !isLoadingMore
  const progressValue = activeTask?.progress ?? 0
  const taskStateLabel = useMemo(() => {
    if (!activeTask)
      return '等待下载'
    if (activeTask.status === 'queued')
      return '排队中...'
    if (activeTask.status === 'running')
      return '正在下载'
    if (activeTask.status === 'succeeded')
      return '下载完成'
    return '下载失败'
  }, [activeTask])

  useEffect(() => {
    if (!activeTask || !['queued', 'running'].includes(activeTask.status))
      return

    const timer = window.setInterval(() => {
      void (async () => {
        try {
          const nextTask = await getDownloadTask(activeTask.task_id)
          setActiveTask(nextTask)
        }
        catch (pollError) {
          setError(pollError instanceof Error ? pollError.message : '读取下载状态失败')
        }
      })()
    }, 1200)

    return () => window.clearInterval(timer)
  }, [activeTask])

  async function handleSearch(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!canSearch)
      return

    setIsSearching(true)
    setError(null)
    setSelectedVideo(null)
    setCurrentPage(1)
    setHasMore(false)
    try {
      const firstPage = await searchVideos(trimmedQuery, 1, SEARCH_PAGE_SIZE)
      setResults(firstPage.items)
      setCurrentPage(firstPage.page)
      setHasMore(firstPage.has_more)
      if (firstPage.items.length === 0)
        setError('没有找到匹配视频，请换一个关键词。')
    }
    catch (searchError) {
      setError(searchError instanceof Error ? searchError.message : '搜索失败')
    }
    finally {
      setIsSearching(false)
    }
  }

  async function handleLoadMore() {
    if (!canLoadMore)
      return

    setIsLoadingMore(true)
    setError(null)
    try {
      const nextPageNumber = currentPage + 1
      const nextPage = await searchVideos(trimmedQuery, nextPageNumber, SEARCH_PAGE_SIZE)
      setResults(current => appendUniqueResults(current, nextPage.items))
      setCurrentPage(nextPage.page)
      setHasMore(nextPage.has_more)
    }
    catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : '加载更多失败')
    }
    finally {
      setIsLoadingMore(false)
    }
  }

  async function handleDownload(video: SearchResult) {
    setError(null)
    setSelectedVideo(video)
    try {
      const task = await createDownload(video.url)
      setActiveTask(task)
    }
    catch (downloadError) {
      setError(downloadError instanceof Error ? downloadError.message : '创建下载任务失败')
    }
  }

  return (
    <>
      <div className="mesh-bg" />
      <main className="min-h-screen text-slate-100 font-sans selection:bg-cyan-500/30 selection:text-cyan-50">
        <section className="mx-auto flex min-h-screen w-full max-w-6xl flex-col px-4 py-8 sm:px-8 lg:px-10 lg:py-12">
          {/* Header */}
          <header className="mb-10 flex flex-col gap-6 animate-fade-in-up">
            <div className="flex flex-col md:flex-row md:items-end md:justify-between gap-6">
              <div>
                <div className="flex items-center gap-3 mb-4">
                  <div className="flex items-center justify-center w-12 h-12 rounded-2xl shadow-[0_0_24px_rgba(34,211,238,0.42)]">
                    <img alt="Bilibili Crawler" className="h-full w-full rounded-2xl" src={logoMarkUrl} />
                  </div>
                  <p className="text-sm font-bold tracking-[0.2em] text-cyan-400 uppercase drop-shadow-[0_0_8px_rgba(34,211,238,0.5)]">
                    Bilibili Downloader
                  </p>
                </div>
                <h1 className="font-heading text-4xl font-extrabold tracking-tight md:text-5xl lg:text-6xl bg-clip-text text-transparent bg-gradient-to-r from-white via-cyan-100 to-cyan-500 drop-shadow-sm">
                  视频解析与下载
                </h1>
              </div>
              <div className="glass-panel rounded-full px-5 py-2.5 text-sm font-medium text-cyan-100 border-cyan-500/20 shadow-[0_0_20px_rgba(34,211,238,0.05)] w-fit flex items-center gap-3">
                <span className="relative flex h-2.5 w-2.5">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-cyan-400 opacity-75"></span>
                  <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-cyan-500"></span>
                </span>
                仅支持公开视频 · 自动获取最高画质
              </div>
            </div>
          </header>

          {/* Search Form */}
          <form
            id="video-search-form"
            className="mb-8 flex flex-col gap-4 sm:flex-row animate-fade-in-up stagger-1 relative group z-10"
            onSubmit={(event) => {
              void handleSearch(event)
            }}
          >
            <div className="absolute -inset-1 bg-gradient-to-r from-cyan-500 to-blue-600 rounded-2xl blur opacity-25 group-focus-within:opacity-50 transition duration-1000 group-hover:opacity-40"></div>
            <label className="sr-only" htmlFor="search-input">搜索关键词</label>
            <div className="relative flex-1 group/input">
              <div className="absolute inset-y-0 left-0 pl-5 flex items-center pointer-events-none transition-colors group-focus-within/input:text-cyan-400">
                <Search className="h-5 w-5 text-slate-400 group-focus-within/input:text-cyan-400 transition-colors" />
              </div>
              <input
                id="search-input"
                className="relative w-full h-14 rounded-xl border border-white/10 bg-slate-900/60 pl-12 pr-4 text-lg text-white outline-none backdrop-blur-2xl transition-all focus:border-cyan-400/50 focus:bg-slate-900/80 focus:shadow-[0_0_0_4px_rgba(34,211,238,0.15)] placeholder:text-slate-500"
                placeholder="输入视频名称、UP 主或关键词..."
                value={query}
                onChange={event => setQuery(event.target.value)}
              />
            </div>
            <button
              id="search-submit-button"
              className="relative inline-flex h-14 items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-blue-600 px-8 font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.3)] transition-all hover:scale-[1.02] hover:shadow-[0_0_25px_rgba(6,182,212,0.5)] active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:scale-100 disabled:hover:shadow-none"
              disabled={!canSearch}
              type="submit"
            >
              {isSearching ? <Loader2 className="size-5 animate-spin" /> : null}
              <span className="text-lg">{isSearching ? '解析中...' : '开始解析'}</span>
            </button>
          </form>

          {/* Error Message */}
          {error !== null && (
            <div id="error-message" className="mb-8 flex items-start gap-3 rounded-xl border border-rose-500/30 bg-rose-500/10 px-5 py-4 text-sm text-rose-200 backdrop-blur-md animate-fade-in-up shadow-[0_4px_20px_rgba(244,63,94,0.1)]">
              <AlertCircle className="mt-0.5 size-5 shrink-0 text-rose-400" />
              <span className="leading-relaxed font-medium">{error}</span>
            </div>
          )}

          {/* Main Content */}
          <div className="grid flex-1 gap-8 lg:grid-cols-[1fr_360px] animate-fade-in-up stagger-2 relative z-0">
            {/* Results Section */}
            <section aria-label="搜索结果" className="min-h-80 flex flex-col">
              <div className="mb-5 flex items-center justify-between">
                <h2 className="text-xl font-heading font-semibold text-white flex items-center gap-2">
                  <PlayCircle className="size-5 text-cyan-400" />
                  候选视频
                </h2>
                {results.length > 0 && (
                  <span className="glass-panel px-3 py-1 rounded-full text-xs font-semibold text-cyan-300">
                    {results.length}
                    {' '}
                    个结果
                  </span>
                )}
              </div>

              <div className="grid gap-4 flex-1 content-start">
                {results.map((video, index) => (
                  <article
                    key={video.url.length > 0 ? video.url : video.id}
                    className="glass-card group flex flex-col sm:flex-row gap-5 rounded-2xl p-4 transition-all duration-300 hover:border-cyan-400/40 hover:-translate-y-1 hover:shadow-[0_10px_40px_-10px_rgba(34,211,238,0.2)] animate-fade-in-up"
                    style={{ animationDelay: `${(index * 0.05) + 0.3}s`, opacity: 0 }}
                  >
                    <div className="relative aspect-video sm:w-48 sm:shrink-0 overflow-hidden rounded-xl bg-slate-800/80 shadow-inner">
                      {video.thumbnail != null && video.thumbnail.length > 0
                        ? (
                            <>
                              <img alt={video.title} className="h-full w-full object-cover transition-transform duration-700 group-hover:scale-105 group-hover:opacity-90" src={getProxiedImageUrl(video.thumbnail)} />
                              <div className="absolute inset-0 bg-gradient-to-t from-slate-900/80 via-transparent to-transparent opacity-80" />
                            </>
                          )
                        : (
                            <div className="flex flex-col gap-2 h-full items-center justify-center text-sm text-slate-500">
                              <ImageOff className="size-6 opacity-50" />
                              <span>暂无封面</span>
                            </div>
                          )}
                      <div className="absolute bottom-2 right-2 rounded bg-black/70 px-1.5 py-0.5 text-xs font-medium text-white backdrop-blur-sm flex items-center gap-1">
                        <Clock className="size-3" />
                        {formatDuration(video.duration)}
                      </div>
                    </div>

                    <div className="flex min-w-0 flex-1 flex-col justify-between py-1 gap-4">
                      <div className="min-w-0">
                        <h3 className="line-clamp-2 text-base font-semibold text-white group-hover:text-cyan-50 transition-colors leading-snug" title={video.title}>
                          {video.title}
                        </h3>
                        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm text-slate-400">
                          <span className="flex items-center gap-1.5 text-slate-300 font-medium bg-white/5 px-2 py-0.5 rounded-md">
                            {video.uploader != null && video.uploader.length > 0 ? video.uploader : '未知 UP'}
                          </span>
                          <span className="flex items-center gap-1.5">
                            <Eye className="size-3.5 opacity-70" />
                            {formatViews(video.view_count)}
                            {' '}
                            播放
                          </span>
                        </div>
                      </div>

                      <div className="flex items-center gap-3 mt-auto">
                        <button
                          id={`result-download-button-${toDomId(video.id)}`}
                          className="inline-flex h-9 items-center justify-center gap-2 rounded-lg bg-cyan-500/10 px-4 text-sm font-semibold text-cyan-300 transition-all hover:bg-cyan-400 hover:text-slate-900 hover:shadow-[0_0_15px_rgba(34,211,238,0.4)] active:scale-95"
                          type="button"
                          onClick={() => void handleDownload(video)}
                        >
                          <Download className="size-4" />
                          下载此视频
                        </button>
                        <a
                          id={`result-open-link-${toDomId(video.id)}`}
                          className="inline-flex h-9 w-9 items-center justify-center rounded-lg bg-white/5 text-slate-400 hover:bg-white/10 hover:text-white transition-all active:scale-95"
                          href={video.url}
                          rel="noreferrer"
                          target="_blank"
                          title="在浏览器中打开"
                        >
                          <ExternalLink className="size-4" />
                        </a>
                      </div>
                    </div>
                  </article>
                ))}

                {!isSearching && results.length === 0 && (
                  <div id="empty-results" className="glass-panel flex min-h-[320px] flex-col items-center justify-center rounded-2xl border border-dashed border-white/20 text-slate-400 p-8 text-center animate-fade-in-up stagger-3">
                    <div className="w-16 h-16 mb-4 rounded-full bg-slate-800/50 flex items-center justify-center shadow-inner">
                      <Search className="size-8 text-slate-500" />
                    </div>
                    <p className="text-lg font-medium text-slate-300 mb-2">暂无搜索结果</p>
                    <p className="text-sm opacity-80">输入视频名称、UP主或关键词后，这里会显示匹配的视频。</p>
                  </div>
                )}

                {results.length > 0 && (
                  <div className="flex justify-center pt-3">
                    <button
                      id="load-more-results-button"
                      className="glass-panel inline-flex min-h-11 items-center justify-center gap-2 rounded-xl border-cyan-500/20 px-5 text-sm font-semibold text-cyan-200 transition-all hover:border-cyan-400/40 hover:bg-cyan-400/10 disabled:cursor-not-allowed disabled:opacity-50"
                      disabled={!canLoadMore}
                      type="button"
                      onClick={() => {
                        void handleLoadMore()
                      }}
                    >
                      {isLoadingMore ? <Loader2 className="size-4 animate-spin" /> : null}
                      {hasMore ? '加载更多' : '没有更多结果'}
                    </button>
                  </div>
                )}
              </div>
            </section>

            {/* Sidebar / Status Section */}
            <aside aria-label="下载状态" className="h-fit sticky top-8">
              <div className="glass-panel rounded-2xl p-6 shadow-xl relative overflow-hidden">
                {/* Decorative glow */}
                <div className="absolute -top-24 -right-24 w-48 h-48 bg-cyan-500/20 rounded-full blur-[50px] pointer-events-none" />

                <div className="mb-6 flex items-center justify-between relative z-10">
                  <h2 className="text-xl font-heading font-semibold text-white flex items-center gap-2">
                    下载状态
                  </h2>
                  {activeTask?.status === 'succeeded' && (
                    <span className="flex items-center justify-center w-8 h-8 rounded-full bg-emerald-500/20 text-emerald-400">
                      <CheckCircle2 className="size-5" />
                    </span>
                  )}
                  {activeTask?.status === 'running' && (
                    <span className="flex items-center justify-center w-8 h-8 text-cyan-400">
                      <Loader2 className="size-5 animate-spin" />
                    </span>
                  )}
                </div>

                <div className="mb-6 rounded-xl bg-slate-900/60 border border-white/5 p-5 relative z-10">
                  <div className="flex justify-between items-end mb-3">
                    <p id="download-state-label" className="text-sm font-medium text-slate-300">{taskStateLabel}</p>
                    <p className="text-xl font-bold font-heading text-cyan-400 drop-shadow-[0_0_8px_rgba(34,211,238,0.3)]">
                      {Math.round(progressValue)}
                      %
                    </p>
                  </div>
                  <Progress.Root id="download-progress" className="h-2.5 overflow-hidden rounded-full bg-slate-800/80 shadow-inner" value={progressValue}>
                    <Progress.Indicator
                      className="h-full rounded-full bg-gradient-to-r from-cyan-500 to-blue-500 transition-all duration-500 ease-out relative"
                      style={{ transform: `translateX(-${100 - progressValue}%)` }}
                    >
                      <div className="absolute inset-0 bg-white/20 w-full animate-[shimmer_2s_infinite]" style={{ backgroundImage: 'linear-gradient(90deg, transparent, rgba(255,255,255,0.4), transparent)' }} />
                    </Progress.Indicator>
                  </Progress.Root>
                </div>

                {selectedVideo !== null && (
                  <div className="mb-6 rounded-xl bg-white/5 p-4 border border-white/5 relative z-10">
                    <p className="mb-1.5 text-xs font-semibold uppercase tracking-wider text-slate-500">当前任务</p>
                    <p id="selected-video-title" className="line-clamp-2 text-sm font-medium text-slate-200 leading-snug" title={selectedVideo.title}>
                      {selectedVideo.title}
                    </p>
                  </div>
                )}

                {activeTask?.filename != null && activeTask.filename.length > 0 && (
                  <Dialog.Root>
                    <Dialog.Trigger asChild>
                      <button
                        id="download-file-detail-button"
                        className="relative z-10 inline-flex min-h-12 w-full items-center justify-center gap-2 rounded-xl border border-cyan-400/30 bg-cyan-500/10 text-sm font-semibold text-cyan-300 transition-all hover:bg-cyan-400 hover:text-slate-900 hover:shadow-[0_0_20px_rgba(34,211,238,0.3)] active:scale-[0.98]"
                        type="button"
                      >
                        <Film className="size-4" />
                        查看文件位置
                      </button>
                    </Dialog.Trigger>
                    <Dialog.Portal>
                      <Dialog.Overlay className="fixed inset-0 bg-slate-950/80 backdrop-blur-sm z-50 animate-fade-in-up data-[state=closed]:animate-out data-[state=closed]:fade-out" />
                      <Dialog.Content className="fixed left-1/2 top-1/2 w-[min(92vw,560px)] -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-white/10 bg-slate-900 p-6 text-white shadow-2xl z-50 animate-fade-in-up glass-card">
                        <div className="mb-6 flex items-center justify-between gap-4">
                          <Dialog.Title className="text-xl font-heading font-semibold">下载详情</Dialog.Title>
                          <Dialog.Close
                            id="download-file-dialog-close-button"
                            className="rounded-full p-2 text-slate-400 transition-all hover:bg-white/10 hover:text-white hover:rotate-90"
                          >
                            <X className="size-5" />
                          </Dialog.Close>
                        </div>

                        <div className="space-y-4">
                          <div>
                            <p className="text-xs font-semibold text-slate-500 mb-2 uppercase tracking-wider">保存路径</p>
                            <p id="download-file-path" className="break-all rounded-xl bg-black/40 border border-white/5 p-4 text-sm font-mono text-cyan-100 shadow-inner">
                              {activeTask.filename}
                            </p>
                          </div>
                        </div>
                      </Dialog.Content>
                    </Dialog.Portal>
                  </Dialog.Root>
                )}

                {activeTask?.error != null && activeTask.error.length > 0 && (
                  <div id="download-error-message" className="mt-6 rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-300 relative z-10 flex gap-3">
                    <AlertCircle className="size-5 shrink-0 text-rose-400" />
                    <span>{activeTask.error}</span>
                  </div>
                )}
              </div>
            </aside>
          </div>
        </section>
      </main>
    </>
  )
}
