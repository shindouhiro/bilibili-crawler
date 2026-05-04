import type { DownloadTask, SearchPage } from './types'

interface ApiErrorPayload {
  detail?: unknown
}

async function parseResponse<T>(response: Response): Promise<T> {
  if (response.ok)
    return response.json() as Promise<T>

  const payload = await response.json().catch(() => null) as ApiErrorPayload | null
  const message = typeof payload?.detail === 'string' ? payload.detail : '请求失败，请稍后重试'
  throw new Error(message)
}

export async function searchVideos(query: string, page = 1, pageSize = 10): Promise<SearchPage> {
  const params = new URLSearchParams({
    q: query,
    page: String(page),
    page_size: String(pageSize),
  })
  const response = await fetch(`/api/search?${params}`)
  return parseResponse<SearchPage>(response)
}

export function getProxiedImageUrl(url: string): string {
  const params = new URLSearchParams({ url })
  return `/api/image?${params}`
}

export async function createDownload(url: string): Promise<DownloadTask> {
  const response = await fetch('/api/download', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url }),
  })
  return parseResponse<DownloadTask>(response)
}

export async function getDownloadTask(taskId: string): Promise<DownloadTask> {
  const response = await fetch(`/api/downloads/${taskId}`)
  return parseResponse<DownloadTask>(response)
}
