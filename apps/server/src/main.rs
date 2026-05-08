mod bilibili;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, sync::Mutex};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use bilibili::{
    BilibiliClient, SearchItem,
};

// ── Models ──────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
struct SearchResult {
    id: String,
    title: String,
    url: String,
    uploader: Option<String>,
    duration: Option<f64>,
    view_count: Option<u64>,
    thumbnail: Option<String>,
}

#[derive(Serialize)]
struct SearchPage {
    items: Vec<SearchResult>,
    page: u32,
    page_size: u32,
    has_more: bool,
}

#[derive(Clone, Serialize)]
struct DownloadTask {
    task_id: String,
    status: String,
    progress: f64,
    filename: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
}

fn default_page() -> u32 { 1 }
fn default_page_size() -> u32 { 10 }

#[derive(Deserialize)]
struct DownloadRequest {
    url: String,
    #[serde(default = "default_output_dir")]
    output_dir: String,
    #[serde(default = "default_format")]
    format: String,
}

fn default_output_dir() -> String { "downloads".to_string() }
fn default_format() -> String { "bestvideo+bestaudio/best".to_string() }

#[derive(Deserialize)]
struct ImageQuery {
    url: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Serialize)]
struct SystemResponse {
    status: String,
    runtime: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    detail: String,
}

// ── App State ───────────────────────────────────────────────────────

struct AppState {
    tasks: Mutex<HashMap<String, DownloadTask>>,
    client: BilibiliClient,
}

type SharedState = Arc<AppState>;

// ── Handlers ────────────────────────────────────────────────────────

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok".to_string() })
}

async fn system_status() -> Json<SystemResponse> {
    Json(SystemResponse {
        status: "ok".to_string(),
        runtime: "axum-rust".to_string(),
    })
}

async fn search_endpoint(
    State(state): State<SharedState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchPage>, (StatusCode, Json<ErrorResponse>)> {
    let keyword = params.q.trim();
    if keyword.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            detail: "搜索关键词不能为空".to_string(),
        })));
    }

    let safe_page = params.page.clamp(1, 20);
    let safe_page_size = params.page_size.clamp(1, 30);

    let items_raw = state.client
        .search(keyword, safe_page, safe_page_size)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { detail: e })))?;

    let items: Vec<SearchResult> = items_raw
        .into_iter()
        .take(safe_page_size as usize)
        .map(to_search_result)
        .collect();

    Ok(Json(SearchPage {
        has_more: items.len() == safe_page_size as usize,
        items,
        page: safe_page,
        page_size: safe_page_size,
    }))
}

async fn proxy_image(
    State(state): State<SharedState>,
    Query(params): Query<ImageQuery>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let parsed = url::Url::parse(&params.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            detail: format!("图片地址无效：{e}"),
        })))?;

    let host = parsed.host_str().unwrap_or("");
    if !host.ends_with("hdslb.com") && !host.ends_with("biliimg.com") {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            detail: "只允许代理 Bilibili 图片".to_string(),
        })));
    }

    let response = state.client
        .proxy_image_bytes(&params.url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { detail: e })))?;

    let content_type = response.0;
    let bytes = response.1;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, axum::http::HeaderValue::from_static("public, max-age=86400")),
        ],
        bytes,
    ).into_response())
}

async fn create_download_task(
    State(state): State<SharedState>,
    Json(request): Json<DownloadRequest>,
) -> Result<(StatusCode, Json<DownloadTask>), (StatusCode, Json<ErrorResponse>)> {
    let url = request.url.trim().to_string();
    if url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            detail: "视频地址不能为空".to_string(),
        })));
    }

    let bvid = bilibili::extract_bvid(&url)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { detail: e })))?;

    let task = DownloadTask {
        task_id: Uuid::new_v4().simple().to_string(),
        status: "queued".to_string(),
        progress: 0.0,
        filename: None,
        error: None,
    };

    {
        let mut tasks = state.tasks.lock().await;
        tasks.insert(task.task_id.clone(), task.clone());
    }

    let task_id = task.task_id.clone();
    let output_dir = request.output_dir.clone();
    let shared = Arc::clone(&state);

    tokio::spawn(async move {
        run_download(shared, task_id, bvid, output_dir).await;
    });

    Ok((StatusCode::ACCEPTED, Json(task)))
}

async fn get_download_task(
    State(state): State<SharedState>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<DownloadTask>, (StatusCode, Json<ErrorResponse>)> {
    let tasks = state.tasks.lock().await;
    tasks.get(&task_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(ErrorResponse {
            detail: "下载任务不存在".to_string(),
        })))
}

// ── Download Logic ──────────────────────────────────────────────────

async fn run_download(state: SharedState, task_id: String, bvid: String, output_dir: String) {
    update_task(&state, &task_id, |t| {
        t.status = "running".to_string();
        t.progress = 1.0;
    }).await;

    match do_download(&state.client, &bvid, &output_dir, &state, &task_id).await {
        Ok(filename) => {
            update_task(&state, &task_id, |t| {
                t.status = "succeeded".to_string();
                t.progress = 100.0;
                t.filename = Some(filename);
                t.error = None;
            }).await;
        }
        Err(error) => {
            update_task(&state, &task_id, |t| {
                t.status = "failed".to_string();
                t.error = Some(error);
            }).await;
        }
    }
}

async fn do_download(
    client: &BilibiliClient,
    bvid: &str,
    output_dir: &str,
    state: &SharedState,
    task_id: &str,
) -> Result<String, String> {
    let view = client.fetch_view(bvid).await?;
    let download_url = client.fetch_play_url(&view.bvid, view.cid).await?;

    let target_dir = PathBuf::from(output_dir);
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("创建下载目录失败：{e}"))?;

    let extension = if download_url.url.contains(".flv") { "flv" } else { "mp4" };
    let filename = format!(
        "{}-{}.{}",
        bilibili::sanitize_filename(&view.title),
        view.bvid,
        extension
    );
    let target_path = target_dir.join(&filename);

    let response = client
        .download_stream(&download_url.url, &view.bvid)
        .await?;

    let total_size = response.content_length().or(download_url.size).unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&target_path)
        .await
        .map_err(|e| format!("创建视频文件失败：{e}"))?;
    let mut downloaded = 0_u64;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("读取视频流失败：{e}"))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("写入视频文件失败：{e}"))?;
        downloaded += bytes.len() as u64;

        if total_size > 0 {
            let progress = (downloaded as f64 / total_size as f64 * 99.0).clamp(1.0, 99.0);
            update_task(state, task_id, |t| { t.progress = progress; }).await;
        }
    }

    file.flush().await.map_err(|e| format!("保存视频文件失败：{e}"))?;
    Ok(target_path.to_string_lossy().to_string())
}

async fn update_task(state: &SharedState, task_id: &str, f: impl FnOnce(&mut DownloadTask)) {
    let mut tasks = state.tasks.lock().await;
    if let Some(task) = tasks.get_mut(task_id) {
        f(task);
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn to_search_result(item: SearchItem) -> SearchResult {
    let title = bilibili::clean_html(&item.title);
    let url = item.arcurl
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("https://www.bilibili.com/video/{}", item.bvid));

    SearchResult {
        id: item.bvid,
        title,
        url,
        uploader: item.author,
        duration: item.duration.as_deref().and_then(bilibili::parse_duration),
        view_count: item.play,
        thumbnail: item.pic.map(|v| if v.starts_with("//") { format!("https:{v}") } else { v }),
    }
}

// ── Main ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState {
        tasks: Mutex::new(HashMap::new()),
        client: BilibiliClient::new().expect("初始化 HTTP 客户端失败"),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/system", get(system_status))
        .route("/api/search", get(search_endpoint))
        .route("/api/image", get(proxy_image))
        .route("/api/download", post(create_download_task))
        .route("/api/downloads/{task_id}", get(get_download_task))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000")
        .await
        .expect("无法绑定端口 8000");

    println!("🚀 Bilibili Crawler API 已启动: http://localhost:8000");
    axum::serve(listener, app).await.expect("服务器运行失败");
}
