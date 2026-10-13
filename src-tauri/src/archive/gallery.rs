//! Album resources have separate cover and playable/original media contracts.
use super::{
    blocking, media,
    runtime::{Control, Session},
    ArchiveState,
};
use crate::commands::{
    qlogin::QLoginState,
    qzone::{self, QzonePageTokenState, RecycleAuthState},
};
use reqwest::header::{COOKIE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;
use url::Url;

fn media_url(raw: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| raw.get(*name).and_then(Value::as_str))
        .find_map(|text| {
            let text = text.trim();
            let url = Url::parse(text).ok()?;
            matches!(url.scheme(), "http" | "https").then(|| text.to_owned())
        })
}

pub(super) fn resource_media(module: &str, raw: &Value) -> (Option<String>, Vec<String>) {
    let cover = media_url(
        raw,
        &[
            "pre",
            "cover_url",
            "coverUrl",
            "cover",
            "thumb",
            "thumbnail",
            "pic_url",
        ],
    );
    match module {
        "albums" => (cover.or_else(|| media_url(raw, &["url"])), Vec::new()),
        "videos" => {
            let video = media_url(
                raw,
                &[
                    "video_download_url",
                    "video_play_url",
                    "videoUrl",
                    "videourl",
                    "video_url",
                    "playUrl",
                    "downloadUrl",
                    "download_url",
                    "raw",
                    "src",
                    "url",
                ],
            )
            .or_else(|| {
                raw.get("video_info")
                    .and_then(|info| media_url(info, &["download_url", "video_url"]))
            });
            (cover, video.into_iter().collect())
        }
        _ => {
            let original = media_url(raw, &["raw", "origin_url", "origin", "url", "pre"]);
            (
                cover.or_else(|| original.clone()),
                original.into_iter().collect(),
            )
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecycleKind {
    Albums,
    Photos,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecycleItem {
    item_key: String,
    parent_key: String,
    title: String,
    cover_url: Option<String>,
    media_urls: Vec<String>,
    deleted_at: String,
    count: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecyclePage {
    items: Vec<RecycleItem>,
    next_offset: usize,
    complete: bool,
    total: Option<u64>,
}

fn recycle_page(
    kind: RecycleKind,
    mut response: Value,
    offset: usize,
    parent: &str,
) -> Result<RecyclePage, String> {
    let data = if response.get("data").is_some() {
        response.get_mut("data").unwrap()
    } else {
        &mut response
    };
    let albums = matches!(kind, RecycleKind::Albums);
    // These fields are used by the supplied Qzone-Restore-Archive reader.
    // Unknown structures are errors rather than a fabricated empty recycle bin.
    let fields: &[&str] = if albums {
        &["album", "albums", "albumList"]
    } else {
        &["photo", "photos", "photoList", "list"]
    };
    let name = fields
        .iter()
        .find(|name| data.get(**name).is_some_and(Value::is_array))
        .ok_or("回收站响应缺少有效列表字段，请在 QQ 空间确认访问权限")?;
    let total = data
        .get("total")
        .or_else(|| data.get("totalNum"))
        .map(|total| {
            total
                .as_u64()
                .or_else(|| total.as_str()?.parse().ok())
                .ok_or("回收站总数字段无效")
        })
        .transpose()?;
    let raw = std::mem::take(data.get_mut(*name).and_then(Value::as_array_mut).unwrap());
    let next_offset = offset.checked_add(raw.len()).ok_or("回收站分页偏移溢出")?;
    let complete = super::library::library_page_complete(
        next_offset,
        raw.len(),
        total.and_then(|n| usize::try_from(n).ok()),
    )?;
    let items = raw
        .into_iter()
        .map(|item| {
            let item_key = super::library::value_text(
                &item,
                if albums {
                    &["albumid", "albumId", "id"]
                } else {
                    &["lloc", "photoId", "photoid", "id"]
                },
            )
            .ok_or("回收站记录缺少资源标识，分页未推进")?;
            let (cover_url, media_urls) =
                resource_media(if albums { "albums" } else { "photos" }, &item);
            Ok(RecycleItem {
                item_key,
                parent_key: super::library::value_text(&item, &["albumid", "albumId"])
                    .unwrap_or_else(|| parent.to_owned()),
                title: super::library::value_text(
                    &item,
                    &["name", "albumname", "photoName", "title"],
                )
                .unwrap_or_else(|| {
                    if albums {
                        "未命名相册"
                    } else {
                        "未命名照片"
                    }
                    .into()
                }),
                cover_url,
                media_urls,
                deleted_at: super::library::value_text(
                    &item,
                    &["deleteTime", "deltime", "modifytime"],
                )
                .unwrap_or_default(),
                count: super::library::value_text(&item, &["num", "photoNum", "total"])
                    .and_then(|n| n.parse().ok()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RecyclePage {
        items,
        next_offset,
        complete,
        total,
    })
}

#[tauri::command]
pub async fn list_recycle_resources(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    archive: tauri::State<'_, ArchiveState>,
    recycle: tauri::State<'_, RecycleAuthState>,
    kind: RecycleKind,
    offset: usize,
    album_id: Option<String>,
) -> Result<RecyclePage, String> {
    // Serialize upstream list reads with the same task lock and rate budget as archiving.
    let _task = archive
        .task
        .clone()
        .try_lock_owned()
        .map_err(|_| "已有同步任务正在执行，请稍后读取回收站")?;
    let auth = login.qzone_auth().await?;
    let signature = recycle.signature(&auth.cookie_header)?;
    let (_control, receiver) = tokio::sync::watch::channel(Control::Run);
    let session = Session::new(
        app,
        auth.uin.clone(),
        receiver,
        archive.requests.clone(),
        2_000,
    );
    let response = session
        .network(qzone::fetch_recycle_page(
            &login,
            matches!(kind, RecycleKind::Albums),
            &signature,
            offset,
            album_id.as_deref(),
            &session,
        ))
        .await?;
    let current = login.qzone_auth().await?;
    if current.cookie_header != auth.cookie_header {
        return Err("登录会话已变化，请重新读取回收站".into());
    }
    recycle_page(
        kind,
        response,
        offset,
        album_id.as_deref().unwrap_or_default(),
    )
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Image,
    Video,
}

pub(super) fn trusted_media_url(text: &str) -> Result<Url, String> {
    let mut url = Url::parse(text).map_err(|_| "资源地址无效")?;
    let host = url.host_str().unwrap_or_default();
    if !matches!(url.scheme(), "http" | "https")
        || !["qq.com", "qpic.cn", "gtimg.cn", "gtimg.com"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return Err("资源地址不是 QQ 媒体域名".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("资源地址无效".into());
    }
    url.set_scheme("https").map_err(|_| "资源地址无效")?;
    Ok(url)
}

#[tauri::command]
pub async fn load_library_resource(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    archive: tauri::State<'_, ArchiveState>,
    token: tauri::State<'_, QzonePageTokenState>,
    resource_url: String,
    kind: ResourceKind,
) -> Result<String, String> {
    let url = trusted_media_url(&resource_url)?;
    let auth = login.qzone_auth().await?;
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("albums")
        .join(&auth.uin);
    let key = super::stable_feed_hash(&Value::String(resource_url));
    let file_stem = format!("{key:016x}");
    let _lock = archive
        .media_locks
        .acquire(&directory.join(&file_stem))
        .await?;
    let cache_dir = directory.clone();
    let cache_stem = file_stem.clone();
    let cached = blocking(move || {
        Ok(match kind {
            ResourceKind::Image => super::existing_archived_image(&cache_dir, &cache_stem),
            ResourceKind::Video => {
                let path = cache_dir.join(format!("{cache_stem}.mp4"));
                path.metadata()
                    .ok()
                    .filter(|metadata| metadata.len() > 32)
                    .map(|_| path)
            }
        })
    })
    .await?;
    if let Some(path) = cached {
        return Ok(path.to_string_lossy().into_owned());
    }
    let _slot = archive
        .media_downloads
        .acquire()
        .await
        .map_err(|_| "媒体下载队列已关闭")?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| e.to_string())?;
    if login.qzone_auth().await?.cookie_header != auth.cookie_header {
        return Err("登录会话已变化，请重新加载资源".into());
    }
    let mut request = archive
        .library_media_client
        .get(url.clone())
        .header(REFERER, "https://user.qzone.qq.com/")
        .header(USER_AGENT, &auth.user_agent);
    let photo_key = token.photo_cookie(&auth.cookie_header)?;
    let host = url.host_str().unwrap_or_default();
    let mut cookies = if host == "qq.com" || host.ends_with(".qq.com") {
        auth.cookie_header.clone()
    } else {
        String::new()
    };
    if let Some(key) = photo_key {
        cookies.push_str(&format!("; qq_photo_key={key}"));
    }
    if !cookies.is_empty() {
        request = request.header(COOKIE, cookies.trim_start_matches("; "));
    }
    // No total transfer deadline or byte cap; redirects cannot forward credentials.
    let response = request
        .send()
        .await
        .map_err(|e| format!("读取相册资源失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("读取相册资源失败：HTTP {}", response.status()));
    }
    let download = media::receive(response, &directory).await?;
    let extension = match kind {
        ResourceKind::Image => super::archived_image_extension(&download.prefix).filter(|_| {
            !super::is_qq_missing_image_placeholder_header(&download.prefix, download.length)
        }),
        ResourceKind::Video => download
            .prefix
            .get(4..12)
            .is_some_and(|p| p.windows(4).any(|p| p == b"ftyp"))
            .then_some("mp4"),
    };
    let Some(extension) = extension else {
        return Err("QQ 返回了无效的媒体文件，请重新同步资源地址".into());
    };
    if download.length <= 32 {
        return Err("QQ 返回了空媒体文件，请重新同步资源地址".into());
    }
    if login.qzone_auth().await?.cookie_header != auth.cookie_header {
        return Err("登录会话已变化，请重新加载资源".into());
    }
    download
        .publish(directory.join(format!("{file_stem}.{extension}")))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn separates_video_source_from_cover_without_extension_guessing() {
        let (cover, urls) = resource_media(
            "videos",
            &json!({"pre":"https://a.qpic.cn/cover", "url":"https://a.qq.com/play?id=1"}),
        );
        assert_eq!(cover.as_deref(), Some("https://a.qpic.cn/cover"));
        assert_eq!(urls, ["https://a.qq.com/play?id=1"]);
    }
    #[test]
    fn keeps_only_canonical_photo_and_does_not_duplicate_album_cover() {
        let raw = json!({"pre":"https://a.qpic.cn/small", "url":"https://a.qpic.cn/large", "raw":"https://a.qpic.cn/original"});
        assert_eq!(
            resource_media("photos", &raw).1,
            ["https://a.qpic.cn/original"]
        );
        assert!(resource_media("albums", &raw).1.is_empty());
    }
    #[test]
    fn recycle_short_pages_keep_paging_and_invalid_contracts_fail() {
        let page = recycle_page(
            RecycleKind::Photos,
            json!({"data":{"photo":[{"lloc":"p1"}],"total":80}}),
            36,
            "album",
        )
        .unwrap();
        assert_eq!(page.next_offset, 37);
        assert!(!page.complete);
        assert!(recycle_page(RecycleKind::Photos, json!({"data":{"unknown":[]}}), 0, "").is_err());
        assert!(recycle_page(
            RecycleKind::Photos,
            json!({"data":{"photo":[],"total":80}}),
            36,
            ""
        )
        .is_err());
    }
    #[test]
    fn rejects_media_hosts_that_only_share_a_suffix() {
        assert!(trusted_media_url("https://evilqq.com/image").is_err());
        assert!(trusted_media_url("https://qpic.cn.evil.com/image").is_err());
        assert!(trusted_media_url("https://localhost/image").is_err());
        assert!(trusted_media_url("https://a.qpic.cn/image").is_ok());
    }
}
