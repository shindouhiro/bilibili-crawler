use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, COOKIE, REFERER, USER_AGENT},
    StatusCode, Url,
};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
const ACCEPT_VALUE: &str = "application/json, text/plain, */*";
const ACCEPT_LANGUAGE_VALUE: &str = "zh-CN,zh;q=0.9,en;q=0.8";
const SEARCH_REFERER: &str = "https://search.bilibili.com/";
const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

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
    pub result: Vec<SearchItem>,
}

#[derive(Deserialize)]
pub struct SearchItem {
    pub bvid: String,
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub duration: Option<String>,
    #[serde(default)]
    pub play: Option<u64>,
    #[serde(default)]
    pub pic: Option<String>,
    #[serde(default)]
    pub arcurl: Option<String>,
}

#[derive(Deserialize)]
pub struct ViewData {
    pub bvid: String,
    pub cid: u64,
    pub title: String,
}

#[derive(Deserialize)]
struct PlayData {
    durl: Option<Vec<DownloadUrl>>,
}

#[derive(Deserialize)]
pub struct DownloadUrl {
    pub url: String,
    #[serde(default)]
    pub size: Option<u64>,
}

pub struct BilibiliClient {
    client: reqwest::Client,
}

impl BilibiliClient {
    pub fn new() -> Result<Self, String> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        headers.insert(ACCEPT, HeaderValue::from_static(ACCEPT_VALUE));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static(ACCEPT_LANGUAGE_VALUE));
        headers.insert(REFERER, HeaderValue::from_static(SEARCH_REFERER));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .cookie_store(true)
            .build()
            .map_err(|e| format!("初始化 HTTP 客户端失败：{e}"))?;

        Ok(Self { client })
    }

    pub async fn search(&self, keyword: &str, page: u32, page_size: u32) -> Result<Vec<SearchItem>, String> {
        let cookie = bilibili_cookie_header();
        seed_bilibili_cookies(&self.client, &cookie).await?;

        let search_url = signed_api_url(
            &self.client,
            &cookie,
            "https://api.bilibili.com/x/web-interface/search/type",
            vec![
                ("search_type", "video".to_string()),
                ("keyword", keyword.to_string()),
                ("page", page.to_string()),
                ("page_size", page_size.to_string()),
            ],
        ).await?;

        let response: BilibiliResponse<SearchData> = self.client
            .get(search_url)
            .header(REFERER, SEARCH_REFERER)
            .header(COOKIE, &cookie)
            .send()
            .await
            .map_err(|e| format!("Bilibili 搜索失败：{e}"))?
            .error_for_status()
            .map_err(|e| map_bilibili_http_error(e, "Bilibili 搜索失败"))?
            .json()
            .await
            .map_err(|e| format!("Bilibili 搜索响应解析失败：{e}"))?;

        if response.code != 0 {
            return Err(response.message.unwrap_or_else(|| "Bilibili 搜索失败".to_string()));
        }

        Ok(response.data.map(|d| d.result).unwrap_or_default())
    }

    pub async fn proxy_image_bytes(&self, url: &str) -> Result<(HeaderValue, bytes::Bytes), String> {
        let response = self.client
            .get(url)
            .header(REFERER, "https://www.bilibili.com/")
            .header(COOKIE, bilibili_cookie_header())
            .send()
            .await
            .map_err(|e| format!("封面图片加载失败：{e}"))?
            .error_for_status()
            .map_err(|e| map_bilibili_http_error(e, "封面图片加载失败"))?;

        let content_type = response
            .headers()
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("image/jpeg"));

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("读取图片数据失败：{e}"))?;

        Ok((content_type, bytes))
    }

    pub async fn fetch_view(&self, bvid: &str) -> Result<ViewData, String> {
        let cookie = bilibili_cookie_header();
        let mut url = Url::parse("https://api.bilibili.com/x/web-interface/view").unwrap();
        url.query_pairs_mut().append_pair("bvid", bvid);

        let response: BilibiliResponse<ViewData> = self.client
            .get(url)
            .header(REFERER, format!("https://www.bilibili.com/video/{bvid}"))
            .header(COOKIE, &cookie)
            .send()
            .await
            .map_err(|e| format!("读取视频信息失败：{e}"))?
            .error_for_status()
            .map_err(|e| map_bilibili_http_error(e, "读取视频信息失败"))?
            .json()
            .await
            .map_err(|e| format!("视频信息解析失败：{e}"))?;

        if response.code != 0 {
            return Err(response.message.unwrap_or_else(|| "读取视频信息失败".to_string()));
        }
        response.data.ok_or_else(|| "视频信息为空".to_string())
    }

    pub async fn fetch_play_url(&self, bvid: &str, cid: u64) -> Result<DownloadUrl, String> {
        let cookie = bilibili_cookie_header();
        let mut url = Url::parse("https://api.bilibili.com/x/player/playurl").unwrap();
        url.query_pairs_mut()
            .append_pair("bvid", bvid)
            .append_pair("cid", &cid.to_string())
            .append_pair("qn", "80")
            .append_pair("fnval", "0")
            .append_pair("fourk", "1");

        let response: BilibiliResponse<PlayData> = self.client
            .get(url)
            .header(REFERER, format!("https://www.bilibili.com/video/{bvid}"))
            .header(COOKIE, &cookie)
            .send()
            .await
            .map_err(|e| format!("读取播放地址失败：{e}"))?
            .error_for_status()
            .map_err(|e| map_bilibili_http_error(e, "读取播放地址失败"))?
            .json()
            .await
            .map_err(|e| format!("播放地址解析失败：{e}"))?;

        if response.code != 0 {
            return Err(response.message.unwrap_or_else(|| "读取播放地址失败".to_string()));
        }
        response.data
            .and_then(|d| d.durl)
            .and_then(|mut urls| urls.drain(..).next())
            .ok_or_else(|| "没有可直接下载的视频流".to_string())
    }

    pub async fn download_stream(&self, url: &str, bvid: &str) -> Result<reqwest::Response, String> {
        self.client
            .get(url)
            .header(REFERER, format!("https://www.bilibili.com/video/{bvid}"))
            .header(COOKIE, bilibili_cookie_header())
            .send()
            .await
            .map_err(|e| format!("视频下载请求失败：{e}"))?
            .error_for_status()
            .map_err(|e| map_bilibili_http_error(e, "视频下载失败"))
    }
}

async fn seed_bilibili_cookies(client: &reqwest::Client, cookie: &str) -> Result<(), String> {
    let _ = client
        .get("https://www.bilibili.com")
        .header(REFERER, "https://www.bilibili.com/")
        .header(COOKIE, cookie)
        .send()
        .await
        .map_err(|e| format!("预热 Bilibili Cookie 失败：{e}"))?;
    Ok(())
}

fn bilibili_cookie_header() -> String {
    let timestamp = unix_timestamp();
    let buvid = Uuid::new_v4().simple().to_string().to_uppercase();
    let uuid = Uuid::new_v4().simple().to_string().to_uppercase();
    let b_lsid = Uuid::new_v4().simple().to_string().chars().take(16).collect::<String>().to_uppercase();

    format!("buvid3={buvid}infoc; buvid4={buvid}-{timestamp}; b_nut={timestamp}; _uuid={uuid}; b_lsid={b_lsid}_{timestamp}; enable_web_push=DISABLE")
}

fn map_bilibili_http_error(error: reqwest::Error, context: &str) -> String {
    if error.status() == Some(StatusCode::PRECONDITION_FAILED) {
        return format!("{context}：Bilibili 返回 412 风控拦截。请稍后重试，或切换网络后再试。");
    }
    match error.status() {
        Some(status) => format!("{context}：HTTP {status}"),
        None => format!("{context}：{error}"),
    }
}

async fn signed_api_url(client: &reqwest::Client, cookie: &str, base_url: &str, mut params: Vec<(&str, String)>) -> Result<Url, String> {
    let mixin_key = fetch_wbi_mixin_key(client, cookie).await?;
    let wts = unix_timestamp().to_string();
    params.push(("wts", wts));
    params.sort_by(|l, r| l.0.cmp(r.0));

    let query = encode_query(&params);
    let w_rid = format!("{:x}", md5::compute(format!("{query}{mixin_key}")));
    let signed_query = format!("{query}&w_rid={w_rid}");
    let mut url = Url::parse(base_url).unwrap();
    url.set_query(Some(&signed_query));
    Ok(url)
}

async fn fetch_wbi_mixin_key(client: &reqwest::Client, cookie: &str) -> Result<String, String> {
    let response: BilibiliResponse<NavData> = client
        .get("https://api.bilibili.com/x/web-interface/nav")
        .header(REFERER, "https://www.bilibili.com/")
        .header(COOKIE, cookie)
        .send()
        .await
        .map_err(|e| format!("读取 Bilibili WBI 配置失败：{e}"))?
        .error_for_status()
        .map_err(|e| map_bilibili_http_error(e, "读取 Bilibili WBI 配置失败"))?
        .json()
        .await
        .map_err(|e| format!("Bilibili WBI 配置解析失败：{e}"))?;

    let data = response.data.ok_or_else(|| "Bilibili WBI 配置为空".to_string())?;
    let raw_key = format!(
        "{}{}",
        filename_stem_from_url(&data.wbi_img.img_url)?,
        filename_stem_from_url(&data.wbi_img.sub_url)?
    );
    let mixin_key: String = MIXIN_KEY_ENC_TAB
        .iter()
        .filter_map(|&idx| raw_key.as_bytes().get(idx).copied())
        .take(32)
        .map(char::from)
        .collect();

    if mixin_key.len() != 32 {
        return Err("Bilibili WBI key 无效".to_string());
    }
    Ok(mixin_key)
}

fn filename_stem_from_url(value: &str) -> Result<String, String> {
    let url = Url::parse(value).map_err(|e| format!("Bilibili WBI 图片地址无效：{e}"))?;
    let filename = url.path_segments().and_then(|mut s| s.next_back()).ok_or_else(|| "Bilibili WBI 图片地址缺少文件名".to_string())?;
    Ok(filename.split('.').next().filter(|s| !s.is_empty()).ok_or_else(|| "Bilibili WBI 图片地址文件名无效".to_string())?.to_string())
}

fn encode_query(params: &[(&str, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        serializer.append_pair(k, &sanitize_wbi_value(v));
    }
    serializer.finish()
}

fn sanitize_wbi_value(value: &str) -> String {
    value.chars().filter(|c| !matches!(c, '!' | '\'' | '(' | ')' | '*')).collect()
}

fn unix_timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn clean_html(value: &str) -> String {
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
    output.replace("&quot;", "\"").replace("&amp;", "&").replace("&#39;", "'")
}

pub fn parse_duration(value: &str) -> Option<f64> {
    let parts: Option<Vec<u64>> = value.split(':').map(|p| p.parse().ok()).collect();
    parts.map(|p| p.into_iter().fold(0_u64, |total, part| total.saturating_mul(60).saturating_add(part)) as f64)
}

pub fn extract_bvid(value: &str) -> Result<String, String> {
    let input = value.trim();
    if input.is_empty() { return Err("视频地址不能为空".to_string()); }
    let upper = input.to_uppercase();
    for (index, _) in upper.match_indices("BV") {
        let candidate: String = input[index..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if candidate.len() >= 12 { return Ok(candidate.chars().take(12).collect()); }
    }
    Err("请输入 Bilibili 视频链接或 BV 号".to_string())
}

pub fn sanitize_filename(value: &str) -> String {
    let clean: String = value.chars().map(|c| match c {
        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
        _ if c.is_control() => '-',
        _ => c,
    }).collect::<String>().trim().chars().take(90).collect();
    if clean.is_empty() { "bilibili-video".to_string() } else { clean }
}
