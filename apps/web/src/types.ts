export interface SearchResult {
  id: string
  title: string
  url: string
  uploader?: string | null
  duration?: number | null
  view_count?: number | null
  thumbnail?: string | null
}

export interface SearchPage {
  items: SearchResult[]
  page: number
  page_size: number
  has_more: boolean
}

export interface DownloadTask {
  task_id: string
  status: 'queued' | 'running' | 'succeeded' | 'failed'
  progress: number
  filename?: string | null
  error?: string | null
}

export interface DownloadRecord {
  id: number
  task_id: string
  bvid: string
  title: string
  url: string
  filename?: string | null
  file_size?: number | null
  status: string
  error?: string | null
  created_at: string
  completed_at?: string | null
}
