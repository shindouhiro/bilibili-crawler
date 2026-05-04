use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

/// 下载历史记录（返回给前端的数据结构）
#[derive(Clone, Serialize)]
pub struct DownloadRecord {
    pub id: i64,
    pub task_id: String,
    pub bvid: String,
    pub title: String,
    pub url: String,
    pub filename: Option<String>,
    pub file_size: Option<u64>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// SQLite 数据库封装
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// 初始化数据库，创建必要的表
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建数据库目录失败：{e}"))?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| format!("打开数据库失败：{e}"))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS downloads (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id     TEXT    NOT NULL UNIQUE,
                bvid        TEXT    NOT NULL,
                title       TEXT    NOT NULL DEFAULT '',
                url         TEXT    NOT NULL DEFAULT '',
                filename    TEXT,
                file_size   INTEGER,
                status      TEXT    NOT NULL DEFAULT 'queued',
                error       TEXT,
                created_at  TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
                completed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
            CREATE INDEX IF NOT EXISTS idx_downloads_created ON downloads(created_at DESC);
            ",
        )
        .map_err(|e| format!("初始化数据库表失败：{e}"))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 插入一条新的下载记录
    pub fn insert_download(
        &self,
        task_id: &str,
        bvid: &str,
        title: &str,
        url: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        conn.execute(
            "INSERT INTO downloads (task_id, bvid, title, url, status) VALUES (?1, ?2, ?3, ?4, 'queued')",
            params![task_id, bvid, title, url],
        )
        .map_err(|e| format!("插入下载记录失败：{e}"))?;
        Ok(())
    }

    /// 更新下载状态为成功
    pub fn mark_succeeded(
        &self,
        task_id: &str,
        filename: &str,
        file_size: Option<u64>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        conn.execute(
            "UPDATE downloads SET status = 'succeeded', filename = ?1, file_size = ?2, completed_at = datetime('now', 'localtime') WHERE task_id = ?3",
            params![filename, file_size.map(|s| s as i64), task_id],
        )
        .map_err(|e| format!("更新下载记录失败：{e}"))?;
        Ok(())
    }

    /// 更新下载状态为失败
    pub fn mark_failed(&self, task_id: &str, error: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        conn.execute(
            "UPDATE downloads SET status = 'failed', error = ?1, completed_at = datetime('now', 'localtime') WHERE task_id = ?2",
            params![error, task_id],
        )
        .map_err(|e| format!("更新下载记录失败：{e}"))?;
        Ok(())
    }

    /// 查询下载历史（最近 N 条）
    pub fn list_downloads(&self, limit: u32) -> Result<Vec<DownloadRecord>, String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, bvid, title, url, filename, file_size, status, error, created_at, completed_at
                 FROM downloads ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|e| format!("查询下载记录失败：{e}"))?;

        let records = stmt
            .query_map(params![limit], |row| {
                Ok(DownloadRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    bvid: row.get(2)?,
                    title: row.get(3)?,
                    url: row.get(4)?,
                    filename: row.get(5)?,
                    file_size: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                    status: row.get(7)?,
                    error: row.get(8)?,
                    created_at: row.get(9)?,
                    completed_at: row.get(10)?,
                })
            })
            .map_err(|e| format!("读取下载记录失败：{e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("解析下载记录失败：{e}"))?;

        Ok(records)
    }

    /// 删除一条下载记录
    pub fn delete_download(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])
            .map_err(|e| format!("删除下载记录失败：{e}"))?;
        Ok(())
    }

    /// 更新下载记录的标题
    pub fn update_title(&self, task_id: &str, title: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        conn.execute(
            "UPDATE downloads SET title = ?1 WHERE task_id = ?2",
            params![title, task_id],
        )
        .map_err(|e| format!("更新标题失败：{e}"))?;
        Ok(())
    }

    /// 清空所有下载记录
    pub fn clear_downloads(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁已损坏")?;
        conn.execute("DELETE FROM downloads", [])
            .map_err(|e| format!("清空下载记录失败：{e}"))?;
        Ok(())
    }
}
