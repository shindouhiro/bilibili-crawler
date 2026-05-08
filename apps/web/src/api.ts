import type { DownloadRecord, DownloadTask, SearchPage } from './types'
import { invoke } from '@tauri-apps/api/core'

function toErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string' && error.length > 0) return error
  return fallback
}

function isTauriReady(): boolean {
  return typeof globalThis === 'object' && '__TAURI_INTERNALS__' in globalThis
}

async function invokeOrThrow<T>(command: string, args?: Record<string, unknown>, fallback = '操作失败'): Promise<T> {
  try {
    return await invoke<T>(command, args)
  } catch (error) {
    throw new Error(toErrorMessage(error, fallback))
  }
}

async function parseResponse<T>(response: Response): Promise<T> {
  if (response.ok) return response.json() as Promise<T>
  const payload = await response.json().catch(() => null)
  const message = typeof payload?.detail === 'string' ? payload.detail : '请求失败，请稍后重试'
  throw new Error(message)
}

export async function searchVideos(query: string, page = 1, pageSize = 10): Promise<SearchPage> {
  if (isTauriReady()) {
    return invokeOrThrow<SearchPage>('search_videos', { query, page, pageSize }, '搜索失败')
  }
  const params = new URLSearchParams({ q: query, page: String(page), page_size: String(pageSize) })
  const response = await fetch(`/api/search?${params}`)
  return parseResponse<SearchPage>(response)
}

export async function getProxiedImageDataUrl(url: string): Promise<string> {
  const absoluteUrl = url.startsWith('//') ? `https:${url}` : url

  if (isTauriReady()) {
    try {
      return await invoke<string>('proxy_image', { url: absoluteUrl })
    } catch (error) {
      console.error('图片代理失败:', absoluteUrl, error)
      return ''
    }
  }

  const params = new URLSearchParams({ url: absoluteUrl })
  return `/api/image?${params}`
}

export function getProxiedImageUrl(url: string): string {
  const absoluteUrl = url.startsWith('//') ? `https:${url}` : url;
  if (isTauriReady()) return 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'
  
  const params = new URLSearchParams({ url: absoluteUrl })
  return `/api/image?${params}`
}

export async function createDownload(url: string, bvid?: string): Promise<DownloadTask> {
  if (isTauriReady()) {
    return invokeOrThrow<DownloadTask>('create_download', { url: bvid || url }, '创建下载任务失败')
  }
  const response = await fetch('/api/download', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url: bvid || url }),
  })
  return parseResponse<DownloadTask>(response)
}

export async function getDownloadTask(taskId: string): Promise<DownloadTask> {
  if (isTauriReady()) {
    return invokeOrThrow<DownloadTask>('get_download_task', { taskId }, '读取下载状态失败')
  }
  const response = await fetch(`/api/downloads/${taskId}`)
  return parseResponse<DownloadTask>(response)
}

export async function getDownloadHistory(limit = 50): Promise<DownloadRecord[]> {
  if (isTauriReady()) {
    return invokeOrThrow<DownloadRecord[]>('get_download_history', { limit }, '读取下载历史失败')
  }
  return [] // Web 端不支持获取本地 SQLite 下载历史
}

export async function deleteDownloadRecord(id: number): Promise<void> {
  if (isTauriReady()) {
    return invokeOrThrow<void>('delete_download_record', { id }, '删除记录失败')
  }
}

export async function clearDownloadHistory(): Promise<void> {
  if (isTauriReady()) {
    return invokeOrThrow<void>('clear_download_history', {}, '清空历史失败')
  }
}

export async function revealFile(path: string): Promise<void> {
  if (isTauriReady()) {
    return invokeOrThrow<void>('reveal_file', { path }, '打开文件位置失败')
  }
}
