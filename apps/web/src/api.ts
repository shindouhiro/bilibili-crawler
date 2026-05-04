import type { DownloadTask, SearchPage } from './types'
import { invoke } from '@tauri-apps/api/core'

interface ApiErrorPayload {
  detail?: unknown
}

function isTauriRuntime(): boolean {
  return typeof globalThis === 'object' && '__TAURI_INTERNALS__' in globalThis
}

function toErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error)
    return error.message
  if (typeof error === 'string' && error.length > 0)
    return error
  return fallback
}

async function invokeOrThrow<T>(command: string, args?: Record<string, unknown>, fallback = '操作失败'): Promise<T> {
  try {
    return await invoke<T>(command, args)
  }
  catch (error) {
    throw new Error(toErrorMessage(error, fallback))
  }
}

async function parseResponse<T>(response: Response): Promise<T> {
  if (response.ok)
    return response.json() as Promise<T>

  const payload = await response.json().catch(() => null) as ApiErrorPayload | null
  const message = typeof payload?.detail === 'string' ? payload.detail : '请求失败，请稍后重试'
  throw new Error(message)
}

export async function searchVideos(query: string, page = 1, pageSize = 10): Promise<SearchPage> {
  if (isTauriRuntime()) {
    return invokeOrThrow<SearchPage>('search_videos', { query, page, pageSize }, '搜索失败')
  }

  const params = new URLSearchParams({
    q: query,
    page: String(page),
    page_size: String(pageSize),
  })
  const response = await fetch(`/api/search?${params}`)
  return parseResponse<SearchPage>(response)
}

export function getProxiedImageUrl(url: string): string {
  if (isTauriRuntime())
    return url

  const params = new URLSearchParams({ url })
  return `/api/image?${params}`
}

export async function createDownload(url: string): Promise<DownloadTask> {
  if (isTauriRuntime())
    return invokeOrThrow<DownloadTask>('create_download', { url }, '创建下载任务失败')

  const response = await fetch('/api/download', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url }),
  })
  return parseResponse<DownloadTask>(response)
}

export async function getDownloadTask(taskId: string): Promise<DownloadTask> {
  if (isTauriRuntime())
    return invokeOrThrow<DownloadTask>('get_download_task', { taskId }, '读取下载状态失败')

  const response = await fetch(`/api/downloads/${taskId}`)
  return parseResponse<DownloadTask>(response)
}
