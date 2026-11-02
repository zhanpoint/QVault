use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, COOKIE, LOCATION, ORIGIN, PRAGMA, REFERER, USER_AGENT,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use crate::commands::{archive::runtime::Session, qlogin::QLoginState};

#[path = "qzone/history.rs"]
mod history;
pub(crate) use history::parse_history_timestamp;
#[cfg(test)]
use history::{decode_history_html, history_html_as_feeds, mood_cell_id_published_at};

const FEEDS_URL: &str = "https://mobile.qzone.qq.com/get_feeds";
const HISTORY_MESSAGES_URL: &str =
    "https://user.qzone.qq.com/proxy/domain/ic2.qzone.qq.com/cgi-bin/feeds/feeds2_html_pav_all";
const DESKTOP_QZONE_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const FEED_RESPONSE_ATTEMPTS: u32 = 3;
const LIBRARY_ALBUMS_URL: &str =
    "https://user.qzone.qq.com/proxy/domain/photo.qzone.qq.com/fcgi-bin/fcg_list_album_v3";
const LIBRARY_PHOTOS_URL: &str =
    "https://user.qzone.qq.com/proxy/domain/photo.qzone.qq.com/fcgi-bin/cgi_list_photo";
const LIBRARY_VIDEOS_URL: &str =
    "https://user.qzone.qq.com/proxy/domain/taotao.qq.com/cgi-bin/video_get_data";
const LIBRARY_GUESTBOOK_URL: &str =
    "https://user.qzone.qq.com/proxy/domain/m.qzone.qq.com/cgi-bin/new/get_msgb";
const LIBRARY_FAVORITES_URL: &str =
    "https://user.qzone.qq.com/proxy/domain/fav.qzone.qq.com/cgi-bin/get_fav_list";
const TOKEN_PROBE_WINDOW_LABEL: &str = "qzone-page-token-probe";
const TOKEN_PROBE_TITLE_PREFIX: &str = "__QZA_QZONETOKEN__";

#[derive(Clone, Default)]
pub struct QzonePageTokenState {
    // Match the authenticated cookie session, including a re-login to the same UIN.
    token: Arc<Mutex<Option<(String, String)>>>,
    photo_key: Arc<Mutex<Option<(String, String)>>>,
}

impl QzonePageTokenState {
    pub(crate) fn photo_cookie(&self, cookies: &str) -> Result<Option<String>, String> {
        Ok(self
            .photo_key
            .lock()
            .map_err(|_| "照片签名状态锁已损坏")?
            .as_ref()
            .filter(|(session, _)| session == cookies)
            .map(|(_, key)| key.clone()))
    }

    fn cached_for(&self, cookies: &str) -> Result<Option<String>, String> {
        Ok(self
            .token
            .lock()
            .map_err(|_| "页面令牌状态锁已损坏")?
            .as_ref()
            .filter(|(session, token)| session == cookies && !token.is_empty())
            .map(|(_, token)| token.clone()))
    }
}

fn extract_qzone_token(page: &str) -> Option<String> {
    // QQ writes this token into an inline script on the authenticated desktop
    // homepage. Keep the parser deliberately narrow so we never mistake a
    // different script value for an authorization token.
    let compact = page.replace(char::is_whitespace, "");
    regex::Regex::new(
        r#"window\.g_qzonetoken=\(function\(\)\{.*?return[\"']([A-Za-z0-9_-]+)[\"'];"#,
    )
    .ok()?
    .captures(&compact)
    .and_then(|captures| captures.get(1))
    .map(|token| token.as_str().to_owned())
    .filter(|token| !token.is_empty())
}

fn extract_qzone_token_expression(page: &str) -> Option<String> {
    // Some Qzone page variants return the token as an obfuscated JavaScript
    // expression instead of a quoted literal. Preserve only the expression
    // after `try { return ...; }` and execute it inside an isolated WebView on
    // the authenticated Qzone origin. It is bounded and never exposed to the
    // application frontend.
    let expression = regex::Regex::new(
        r#"(?s)window\s*\.\s*g_qzonetoken\s*=\s*\(\s*function\s*\(\s*\)\s*\{\s*try\s*\{\s*return\s*(.+?)\s*;\s*\}\s*catch"#,
    )
    .ok()?
    .captures(page)?
    .get(1)?
    .as_str()
    .trim();
    (!expression.is_empty() && expression.len() <= 64 * 1024).then(|| expression.to_owned())
}

struct TokenProbe(tauri::WebviewWindow);
impl Drop for TokenProbe {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}

async fn fetch_qzone_token_from_webview(
    app: &tauri::AppHandle,
    auth: &crate::commands::qlogin::QzoneAuth,
    token_state: &QzonePageTokenState,
    page_expression: Option<&str>,
) -> Option<String> {
    if let Some(token) = token_state.cached_for(&auth.cookie_header).ok().flatten() {
        return Some(token);
    }
    if let Some(existing) = app.get_webview_window(TOKEN_PROBE_WINDOW_LABEL) {
        existing.close().ok();
    }
    let page_url = Url::parse(&format!("https://user.qzone.qq.com/{}", auth.uin)).ok()?;
    let encoded_expression = page_expression
        .map(|expression| BASE64.encode(expression.as_bytes()))
        .unwrap_or_default();
    let probe_script = format!(
        r#"
        (() => {{
          if (window.__qzaTokenProbeInstalled) return;
          window.__qzaTokenProbeInstalled = true;
          const publish = (value) => {{
            const token = typeof value === 'string' ? value.trim() : '';
            if (!/^[A-Za-z0-9_-]+$/.test(token)) return false;
            document.title = '{TOKEN_PROBE_TITLE_PREFIX}' + token;
            try {{
              window.location.replace('qza-qzonetoken://capture/' + encodeURIComponent(token));
            }} catch (_) {{}}
            return true;
          }};
          const inspect = () => {{
            try {{ if (publish(window.g_qzonetoken)) return; }} catch (_) {{}}
            try {{
              const encoded = '{encoded_expression}';
              if (encoded) {{
                const expression = atob(encoded);
                if (publish((0, eval)('(' + expression + ')'))) return;
              }}
            }} catch (_) {{}}
            try {{
              for (const script of document.scripts || []) {{
                const compact = String(script.textContent || '').replace(/\s+/g, '');
                if (!compact.includes('window.g_qzonetoken')) continue;
                const match = compact.match(/return[\"']([A-Za-z0-9_-]+)[\"'];/);
                if (match && publish(match[1])) return;
              }}
            }} catch (_) {{}}
          }};
          inspect();
          const timer = setInterval(inspect, 200);
          setTimeout(() => clearInterval(timer), 12000);
          document.addEventListener('DOMContentLoaded', inspect, {{ once: true }});
        }})();
        "#
    );
    let capture_state = token_state.clone();
    let capture_cookie_header = auth.cookie_header.clone();
    let window = WebviewWindowBuilder::new(
        app,
        TOKEN_PROBE_WINDOW_LABEL,
        WebviewUrl::External(page_url.clone()),
    )
    .title("QQ 空间会话校验")
    .visible(false)
    .initialization_script(&probe_script)
    .on_navigation(move |url| {
        if url.scheme() == "qza-qzonetoken" && url.host_str() == Some("capture") {
            if let Some(token) = url.path_segments().and_then(|mut segments| segments.next()) {
                if !token.is_empty()
                    && token
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                {
                    if let Ok(mut guard) = capture_state.token.lock() {
                        *guard = Some((capture_cookie_header.clone(), token.to_owned()));
                    }
                }
            }
            return false;
        }
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host == "qzone.qq.com" || host.ends_with(".qzone.qq.com"))
    })
    .build()
    .ok()?;
    let probe = TokenProbe(window);
    let window = &probe.0;
    // The login session is already narrowed to QQ's authentication cookies.
    // Populate the shared WebKit/WebView cookie store before reloading the
    // authenticated page; the token never leaves this native process.
    for pair in auth.cookie_header.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let cookie_text = format!("{name}={value}; Domain=.qq.com; Path=/; Secure");
        if let Ok(cookie) = cookie_text.parse::<cookie::Cookie>() {
            window.set_cookie(cookie).ok();
        }
    }
    window.navigate(page_url).ok();
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        if let Ok(title) = window.title() {
            if let Some(token) = title.strip_prefix(TOKEN_PROBE_TITLE_PREFIX) {
                if !token.is_empty()
                    && token
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
                {
                    let token = token.to_owned();
                    if let Ok(mut guard) = token_state.token.lock() {
                        *guard = Some((auth.cookie_header.clone(), token.clone()));
                    }
                    return Some(token);
                }
            }
        }
        if let Some(token) = token_state.cached_for(&auth.cookie_header).ok().flatten() {
            return Some(token);
        }
    }
    None
}

async fn fetch_qzone_token(
    app: &tauri::AppHandle,
    state: &QLoginState,
    auth: &crate::commands::qlogin::QzoneAuth,
    token_state: &QzonePageTokenState,
    session: Option<&Session>,
) -> Result<Option<String>, String> {
    if let Some(token) = token_state.cached_for(&auth.cookie_header)? {
        return Ok(Some(token));
    }
    // Different desktop shells are enabled for different QQ accounts.  Probe
    // only known same-origin entry paths and stop as soon as one exposes the
    // page token.  No page body or token is logged or returned to the UI.
    let candidates = [
        format!("https://user.qzone.qq.com/{}", auth.uin),
        format!("https://user.qzone.qq.com/{}/main", auth.uin),
        format!("https://user.qzone.qq.com/{}/infocenter", auth.uin),
    ];
    let mut page_expression = None;
    for home_url in candidates {
        if let Some(session) = session {
            session.before_request().await?;
        }
        let Ok(mut response) = state
            .client()
            .get(&home_url)
            .header(ACCEPT, "text/html,application/xhtml+xml")
            .header(REFERER, "https://qzone.qq.com/")
            .header(USER_AGENT, DESKTOP_QZONE_USER_AGENT)
            // Match an authenticated browser request.  The persisted session
            // is already restricted to the small allow-list in qlogin.rs.
            .header(COOKIE, &auth.cookie_header)
            .send()
            .await
        else {
            continue;
        };
        if let Some(session) = session {
            session.observe(&response).await?;
        }
        // The shared client disables automatic redirects for login safety.
        // Follow only HTTPS redirects that stay on user.qzone.qq.com.
        for _ in 0..3 {
            if !response.status().is_redirection() {
                break;
            }
            let Some(location) = response.headers().get(LOCATION) else {
                break;
            };
            let Some(next) = location.to_str().ok().and_then(|location| {
                Url::parse(response.url().as_str())
                    .ok()?
                    .join(location)
                    .ok()
            }) else {
                break;
            };
            if next.scheme() != "https" || next.host_str() != Some("user.qzone.qq.com") {
                break;
            }
            if let Some(session) = session {
                session.before_request().await?;
            }
            let Ok(next_response) = state
                .client()
                .get(next)
                .header(ACCEPT, "text/html,application/xhtml+xml")
                .header(REFERER, &home_url)
                .header(USER_AGENT, DESKTOP_QZONE_USER_AGENT)
                .header(COOKIE, &auth.cookie_header)
                .send()
                .await
            else {
                break;
            };
            if let Some(session) = session {
                session.observe(&next_response).await?;
            }
            response = next_response;
        }
        if response.status().is_success() {
            if let Ok(page) = response.text().await {
                if let Some(token) = extract_qzone_token(&page) {
                    *token_state
                        .token
                        .lock()
                        .map_err(|_| "页面令牌状态锁已损坏")? =
                        Some((auth.cookie_header.clone(), token.clone()));
                    return Ok(Some(token));
                }
                if page_expression.is_none() {
                    page_expression = extract_qzone_token_expression(&page);
                }
            }
        }
    }
    if let Some(session) = session {
        session.before_request().await?;
    }
    Ok(fetch_qzone_token_from_webview(app, auth, token_state, page_expression.as_deref()).await)
}

fn parse_qzone_json(text: &str) -> Result<Value, String> {
    let normalized = text.trim().trim_start_matches('\u{feff}').trim();
    if normalized.is_empty() {
        return Err("QQ 空间返回了空响应".into());
    }
    if let Ok(value) = serde_json::from_str(normalized) {
        return Ok(value);
    }
    // The JSON decoder determines the object's end, including nested objects
    // and braces inside strings. Skip parsed spans rather than enumerating
    // every start/end pair in a JSONP or HTML envelope.
    let mut cursor = 0;
    let mut fallback = None;
    while let Some(relative) = normalized[cursor..].find('{') {
        let start = cursor + relative;
        let mut values =
            serde_json::Deserializer::from_str(&normalized[start..]).into_iter::<Value>();
        match values.next() {
            Some(Ok(value)) => {
                cursor = start + values.byte_offset();
                if value.get("code").is_some() {
                    return Ok(value);
                }
                fallback = Some(value);
            }
            _ => cursor = start + 1,
        }
    }
    if let Some(value) = fallback {
        return Ok(value);
    }
    Err(format!(
        "解析 QQ 空间响应失败：响应片段：{}",
        normalized.chars().take(180).collect::<String>()
    ))
}

fn ensure_qzone_success(value: Value) -> Result<Value, String> {
    let code = value
        .get("code")
        .and_then(|code| {
            code.as_i64()
                .or_else(|| code.as_str().and_then(|text| text.parse().ok()))
        })
        .ok_or("QQ 空间响应缺少 code 字段")?;
    if code == 0 {
        return Ok(value);
    }
    if code == -3000 {
        return Err(
            "ARCHIVE_AUTH_EXPIRED:QQ 空间登录已失效，请重新扫码后继续，已保存进度保留".into(),
        );
    }
    let message = value
        .get("message")
        .or_else(|| value.get("msg"))
        .and_then(Value::as_str)
        .unwrap_or("未知错误");
    Err(format!("QQ 空间接口返回错误 {code}：{message}"))
}

pub(crate) fn library_page_size(module: &str) -> usize {
    match module {
        "albums" => 100,
        "favorites" => 30,
        "guestbook" | "videos" => 20,
        _ => 100,
    }
}

pub(crate) async fn fetch_library_page(
    app: &tauri::AppHandle,
    state: &QLoginState,
    token_state: &QzonePageTokenState,
    module: &str,
    offset: usize,
    parent_key: Option<&str>,
    session: Option<&Session>,
) -> Result<Value, String> {
    let auth = state.qzone_auth().await?;
    if let Some(session) = session {
        session.validate_owner(&auth.uin)?;
    }
    // Several legacy Qzone endpoints reject oversized batches instead of
    // clamping them. In particular, Favorites returns -4003 for num=100;
    // QZoneExport's proven value is 30.
    let page_size = library_page_size(module);
    let mut common = vec![
        ("uin", auth.uin.clone()),
        ("hostUin", auth.uin.clone()),
        ("inCharset", "utf-8".into()),
        ("outCharset", "utf-8".into()),
        ("format", "jsonp".into()),
        ("g_tk", auth.g_tk.to_string()),
    ];
    if module == "guestbook" {
        // Reuse the desktop page token for the guestbook. Favorites follows
        // the maintained cookie + g_tk contract and needs no page-token request.
        let fetch = fetch_qzone_token(app, state, &auth, token_state, session);
        let qzone_token = match session {
            Some(session) => session.page_token(fetch).await?,
            None => fetch.await?,
        };
        if let Some(token) = qzone_token.as_ref().filter(|token| !token.is_empty()) {
            common.push(("qzonetoken", token.clone()));
        }
    }
    let (url, mut query) = match module {
        "albums" => (
            LIBRARY_ALBUMS_URL,
            vec![
                ("appid", "4".into()),
                ("source", "qzone".into()),
                ("plat", "qzone".into()),
                ("notice", "0".into()),
                ("filter", "1".into()),
                ("handset", "4".into()),
                ("needUserInfo", "1".into()),
                ("mode", "2".into()),
                ("sortOrder", "2".into()),
                ("pageStart", offset.to_string()),
                ("pageNum", page_size.to_string()),
            ],
        ),
        "photos" => (
            LIBRARY_PHOTOS_URL,
            vec![
                ("mode", "0".into()),
                ("topicId", parent_key.unwrap_or_default().into()),
                ("noTopic", "0".into()),
                ("pageStart", offset.to_string()),
                ("pageNum", page_size.to_string()),
                ("skipCmtCount", "0".into()),
                ("singleurl", "1".into()),
                ("batchId", String::new()),
                ("notice", "0".into()),
                ("appid", "4".into()),
                ("source", "qzone".into()),
                ("plat", "qzone".into()),
                ("outstyle", "json".into()),
                ("json_esc", "1".into()),
            ],
        ),
        "videos" => (
            LIBRARY_VIDEOS_URL,
            vec![
                ("appid", "4".into()),
                ("getMethod", "2".into()),
                ("start", offset.to_string()),
                ("count", page_size.to_string()),
                ("need_old", "1".into()),
                ("getUserInfo", "1".into()),
                ("refer", "qzone".into()),
                ("source", "qzone".into()),
            ],
        ),
        "guestbook" => (
            LIBRARY_GUESTBOOK_URL,
            vec![
                ("start", offset.to_string()),
                ("num", page_size.to_string()),
                ("hostword", "0".into()),
                ("essence", "1".into()),
                ("r", format!("0.{}", now_millis())),
                ("iNotice", "0".into()),
                ("ref", "qzone".into()),
            ],
        ),
        "favorites" => (
            LIBRARY_FAVORITES_URL,
            vec![
                ("type", "0".into()),
                ("start", offset.to_string()),
                ("num", page_size.to_string()),
                ("need_nick", "1".into()),
                ("need_cnt", if offset == 0 { "1" } else { "0" }.into()),
                ("need_new_user", if offset == 0 { "1" } else { "0" }.into()),
                ("fupdate", "1".into()),
                ("callback", "_Callback".into()),
                ("random", format!("0.{}", now_millis())),
            ],
        ),
        _ => return Err("不支持的归档模块".into()),
    };
    if module == "photos" && parent_key.is_none_or(str::is_empty) {
        return Err("读取照片前需要先选择相册".into());
    }
    query.extend(common);
    if module == "favorites" {
        // Match the currently maintained QzonePhoto request contract.  The
        // legacy Favorites CGI keys off `uin`; hostUin/format are unrelated
        // feed parameters and can change its response shape on some accounts.
        query.retain(|(key, _)| !matches!(*key, "hostUin" | "format"));
    }
    let referer = if module == "favorites" {
        format!("https://user.qzone.qq.com/{}/myhome/favorite", auth.uin)
    } else {
        format!("https://user.qzone.qq.com/{}", auth.uin)
    };
    let mut cookies = auth.cookie_header.clone();
    if module == "photos" {
        if let Some(key) = token_state.photo_cookie(&auth.cookie_header)? {
            cookies.push_str(&format!("; qq_photo_key={key}"));
        }
    }
    let request = state
        .client()
        .get(url)
        .header(ACCEPT, "application/json, text/javascript, */*; q=0.01")
        .header(REFERER, referer)
        .header(USER_AGENT, DESKTOP_QZONE_USER_AGENT)
        .header(COOKIE, cookies)
        .header(ORIGIN, "https://user.qzone.qq.com")
        .query(&query);
    // Read-only retries keep the same offset; every attempt uses the same gate.
    for attempt in 0..3 {
        if let Some(session) = session {
            session.before_request().await?;
        }
        let response = match request
            .try_clone()
            .ok_or("无法复用资料读取请求")?
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if attempt < 2 && (error.is_timeout() || error.is_connect()) => continue,
            Err(error) => return Err(format!("同步 QQ 空间资料失败：{error}")),
        };
        if let Some(session) = session {
            session.observe(&response).await?;
        }
        let status = response.status();
        if status.is_server_error() && attempt < 2 {
            continue;
        }
        if !status.is_success() {
            return Err(format!("同步 QQ 空间资料失败：HTTP {status}"));
        }
        if module == "photos" {
            for header in response.headers().get_all(reqwest::header::SET_COOKIE) {
                if let Ok(cookie) = header
                    .to_str()
                    .unwrap_or_default()
                    .parse::<cookie::Cookie>()
                {
                    if cookie.name() == "qq_photo_key" && !cookie.value().is_empty() {
                        *token_state
                            .photo_key
                            .lock()
                            .map_err(|_| "照片签名状态锁已损坏")? =
                            Some((auth.cookie_header.clone(), cookie.value().to_owned()));
                    }
                }
            }
        }
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) if attempt < 2 && (error.is_timeout() || error.is_body()) => continue,
            Err(error) => return Err(format!("读取 QQ 空间资料响应失败：{error}")),
        };
        if state.qzone_auth().await?.cookie_header != auth.cookie_header {
            return Err("ARCHIVE_ACCOUNT_CHANGED:登录会话已变化，已保留断点".into());
        }
        return tauri::async_runtime::spawn_blocking(move || {
            let text = std::str::from_utf8(&bytes)
                .map_err(|error| format!("资料响应编码无效：{error}"))?;
            ensure_qzone_success(parse_qzone_json(text)?)
        })
        .await
        .map_err(|error| format!("解析资料任务异常：{error}"))?;
    }
    unreachable!("every last-attempt branch returns")
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn feed_retry_delay(attempt: u32) -> std::time::Duration {
    let base = 1_500_u64.saturating_mul(1_u64 << attempt.saturating_sub(1).min(5));
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_millis() as u64;
    std::time::Duration::from_millis(base.min(30_000) + jitter)
}

fn sec_ch_ua(user_agent: &str) -> String {
    if let Some(start) = user_agent.find("Chrome/") {
        let version_start = start + 7;
        let major = user_agent[version_start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        let version = if major.is_empty() { "131" } else { &major };
        format!("\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"{version}\", \"Microsoft Edge\";v=\"{version}\"")
    } else {
        "\"Not;A=Brand\";v=\"8\", \"Apple\";v=\"0\", \"Safari\";v=\"18\"".to_owned()
    }
}

fn sec_platform(user_agent: &str) -> &'static str {
    if user_agent.contains("iPhone") {
        "\"iOS\""
    } else {
        "\"Android\""
    }
}
#[derive(Debug)]
pub(crate) struct FeedPage {
    pub(crate) feeds: Vec<Value>,
}

#[derive(Debug)]
pub(crate) struct HistoryMessagePage {
    pub(crate) feeds: Vec<Value>,
    pub(crate) record_count: usize,
}

fn usable_content_media_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let url = if value.starts_with("//") {
        format!("https:{value}")
    } else {
        value.to_owned()
    };
    let lower = url.to_ascii_lowercase();
    ((url.starts_with("http://") || url.starts_with("https://"))
        && !lower.contains("qlogo")
        && !lower.contains("headimg")
        && !lower.contains("qzonestyle")
        && !lower.contains("custompraise"))
    .then_some(url)
}

pub(crate) async fn fetch_history_messages(
    state: &QLoginState,
    offset: u64,
    count: u32,
    owner_name: Option<&str>,
    session: Option<&Session>,
) -> Result<HistoryMessagePage, String> {
    let auth = state.qzone_auth().await?;
    if let Some(session) = session {
        session.validate_owner(&auth.uin)?;
    }
    // The endpoint accepts a larger count while probing, but in live responses
    // it can return an empty body for count=100. Thirty is the largest window
    // we have verified to return the actual notification cards reliably.
    let count = count.clamp(1, 30);
    let query = vec![
        ("uin", auth.uin.clone()),
        ("begin_time", "0".into()),
        ("end_time", "0".into()),
        ("getappnotification", "1".into()),
        ("getnotifi", "1".into()),
        ("has_get_key", "0".into()),
        ("offset", offset.to_string()),
        ("set", "0".into()),
        ("count", count.to_string()),
        ("useutf8", "1".into()),
        ("outputhtmlfeed", "1".into()),
        ("scope", "1".into()),
        ("format", "jsonp".into()),
        ("g_tk", auth.g_tk.to_string()),
        ("g_tk", auth.g_tk.to_string()),
    ];
    let mut last_error = String::new();
    for attempt in 1..=FEED_RESPONSE_ATTEMPTS {
        if let Some(session) = session {
            session.before_request().await?;
        }
        let response = state
            .client()
            .get(HISTORY_MESSAGES_URL)
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
            )
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8,en-US;q=0.6")
            .header(CACHE_CONTROL, "no-cache")
            .header(PRAGMA, "no-cache")
            .header(REFERER, format!("https://user.qzone.qq.com/{}/main", auth.uin))
            .header(USER_AGENT, DESKTOP_QZONE_USER_AGENT)
            .header(COOKIE, &auth.desktop_cookie_header)
            .header("Sec-Ch-Ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"128\"")
            .header("Sec-Ch-Ua-Mobile", "?0")
            .header("Sec-Ch-Ua-Platform", "\"Linux\"")
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "same-origin")
            .header("Upgrade-Insecure-Requests", "1")
            .query(&query)
            .send()
            .await;
        match response {
            Ok(response) => {
                if let Some(session) = session {
                    session.observe(&response).await?;
                }
                let status = response.status();
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|error| format!("读取历史消息响应失败：{error}"))?;
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                    last_error = format!("HTTP {status}");
                } else if !status.is_success() {
                    return Err(format!("获取历史消息失败：HTTP {status}"));
                } else {
                    let owner = auth.uin.clone();
                    let name = owner_name.map(str::to_owned);
                    let result = tauri::async_runtime::spawn_blocking(move || {
                        let body = history::decode_history_response(&bytes);
                        if let Ok(value) = parse_qzone_json(&body) {
                            if value.get("code").is_some() {
                                ensure_qzone_success(value)?;
                            }
                        }
                        let html = history::decode_history_html(&body)?;
                        let (record_count, feeds) =
                            history::history_html_as_feeds(&html, &owner, name.as_deref());
                        Ok(HistoryMessagePage {
                            record_count,
                            feeds,
                        })
                    })
                    .await
                    .map_err(|error| format!("历史消息解析任务异常退出：{error}"))?;
                    if result
                        .as_ref()
                        .is_err_and(|error: &String| error.starts_with("ARCHIVE_AUTH_EXPIRED:"))
                    {
                        state.expire_auth(&auth).await?;
                    }
                    return result;
                }
            }
            Err(error) => last_error = error.to_string(),
        }
        if attempt < FEED_RESPONSE_ATTEMPTS {
            tokio::time::sleep(feed_retry_delay(attempt)).await;
        }
    }
    Err(format!("获取历史消息失败：{last_error}"))
}

fn parse_feed_page(mut value: Value) -> Result<FeedPage, String> {
    if let Some(code) = value
        .get("code")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
    {
        if code != 0 {
            let message = value
                .get("message")
                .or_else(|| value.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(format!("QQ 空间动态接口返回错误 {code}：{message}"));
        }
    }
    let data = value
        .get_mut("data")
        .and_then(Value::as_object_mut)
        .ok_or("动态响应中缺少 data")?;
    let server_has_more = match data.get("hasmore") {
        Some(Value::Bool(more)) => *more,
        Some(v) if v.as_i64() == Some(0) || v.as_str() == Some("0") => false,
        Some(v) if v.as_i64() == Some(1) || v.as_str() == Some("1") => true,
        _ => return Err("动态响应中缺少有效 hasmore，不能判定完成".into()),
    };
    let feeds = match data.remove("vFeeds") {
        Some(Value::Array(feeds)) => feeds,
        Some(Value::Null) if !server_has_more => Vec::new(),
        _ => return Err("动态响应中缺少有效 vFeeds，断点已保留".into()),
    };
    let attach_info = data
        .get("attachinfo")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if server_has_more && (feeds.is_empty() || attach_info.is_none()) {
        return Err("动态响应声明仍有数据，但缺少记录或下一页游标".into());
    }
    Ok(FeedPage { feeds })
}

pub(crate) async fn fetch_feeds(
    state: &QLoginState,
    refresh_type: &str,
    attach_info: Option<&str>,
    session: Option<&Session>,
) -> Result<FeedPage, String> {
    fetch_feeds_with_attempts(
        state,
        refresh_type,
        attach_info,
        FEED_RESPONSE_ATTEMPTS,
        session,
    )
    .await
}

pub(crate) fn feed_error_is_transient(error: &str) -> bool {
    error.starts_with("解析空间动态失败：")
        || error.starts_with("动态响应中缺少 data")
        || error.contains("HTTP 5")
        || error.contains("HTTP 429")
        || error.contains("请求超时")
        || error.contains("连接失败")
        || error.contains("传输失败")
        || error.contains("响应体读取失败")
        || ["系统繁忙", "稍后再试", "频繁", "限流"]
            .iter()
            .any(|keyword| error.contains(keyword))
}

async fn fetch_feeds_with_attempts(
    state: &QLoginState,
    refresh_type: &str,
    attach_info: Option<&str>,
    attempts: u32,
    session: Option<&Session>,
) -> Result<FeedPage, String> {
    let auth = state.qzone_auth().await?;
    if let Some(session) = session {
        session.validate_owner(&auth.uin)?;
    }
    let mut query = vec![
        ("g_tk", auth.g_tk.to_string()),
        ("res_type", "1".into()),
        ("refresh_type", refresh_type.into()),
        ("format", "json".into()),
    ];
    if let Some(attach_info) = attach_info {
        if attach_info.trim().is_empty() {
            let error = "分页游标不能为空";
            return Err(error.into());
        }
        query.push(("res_attach", attach_info.to_owned()));
    }
    let attempts = attempts.max(1);
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        if let Some(session) = session {
            session.before_request().await?;
        }
        let response = state
            .client()
            .get(FEEDS_URL)
            .header(ACCEPT, "application/json")
            .header(
                ACCEPT_LANGUAGE,
                "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6,zh-TW;q=0.5",
            )
            .header(CACHE_CONTROL, "no-cache")
            .header(PRAGMA, "no-cache")
            .header(ORIGIN, "https://h5.qzone.qq.com")
            .header(REFERER, "https://h5.qzone.qq.com/")
            .header(USER_AGENT, &auth.user_agent)
            .header(COOKIE, &auth.cookie_header)
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-site")
            .header("Sec-Ch-Ua", sec_ch_ua(&auth.user_agent))
            .header("Sec-Ch-Ua-Mobile", "?1")
            .header("Sec-Ch-Ua-Platform", sec_platform(&auth.user_agent))
            .query(&query)
            .send()
            .await;
        let parsed = match response {
            Ok(response) => {
                if let Some(session) = session {
                    session.observe(&response).await?;
                }
                let status = response.status();
                if !status.is_success() {
                    if status == reqwest::StatusCode::NOT_IMPLEMENTED {
                        return Err("互动补充接口不支持当前请求（HTTP 501），该位置保留待重试；不影响本人说说同步和历史恢复".into());
                    }
                    if !status.is_server_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS
                    {
                        return Err(format!("获取空间动态失败：HTTP {status}"));
                    }
                    Err(format!("获取空间动态失败：HTTP {status}"))
                } else {
                    match response.bytes().await {
                        Ok(bytes) => tauri::async_runtime::spawn_blocking(move || {
                            let value = serde_json::from_slice::<Value>(&bytes)
                                .map_err(|e| format!("解析空间动态失败：{e}"))?;
                            parse_feed_page(value)
                        })
                        .await
                        .map_err(|e| format!("解析空间动态失败：{e}"))?,
                        Err(error) => Err(format!("响应体读取失败：{error}")),
                    }
                }
            }
            Err(error) => {
                let kind = if error.is_timeout() {
                    "请求超时"
                } else if error.is_connect() {
                    "连接失败"
                } else {
                    "传输失败"
                };
                Err(format!("获取空间动态失败：{kind}：{error}"))
            }
        };
        match parsed {
            Ok(page) => return Ok(page),
            Err(error) => {
                if !feed_response_can_retry(&error) {
                    return Err(error);
                }
                last_error = error;
            }
        }
        if attempt < attempts {
            tokio::time::sleep(feed_retry_delay(attempt)).await;
        }
    }
    Err(last_error)
}

fn feed_response_can_retry(error: &str) -> bool {
    if feed_error_is_transient(error) {
        return true;
    }
    error.starts_with("QQ 空间动态接口返回错误")
        && !["未登录", "登录失效", "权限", "封禁", "禁止访问", "p_skey"]
            .iter()
            .any(|keyword| error.contains(keyword))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_history_html, ensure_qzone_success, extract_qzone_token,
        extract_qzone_token_expression, feed_response_can_retry, history_html_as_feeds,
        library_page_size, mood_cell_id_published_at, parse_feed_page, parse_qzone_json, FEEDS_URL,
    };
    use serde_json::json;

    #[test]
    fn login_rejection_is_distinct_from_retryable_api_errors() {
        let error =
            ensure_qzone_success(json!({"code":-3000,"message":"请先登录空间"})).unwrap_err();
        assert!(error.starts_with("ARCHIVE_AUTH_EXPIRED:"));
        assert!(!feed_response_can_retry(&error));
        assert!(
            !ensure_qzone_success(json!({"code":-1,"message":"系统繁忙"}))
                .unwrap_err()
                .starts_with("ARCHIVE_AUTH_EXPIRED:")
        );
    }

    #[test]
    fn history_uses_owner_links_without_a_portrait_nickname() {
        let html = r#"<li class="f-single f-s-s" data-key="fct_20001_217_3_1627745220_1_1"><p class="txt-box-title">旧昵称：一条历史说说</p><a href="https://user.qzone.qq.com/10001/mood/own-post">查看说说</a></li>
        <li class="f-single f-s-s" data-key="fct_20001_217_3_1627745230_1_1"><p class="txt-box-title">朋友：别人的说说</p><a href="https://user.qzone.qq.com/99999/mood/other-post">查看说说</a></li>"#;
        let (scanned, feeds) = history_html_as_feeds(html, "10001", None);
        assert_eq!(scanned, 2);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0]["original"]["cell_id"]["cellid"], "own-post");
        assert_eq!(
            feeds[0]["original"]["cell_summary"]["summary"],
            "旧昵称：一条历史说说"
        );
    }

    #[test]
    fn page_token_cache_is_bound_to_the_authenticated_session() {
        let state = super::QzonePageTokenState::default();
        *state.token.lock().unwrap() = Some(("session-a".into(), "token-a".into()));
        assert_eq!(
            state.cached_for("session-a").unwrap(),
            Some("token-a".into())
        );
        assert_eq!(state.cached_for("session-b").unwrap(), None);
        *state.token.lock().unwrap() = Some(("session-a".into(), String::new()));
        assert_eq!(state.cached_for("session-a").unwrap(), None);
    }

    #[test]
    fn pagination_metadata_cannot_silently_finish_an_archive() {
        assert!(parse_feed_page(json!({"code":0,"data":{"vFeeds":[]}})).is_err());
        assert!(parse_feed_page(json!({"code":0,"data":{"hasmore":0}})).is_err());
        assert!(parse_feed_page(json!({"code":"-1","message":"失败"})).is_err());
    }

    #[test]
    fn jsonp_keeps_all_objects_and_rejects_empty_responses() {
        let data = json!({"code":0,"data":{"photoList":(0..400).map(|i| json!({"id":i,"text":"{literal}"})).collect::<Vec<_>>()}});
        let parsed = parse_qzone_json(&format!("callback({data});")).unwrap();
        assert_eq!(parsed, data);
        assert!(parse_qzone_json("").is_err());
        assert!(parse_qzone_json("callback({\"code\":0").is_err());
    }

    #[test]
    fn different_history_notifications_with_equal_text_keep_distinct_identities() {
        let html = (0..2).map(|i| format!(r#"<li class="f-single f-s-s" data-key="fct_200_217_3_170000000{i}_1_1"><p class="txt-box-title">本人：相同正文</p></li>"#)).collect::<String>();
        let (scanned, feeds) = history_html_as_feeds(&html, "100", Some("本人"));
        assert_eq!(scanned, 2);
        assert_eq!(feeds.len(), 2);
        assert_ne!(
            feeds[0]["original"]["cell_id"],
            feeds[1]["original"]["cell_id"]
        );
    }

    #[test]
    fn history_dom_keeps_nested_cards_entities_and_media_after_child_list() {
        let html = r#"<li class='f-s-s f-single' data-key='fct_20001_311_13_1627745220_1_1'>
            <a class='f-name q_namecard' link='nameCard_20001'>好友&#x1F600;</a>
            <div class='info-detail' data-time='1627745220'>2021年7月31日 23:27</div>
            <p class='txt-box-title'>本人：&lt;旧<span>日</span>&gt; &amp;#65; &#x1F680;</p>
            <div class='comments-content'>好友：<div>第<span>一</span>段</div><div>仍在后面 &#x1F600;</div></div>
            <ul><li>嵌套列表</li></ul>
            <a href='//user.qzone.qq.com/10001/mood/mood-after-list.1'>查看说说</a>
            <a class='img-item'><img data-src='//a1.qpic.cn/photo.jpg?x=1&amp;y=2'></a>
        </li>"#;
        let (count, feeds) = history_html_as_feeds(html, "10001", Some("本人"));
        assert_eq!((count, feeds.len()), (1, 1));
        assert_eq!(feeds[0]["userinfo"]["user"]["nickname"], "好友😀");
        assert_eq!(
            feeds[0]["original"]["cell_summary"]["summary"],
            "<旧日> &#65; 🚀"
        );
        assert_eq!(feeds[0]["summary"]["summary"], "第一段仍在后面 😀");
        assert_eq!(feeds[0]["original"]["cell_id"]["cellid"], "mood-after-list");
        assert_eq!(
            feeds[0]["original"]["cell_pic"]["picdata"]["pic"][0]["photourl"][0]["url"],
            "https://a1.qpic.cn/photo.jpg?x=1&y=2"
        );
    }

    #[test]
    #[ignore = "local parser throughput benchmark; no network requests"]
    fn history_parser_benchmark() {
        let html = (0..30).map(|index| format!(r#"<li class="f-single f-s-s" data-key="fct_20001_217_3_1627745220_{index}_1"><a class="f-name q_namecard" link="nameCard_20001">好友甲</a><a href="//user.qzone.qq.com/10001/mood/post-{index}">查看说说</a><div class="info-detail" data-time="1627745220">2021年7月31日 23:27</div><p class="txt-box-title">本人：历史记录 {index}</p><a class="img-item"><img src="//a1.qpic.cn/photo-{index}.jpg"></a></li>"#)).collect::<String>();
        let start = std::time::Instant::now();
        for _ in 0..10 {
            let (count, feeds) = history_html_as_feeds(&html, "10001", Some("本人"));
            assert_eq!((count, feeds.len()), (30, 30));
            std::hint::black_box(feeds);
        }
        println!(
            "history parser: {:.2} ms/page (30 cards, 10 pages)",
            start.elapsed().as_secs_f64() * 100.0
        );
    }

    #[test]
    fn rejects_incomplete_pagination_instead_of_claiming_archive_complete() {
        assert!(parse_feed_page(
            json!({"code":0,"data":{"hasmore":1,"vFeeds":[],"attachinfo":"next"}})
        )
        .is_err());
        assert!(parse_feed_page(json!({"code":0,"data":{"hasmore":1,"vFeeds":[{}]}})).is_err());
        assert!(
            parse_feed_page(json!({"code":0,"data":{"hasmore":0,"vFeeds":[]}}))
                .unwrap()
                .feeds
                .is_empty()
        );
    }

    #[test]
    fn photo_signature_is_reused_only_in_the_original_cookie_session() {
        let state = super::QzonePageTokenState::default();
        *state.photo_key.lock().unwrap() = Some(("session-a".into(), "key".into()));
        assert_eq!(
            state.photo_cookie("session-a").unwrap().as_deref(),
            Some("key")
        );
        assert!(state.photo_cookie("session-b").unwrap().is_none());
    }

    #[test]
    fn uses_endpoint_safe_library_page_sizes() {
        assert_eq!(library_page_size("favorites"), 30);
        assert_eq!(library_page_size("guestbook"), 20);
        assert_eq!(library_page_size("videos"), 20);
        assert_eq!(library_page_size("photos"), 100);
        assert_eq!(library_page_size("albums"), 100);
    }

    #[test]
    fn extracts_authenticated_page_token_without_exposing_other_script_values() {
        let html = r#"
          <script>window.unrelated = "do-not-use";</script>
          <script>
            window.g_qzonetoken = (function(){ try { return "abc_123-token"; }
            catch(e){ return ""; } })();
          </script>
        "#;
        assert_eq!(extract_qzone_token(html).as_deref(), Some("abc_123-token"));
        assert!(extract_qzone_token("<script>window.unrelated='x'</script>").is_none());
    }

    #[test]
    fn extracts_obfuscated_page_token_expression_for_isolated_webview_evaluation() {
        let html = r#"
          <script>
            window.g_qzonetoken = (function(){ try {
              return (+[]+[])+(!+[]+!![]+[]);
            } catch(e) { return ""; } })();
          </script>
        "#;
        assert_eq!(
            extract_qzone_token_expression(html).as_deref(),
            Some("(+[]+[])+(!+[]+!![]+[])")
        );
        assert!(extract_qzone_token_expression("<script>window.unrelated='x'</script>").is_none());
    }

    #[test]
    fn keeps_first_page_feeds_and_cursor() {
        let page = parse_feed_page(json!({
            "code": 0,
            "data": { "attachinfo": "next-cursor", "hasmore": 1, "vFeeds": [{"id": 1}] }
        }))
        .unwrap();
        assert_eq!(page.feeds.len(), 1);
    }

    #[test]
    fn empty_page_finishes_pagination() {
        let page = parse_feed_page(json!({"code": 0, "data": {"hasmore":0,"vFeeds": []}})).unwrap();
        assert!(page.feeds.is_empty());
    }

    #[test]
    fn cursor_remains_server_encoded_until_query_serialization() {
        let cursor = "att=back%5Fserver%5Finfo%3Doffset%253D6&tl=123";
        let encoded =
            reqwest::Url::parse_with_params(FEEDS_URL, &[("res_attach", cursor)]).unwrap();
        assert!(encoded
            .as_str()
            .contains("back%255Fserver%255Finfo%253Doffset%25253D6%26tl%3D123"));
        assert_eq!(
            encoded
                .query_pairs()
                .find(|(key, _)| key == "res_attach")
                .unwrap()
                .1,
            cursor
        );
    }

    #[test]
    fn retries_rate_limits_and_temporary_api_errors() {
        assert!(feed_response_can_retry("获取空间动态失败：HTTP 429"));
        assert!(feed_response_can_retry(
            "QQ 空间动态接口返回错误 -1：系统繁忙，请稍后再试"
        ));
    }

    #[test]
    fn does_not_retry_expired_login_response() {
        assert!(!feed_response_can_retry(
            "QQ 空间动态接口返回错误 -3000：登录失效，请重新登录"
        ));
    }

    #[test]
    fn parses_qzone_callback_response() {
        let value = parse_qzone_json(
            r#"<script>frameElement.callback({"code":0,"data":{"succ_num":1}});</script>"#,
        )
        .unwrap();
        assert_eq!(value["code"], 0);
        assert_eq!(value["data"]["succ_num"], 1);
        assert!(ensure_qzone_success(value).is_ok());
    }

    #[test]
    fn parses_outer_object_from_nested_jsonp_response() {
        let value = parse_qzone_json(
            r#"shine0({"code":0,"data":{"albumList":[{"id":"album-1","name":"恢复相册"}]}});"#,
        )
        .unwrap();
        assert_eq!(value["code"], 0);
        assert_eq!(value["data"]["albumList"][0]["id"], "album-1");
    }

    #[test]
    fn rejects_response_without_code() {
        let value = serde_json::json!({"data": {"succ_num": 1}});
        assert!(ensure_qzone_success(value).is_err());
    }

    #[test]
    fn decodes_original_publish_time_from_legacy_mood_id() {
        let cell_id = "b327e672e295585b4ceb0500";
        assert_eq!(
            mood_cell_id_published_at(cell_id, "1927686067", 1_574_686_344),
            Some(1_532_532_194)
        );
        assert_eq!(
            mood_cell_id_published_at(cell_id, "1927686068", 1_574_686_344),
            None
        );

        let response = format!(
            r#"_Callback({{html:'\x3Cli class="f-single f-s-s" data-key="fct_2789306771_217_3_1574686344_1_1"\x3E\x3Ca class="f-name q_namecard" link="nameCard_2789306771"\x3E琪\x3C/a\x3E\x3Ca href="//user.qzone.qq.com/1927686067/mood/{cell_id}"\x3E查看说说\x3C/a\x3E\x3Cdiv class="info-detail" data-time="1574686344"\x3E2019年11月25日 20:52\x3C/div\x3E\x3Cp class="txt-box-title"\x3E小苏：生活如果不宠你\x3C/p\x3E\x3C/li\x3E',opuin:'1927686067'}});"#
        );
        let html = decode_history_html(&response).expect("应解码旧历史消息 HTML");
        let (_, feeds) = history_html_as_feeds(&html, "1927686067", Some("小苏"));
        assert_eq!(feeds[0]["comm"]["time"], 1_574_686_344_i64);
        assert_eq!(feeds[0]["original"]["cell_comm"]["time"], 1_532_532_194_i64);
    }

    #[test]
    fn classifies_history_notifications_and_filters_decorative_media() {
        let response = r#"_Callback({html:'\x3Cli class="f-single f-s-s" data-key="fct_20001_217_3_1627745220_1_1"\x3E\x3Ca class="f-name q_namecard" link="nameCard_20001"\x3E好友甲\x3C/a\x3E\x3Ca href="//user.qzone.qq.com/10001/mood/moment-1"\x3E查看说说\x3C/a\x3E\x3Cdiv class="info-detail" data-time="1627745220"\x3E2021年7月31日 23:27\x3C/div\x3E\x3Cp class="txt-box-title ellipsis-one"\x3E本人：\t\t一条历史说说\x3C/p\x3E\x3Cimg src="//qlogo2.store.qq.com/qzone/20001/20001/50"\x3E\x3Ca class="img-item"\x3E\x3Cimg src="//a1.qpic.cn/old.jpg"\x3E\x3C/a\x3E\x3C/li\x3E\x3Cli class="f-single f-s-s" data-key="fct_30001_311_14_1627745320_1_1"\x3E\x3Ca class="f-name q_namecard" link="nameCard_99999"\x3E好友乙\x3C/a\x3E\x3Ca href="//user.qzone.qq.com/10001/mood/moment-1"\x3E查看说说\x3C/a\x3E\x3Cdiv class="info-detail" data-time="1627745320"\x3E2021年7月31日 23:28\x3C/div\x3E\x3Cp class="txt-box-title ellipsis-one"\x3E本人：一条历史说说\x3C/p\x3E\x3Cdiv class="comments-content"\x3E好友乙：这是评论正文\x3C/div\x3E\x3C/li\x3E\x3Cli class="f-single f-s-s" data-key="fct_40001_333_15_1627745420_1_1"\x3E\x3Cp class="txt-box-title ellipsis-one"\x3E本人：系统通知\x3C/p\x3E\x3C/li\x3E\x3Cli class="f-single f-s-s" data-key="fct_50001_334_1_1585007595_1_1"\x3E\x3Ca class="f-name q_namecard" link="nameCard_50001"\x3E留言好友\x3C/a\x3E\x3Cdiv class="info-detail" data-time="1585007595"\x3E2020年3月24日 08:33\x3C/div\x3E\x3Cp class="txt-box-title ellipsis-one"\x3E本人：留言：愿你快乐\x3C/p\x3E\x3C/li\x3E',opuin:'10001'});"#;
        let html = decode_history_html(response).expect("应解码旧历史消息 HTML");
        let (scanned, feeds) = history_html_as_feeds(&html, "10001", Some("本人"));
        assert_eq!(scanned, 4);
        assert_eq!(feeds.len(), 4);
        assert_eq!(feeds[0]["comm"]["subid"], 217);
        assert_eq!(feeds[1]["comm"]["subid"], 2);
        assert_eq!(feeds[2]["comm"]["subid"], 333);
        assert_eq!(feeds[3]["comm"]["subid"], 334);
        assert_eq!(feeds[3]["original"]["cell_comm"]["appid"], 334);
        assert_eq!(feeds[3]["original"]["cell_comm"]["time"], 1_585_007_595_i64);
        assert_eq!(
            feeds[3]["original"]["cell_userinfo"]["user"]["uin"],
            "50001"
        );
        assert_eq!(feeds[3]["original"]["cell_summary"]["summary"], "愿你快乐");
        assert_eq!(feeds[3]["summary"]["summary"], "愿你快乐");
        assert!(feeds[3]["original"]["cell_id"]["cellid"]
            .as_str()
            .unwrap()
            .starts_with("history-guestbook:"));
        assert_eq!(feeds[2]["original"]["cell_summary"]["summary"], "系统通知");
        assert_eq!(feeds[0]["userinfo"]["user"]["uin"], "20001");
        assert_eq!(feeds[1]["userinfo"]["user"]["uin"], "30001");
        assert_eq!(feeds[1]["userinfo"]["user"]["nickname"], "好友乙");
        assert_eq!(feeds[1]["summary"]["summary"], "这是评论正文");
        assert_eq!(
            feeds[1]["original"]["cell_comment"]["main_comment"]["content"],
            "这是评论正文"
        );
        assert_eq!(feeds[0]["original"]["cell_id"]["cellid"], "moment-1");
        assert_eq!(feeds[1]["original"]["cell_id"]["cellid"], "moment-1");
        assert_eq!(
            feeds[0]["original"]["cell_userinfo"]["user"]["uin"],
            "10001"
        );
        assert_eq!(
            feeds[0]["original"]["cell_summary"]["summary"],
            "一条历史说说"
        );
        assert_eq!(
            feeds[0]["original"]["cell_pic"]["picdata"]["pic"][0]["photourl"][0]["url"],
            "https://a1.qpic.cn/old.jpg"
        );
        assert_eq!(
            feeds[0]["original"]["cell_pic"]["picdata"]["pic"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(feeds[0]["comm"]["time"], 1_627_745_220);
        assert_eq!(feeds[0]["original"]["cell_comm"]["time"], 0);
        assert!(!feeds[0]["original"]["cell_summary"]["summary"]
            .as_str()
            .unwrap()
            .contains("\\t"));
    }
}
