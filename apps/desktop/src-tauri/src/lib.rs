mod db;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use db::{Database, DownloadRecord};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderValue, REFERER, USER_AGENT},
    Url,
};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
const SEARCH_REFERER: &str = "https://search.bilibili.com/";
const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

type SharedState = Arc<AppState>;

struct AppState {
    tasks: Mutex<HashMap<String, DownloadTask>>,
    db: Database,
}

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

#[derive(Serialize)]
struct SystemStatus {
    status: String,
    runtime: String,
}

#[derive(Deserialize)]
struct BilibiliResponse<T> {
    code: i32,
    message: Option<String>,
    data: Option<T>,
}

#[derive(Deserialize)]
struct NavData {
    wbi_img: WbiImage,
}

#[derive(Deserialize)]
struct WbiImage {
    img_url: String,
    sub_url: String,
}

#[derive(Deserialize)]
struct SearchData {
    #[serde(default)]
    result: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    bvid: String,
    title: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    play: Option<u64>,
    #[serde(default)]
    pic: Option<String>,
    #[serde(default)]
    arcurl: Option<String>,
}

#[derive(Deserialize)]
struct ViewData {
    bvid: String,
    cid: u64,
    title: String,
}

#[derive(Deserialize)]
struct PlayData {
    durl: Option<Vec<DownloadUrl>>,
}

#[derive(Deserialize)]
struct DownloadUrl {
    url: String,
    #[serde(default)]
    size: Option<u64>,
}

#[tauri::command]
fn system_status() -> SystemStatus {
    SystemStatus {
        status: "ok".to_string(),
        runtime: "tauri-rust".to_string(),
    }
}

#[tauri::command]
async fn search_videos(query: String, page: u32, page_size: u32) -> Result<SearchPage, String> {
    let keyword = query.trim();
    if keyword.is_empty() {
        return Err("搜索关键词不能为空".to_string());
    }

    let safe_page = page.clamp(1, 20);
    let safe_page_size = page_size.clamp(1, 30);
    let client = http_client()?;
    seed_bilibili_cookies(&client).await?;
    let response: BilibiliResponse<SearchData> = client
        .get(
            signed_api_url(
                &client,
                "https://api.bilibili.com/x/web-interface/search/type",
                vec![
                    ("search_type", "video".to_string()),
                    ("keyword", keyword.to_string()),
                    ("page", safe_page.to_string()),
                    ("page_size", safe_page_size.to_string()),
                ],
            )
            .await?,
        )
        .send()
        .await
        .map_err(|error| format!("Bilibili 搜索失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("Bilibili 搜索失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("Bilibili 搜索响应解析失败：{error}"))?;

    if response.code != 0 {
        return Err(response
            .message
            .unwrap_or_else(|| "Bilibili 搜索失败".to_string()));
    }

    let items = response
        .data
        .map(|data| data.result)
        .unwrap_or_default()
        .into_iter()
        .take(safe_page_size as usize)
        .map(to_search_result)
        .collect::<Vec<_>>();

    Ok(SearchPage {
        has_more: items.len() == safe_page_size as usize,
        items,
        page: safe_page,
        page_size: safe_page_size,
    })
}

#[tauri::command]
async fn create_download(
    url: String,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<DownloadTask, String> {
    let bvid = extract_bvid(&url)?;
    let task = DownloadTask {
        task_id: Uuid::new_v4().simple().to_string(),
        status: "queued".to_string(),
        progress: 0.0,
        filename: None,
        error: None,
    };
    insert_task(&state, task.clone())?;

    // 写入 SQLite 下载历史
    let _ = state.db.insert_download(&task.task_id, &bvid, "", &url);

    let task_id = task.task_id.clone();
    let shared_state = Arc::clone(state.inner());
    tauri::async_runtime::spawn(async move {
        run_download_task(task_id, bvid, shared_state, app).await;
    });

    Ok(task)
}

#[tauri::command]
fn get_download_task(
    task_id: String,
    state: State<'_, SharedState>,
) -> Result<DownloadTask, String> {
    get_task(&state, &task_id)?.ok_or_else(|| "下载任务不存在".to_string())
}

#[tauri::command]
async fn proxy_image(url: String) -> Result<String, String> {
    let parsed = Url::parse(&url).map_err(|error| format!("图片地址无效：{error}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "图片地址缺少域名".to_string())?;
    if !host.ends_with("hdslb.com") && !host.ends_with("biliimg.com") {
        return Err("只允许代理 Bilibili 图片".to_string());
    }

    let client = http_client()?;
    let response = client
        .get(parsed)
        .header(REFERER, "https://www.bilibili.com/")
        .send()
        .await
        .map_err(|error| format!("封面图片加载失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("封面图片加载失败：{error}"))?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取图片数据失败：{error}"))?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{content_type};base64,{b64}"))
}

async fn run_download_task(
    task_id: String,
    bvid: String,
    state: SharedState,
    app: tauri::AppHandle,
) {
    let _ = update_task(&state, &task_id, |task| {
        task.status = "running".to_string();
        task.progress = 1.0;
    });

    match download_video(&bvid, &task_id, &state, app).await {
        Ok((filename, title, file_size)) => {
            let _ = update_task(&state, &task_id, |task| {
                task.status = "succeeded".to_string();
                task.progress = 100.0;
                task.filename = Some(filename.clone());
                task.error = None;
            });
            // 更新 SQLite 记录
            let _ = state
                .db
                .mark_succeeded(&task_id, &filename, Some(file_size));
            // 回写标题到数据库
            if !title.is_empty() {
                let conn = state.db.update_title(&task_id, &title);
                let _ = conn;
            }
        }
        Err(error) => {
            let _ = update_task(&state, &task_id, |task| {
                task.status = "failed".to_string();
                task.error = Some(error.clone());
            });
            // 更新 SQLite 记录
            let _ = state.db.mark_failed(&task_id, &error);
        }
    }
}

async fn download_video(
    bvid: &str,
    task_id: &str,
    state: &SharedState,
    app: tauri::AppHandle,
) -> Result<(String, String, u64), String> {
    let client = http_client()?;
    let view = fetch_view(&client, bvid).await?;
    let download = fetch_play_url(&client, &view.bvid, view.cid).await?;
    let target_dir = app
        .path()
        .download_dir()
        .map_err(|error| format!("读取下载目录失败：{error}"))?
        .join("BilibiliCrawler");
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|error| format!("创建下载目录失败：{error}"))?;

    let extension = extension_from_url(&download.url);
    let filename = format!(
        "{}-{}.{}",
        sanitize_filename(&view.title),
        view.bvid,
        extension
    );
    let target_path = unique_path(target_dir.join(filename)).await;
    let response = client
        .get(&download.url)
        .header(REFERER, format!("https://www.bilibili.com/video/{bvid}"))
        .send()
        .await
        .map_err(|error| format!("视频下载请求失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("视频下载失败：{error}"))?;
    let total_size = response.content_length().or(download.size).unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&target_path)
        .await
        .map_err(|error| format!("创建视频文件失败：{error}"))?;
    let mut downloaded = 0_u64;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|error| format!("读取视频流失败：{error}"))?;
        file.write_all(&bytes)
            .await
            .map_err(|error| format!("写入视频文件失败：{error}"))?;
        downloaded += bytes.len() as u64;

        if total_size > 0 {
            let progress = (downloaded as f64 / total_size as f64 * 99.0).clamp(1.0, 99.0);
            let _ = update_task(state, task_id, |task| {
                task.progress = progress;
            });
        }
    }

    file.flush()
        .await
        .map_err(|error| format!("保存视频文件失败：{error}"))?;
    Ok((
        target_path.to_string_lossy().to_string(),
        view.title,
        downloaded,
    ))
}

async fn fetch_view(client: &reqwest::Client, bvid: &str) -> Result<ViewData, String> {
    let mut url = Url::parse("https://api.bilibili.com/x/web-interface/view")
        .map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("bvid", bvid);

    let response: BilibiliResponse<ViewData> = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("读取视频信息失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("读取视频信息失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("视频信息解析失败：{error}"))?;

    if response.code != 0 {
        return Err(response
            .message
            .unwrap_or_else(|| "读取视频信息失败".to_string()));
    }
    response.data.ok_or_else(|| "视频信息为空".to_string())
}

async fn fetch_play_url(
    client: &reqwest::Client,
    bvid: &str,
    cid: u64,
) -> Result<DownloadUrl, String> {
    let mut url = Url::parse("https://api.bilibili.com/x/player/playurl")
        .map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("bvid", bvid)
        .append_pair("cid", &cid.to_string())
        .append_pair("qn", "80")
        .append_pair("fnval", "0")
        .append_pair("fourk", "1");

    let response: BilibiliResponse<PlayData> = client
        .get(url)
        .header(REFERER, format!("https://www.bilibili.com/video/{bvid}"))
        .send()
        .await
        .map_err(|error| format!("读取播放地址失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("读取播放地址失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("播放地址解析失败：{error}"))?;

    if response.code != 0 {
        return Err(response
            .message
            .unwrap_or_else(|| "读取播放地址失败".to_string()));
    }
    response
        .data
        .and_then(|data| data.durl)
        .and_then(|mut urls| urls.drain(..).next())
        .ok_or_else(|| "没有可直接下载的视频流，可能需要登录 Cookie 或 DASH 合并支持".to_string())
}

fn http_client() -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(REFERER, HeaderValue::from_static(SEARCH_REFERER));

    reqwest::Client::builder()
        .default_headers(headers)
        .cookie_store(true)
        .build()
        .map_err(|error| format!("初始化 HTTP 客户端失败：{error}"))
}

/// 预热 Bilibili Cookie。
/// 先访问 bilibili.com 主站获取 buvid3/buvid4 等必要 Cookie，
/// 这些 Cookie 是通过 Bilibili 搜索 API 风控检测的前提条件。
async fn seed_bilibili_cookies(client: &reqwest::Client) -> Result<(), String> {
    // 访问主站首页以获取初始 Cookie
    let _response = client
        .get("https://www.bilibili.com")
        .header(REFERER, "https://www.bilibili.com/")
        .send()
        .await
        .map_err(|error| format!("预热 Bilibili Cookie 失败：{error}"))?;
    Ok(())
}

async fn signed_api_url(
    client: &reqwest::Client,
    base_url: &str,
    mut params: Vec<(&str, String)>,
) -> Result<Url, String> {
    let mixin_key = fetch_wbi_mixin_key(client).await?;
    let wts = unix_timestamp().to_string();
    params.push(("wts", wts));
    params.sort_by(|left, right| left.0.cmp(right.0));

    let query = encode_query(&params);
    let w_rid = format!("{:x}", md5::compute(format!("{query}{mixin_key}")));
    let signed_query = format!("{query}&w_rid={w_rid}");
    let mut url = Url::parse(base_url).map_err(|error| error.to_string())?;
    url.set_query(Some(&signed_query));
    Ok(url)
}

async fn fetch_wbi_mixin_key(client: &reqwest::Client) -> Result<String, String> {
    let response: BilibiliResponse<NavData> = client
        .get(
            Url::parse("https://api.bilibili.com/x/web-interface/nav")
                .map_err(|error| error.to_string())?,
        )
        .header(REFERER, "https://www.bilibili.com/")
        .send()
        .await
        .map_err(|error| format!("读取 Bilibili WBI 配置失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("读取 Bilibili WBI 配置失败：{error}"))?
        .json()
        .await
        .map_err(|error| format!("Bilibili WBI 配置解析失败：{error}"))?;

    let data = response.data.ok_or_else(|| {
        response
            .message
            .unwrap_or_else(|| "Bilibili WBI 配置为空".to_string())
    })?;
    let raw_key = format!(
        "{}{}",
        filename_stem_from_url(&data.wbi_img.img_url)?,
        filename_stem_from_url(&data.wbi_img.sub_url)?
    );
    let mixin_key = MIXIN_KEY_ENC_TAB
        .iter()
        .filter_map(|index| raw_key.as_bytes().get(*index).copied())
        .take(32)
        .map(char::from)
        .collect::<String>();

    if mixin_key.len() != 32 {
        return Err("Bilibili WBI key 无效".to_string());
    }
    Ok(mixin_key)
}

fn filename_stem_from_url(value: &str) -> Result<String, String> {
    let url = Url::parse(value).map_err(|error| format!("Bilibili WBI 图片地址无效：{error}"))?;
    let filename = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .ok_or_else(|| "Bilibili WBI 图片地址缺少文件名".to_string())?;
    Ok(filename
        .split('.')
        .next()
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| "Bilibili WBI 图片地址文件名无效".to_string())?
        .to_string())
}

fn encode_query(params: &[(&str, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, &sanitize_wbi_value(value));
    }
    serializer.finish()
}

fn sanitize_wbi_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn to_search_result(item: SearchItem) -> SearchResult {
    let title = clean_html(&item.title);
    let url = item
        .arcurl
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("https://www.bilibili.com/video/{}", item.bvid));

    SearchResult {
        id: item.bvid,
        title,
        url,
        uploader: item.author,
        duration: item.duration.as_deref().and_then(parse_duration),
        view_count: item.play,
        thumbnail: item.pic.map(normalize_url),
    }
}

fn parse_duration(value: &str) -> Option<f64> {
    let parts = value
        .split(':')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let seconds = parts.into_iter().fold(0_u64, |total, part| {
        total.saturating_mul(60).saturating_add(part)
    });
    Some(seconds as f64)
}

fn normalize_url(value: String) -> String {
    if value.starts_with("//") {
        format!("https:{value}")
    } else {
        value
    }
}

fn clean_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
}

fn extract_bvid(value: &str) -> Result<String, String> {
    let input = value.trim();
    if input.is_empty() {
        return Err("视频地址不能为空".to_string());
    }

    // 大小写不敏感搜索 "BV"
    let upper = input.to_uppercase();
    for (index, _) in upper.match_indices("BV") {
        let candidate = input[index..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric())
            .collect::<String>();
        if candidate.len() >= 12 {
            return Ok(candidate.chars().take(12).collect());
        }
    }

    Err("请输入 Bilibili 视频链接或 BV 号".to_string())
}

fn sanitize_filename(value: &str) -> String {
    let clean = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            _ if character.is_control() => '-',
            _ => character,
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(90)
        .collect::<String>();

    if clean.is_empty() {
        "bilibili-video".to_string()
    } else {
        clean
    }
}

fn extension_from_url(value: &str) -> &'static str {
    if value.contains(".flv") {
        "flv"
    } else {
        "mp4"
    }
}

async fn unique_path(path: PathBuf) -> PathBuf {
    if !matches!(tokio::fs::try_exists(&path).await, Ok(true)) {
        return path;
    }

    let parent = path.parent().map(PathBuf::from).unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("video");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");

    for index in 1..1000 {
        let next = parent.join(format!("{stem}-{index}.{extension}"));
        if !matches!(tokio::fs::try_exists(&next).await, Ok(true)) {
            return next;
        }
    }

    path
}

fn insert_task(state: &SharedState, task: DownloadTask) -> Result<(), String> {
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "下载任务状态锁已损坏".to_string())?;
    tasks.insert(task.task_id.clone(), task);
    Ok(())
}

fn get_task(state: &SharedState, task_id: &str) -> Result<Option<DownloadTask>, String> {
    let tasks = state
        .tasks
        .lock()
        .map_err(|_| "下载任务状态锁已损坏".to_string())?;
    Ok(tasks.get(task_id).cloned())
}

fn update_task(
    state: &SharedState,
    task_id: &str,
    update: impl FnOnce(&mut DownloadTask),
) -> Result<(), String> {
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "下载任务状态锁已损坏".to_string())?;
    let task = tasks
        .get_mut(task_id)
        .ok_or_else(|| "下载任务不存在".to_string())?;
    update(task);
    Ok(())
}

#[tauri::command]
fn get_download_history(
    state: State<'_, SharedState>,
    limit: Option<u32>,
) -> Result<Vec<DownloadRecord>, String> {
    state.db.list_downloads(limit.unwrap_or(50))
}

#[tauri::command]
fn delete_download_record(state: State<'_, SharedState>, id: i64) -> Result<(), String> {
    state.db.delete_download(id)
}

#[tauri::command]
fn clear_download_history(state: State<'_, SharedState>) -> Result<(), String> {
    state.db.clear_downloads()
}

#[tauri::command]
async fn reveal_file(path: String) -> Result<(), String> {
    #[cfg(mobile)]
    {
        let _ = path;
        return Err("移动端不支持打开文件所在位置".to_string());
    }

    #[cfg(desktop)]
    {
        let file_path = std::path::Path::new(&path);
        if !file_path.exists() {
            return Err("文件不存在，可能已被移动或删除".to_string());
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&path)
                .spawn()
                .map_err(|e| format!("打开文件位置失败：{e}"))?;
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", &path))
                .spawn()
                .map_err(|e| format!("打开文件位置失败：{e}"))?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(file_path.parent().unwrap_or(file_path))
                .spawn()
                .map_err(|e| format!("打开文件位置失败：{e}"))?;
        }
        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir().expect("无法获取应用数据目录");
            let db_path = app_data_dir.join("downloads.db");
            let db = Database::open(&db_path).expect("初始化数据库失败");
            let state = Arc::new(AppState {
                tasks: Mutex::new(HashMap::new()),
                db,
            });
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_status,
            search_videos,
            create_download,
            get_download_task,
            proxy_image,
            get_download_history,
            delete_download_record,
            clear_download_history,
            reveal_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
