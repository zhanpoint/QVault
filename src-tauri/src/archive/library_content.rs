//! Read-only projections of the desktop guestbook/favorites contracts.
use std::{collections::HashSet, sync::OnceLock};

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use serde_json::Value;
use url::Url;

use super::library::{library_created_at, value_text};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryImage {
    pub url: String,
    pub thumbnail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryVideo {
    pub url: Option<String>,
    pub cover_url: Option<String>,
    pub title: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryReply {
    pub author_uin: Option<String>,
    pub author_name: Option<String>,
    pub created_at: i64,
    pub content: String,
    pub images: Vec<LibraryImage>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDetails {
    pub source: &'static str,
    pub kind: Option<&'static str>,
    pub source_url: Option<String>,
    pub private_message: bool,
    pub images: Vec<LibraryImage>,
    pub videos: Vec<LibraryVideo>,
    pub audio_urls: Vec<String>,
    pub replies: Vec<LibraryReply>,
}

pub(super) fn web_url(text: &str) -> Option<String> {
    let text = text.trim();
    let candidate = if text.starts_with("//") {
        format!("https:{text}")
    } else {
        text.to_owned()
    };
    let url = Url::parse(&candidate).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.into())
}

fn field_url(value: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .filter_map(|name| value.get(*name).and_then(Value::as_str))
        .find_map(web_url)
}

fn break_line(output: &mut String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}

fn element_text(element: ElementRef<'_>, output: &mut String) {
    let name = element.value().name();
    if matches!(name, "script" | "style" | "template") {
        return;
    }
    let block = matches!(
        name,
        "p" | "div" | "li" | "blockquote" | "h1" | "h2" | "h3" | "tr"
    );
    if block || name == "br" {
        break_line(output);
    }
    if name == "img" {
        if let Some(alt) = element.value().attr("alt").filter(|alt| !alt.is_empty()) {
            output.push_str(alt);
        }
    }
    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            output.push_str(text);
        } else if let Some(element) = ElementRef::wrap(child) {
            element_text(element, output);
        }
    }
    if block {
        break_line(output);
    }
}

/// HTML is parsed as data and never sent to React as executable markup.
pub(super) fn text_content(html: &str) -> String {
    let document = Html::parse_fragment(html);
    let mut output = String::with_capacity(html.len());
    element_text(document.root_element(), &mut output);
    output
        .lines()
        .map(|line| line.trim().replace('\u{a0}', " "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn html_images(html: &str) -> Vec<LibraryImage> {
    static IMAGES: OnceLock<Selector> = OnceLock::new();
    let document = Html::parse_fragment(html);
    let selector = IMAGES.get_or_init(|| Selector::parse("img").expect("constant image selector"));
    let mut seen = HashSet::new();
    document
        .select(selector)
        .filter_map(|element| {
            let thumbnail = element.value().attr("src").and_then(web_url);
            let url = element
                .value()
                .attr("orgsrc")
                .and_then(web_url)
                .or_else(|| thumbnail.clone())?;
            if !seen.insert(url.clone()) {
                return None;
            }
            Some(LibraryImage {
                thumbnail: thumbnail.unwrap_or_else(|| url.clone()),
                url,
            })
        })
        .collect()
}

pub(super) fn guestbook_content(raw: &Value) -> String {
    value_text(
        raw,
        &[
            "htmlContent",
            "content",
            "ubbContent",
            "message",
            "msgContent",
        ],
    )
    .map(|html| text_content(&html))
    .unwrap_or_default()
}

pub(super) fn favorite_info(raw: &Value) -> &Value {
    let field = match raw.get("type").and_then(Value::as_u64) {
        Some(1) => "url_info",
        Some(3) => "blog_info",
        Some(4) => "album_info",
        Some(5) => "shuoshuo_info",
        Some(7) => "share_info",
        _ => "",
    };
    raw.get(field).unwrap_or(&Value::Null)
}

pub(super) fn images(module: &str, raw: &Value) -> Vec<LibraryImage> {
    if module == "guestbook" {
        return html_images(&value_text(raw, &["htmlContent"]).unwrap_or_default());
    }
    let mut images = Vec::new();
    let originals = raw
        .get("origin_img_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let thumbnails = raw
        .get("img_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut seen = HashSet::new();
    for index in 0..originals.len().max(thumbnails.len()) {
        let extract = |entry: Option<&Value>| {
            entry.and_then(|value| {
                value
                    .as_str()
                    .and_then(web_url)
                    .or_else(|| field_url(value, &["url"]))
            })
        };
        let thumbnail = extract(thumbnails.get(index));
        if let Some(url) = extract(originals.get(index)).or_else(|| thumbnail.clone()) {
            if seen.insert(url.clone()) {
                images.push(LibraryImage {
                    thumbnail: thumbnail.unwrap_or_else(|| url.clone()),
                    url,
                });
            }
        }
    }
    images
}

pub(super) fn details(module: &str, raw: &Value, history: bool) -> LibraryDetails {
    let mut result = LibraryDetails {
        source: if history { "history" } else { "current" },
        ..Default::default()
    };
    if history {
        return result;
    }
    if module == "guestbook" {
        result.private_message = raw
            .get("secret")
            .is_some_and(|value| value.as_u64() == Some(1) || value.as_str() == Some("1"));
        result.images = images(module, raw);
        if let Some(replies) = raw.get("replyList").and_then(Value::as_array) {
            result.replies = replies
                .iter()
                .map(|reply| LibraryReply {
                    author_uin: value_text(reply, &["uin"]),
                    author_name: value_text(reply, &["nickname", "nick"]),
                    created_at: library_created_at(reply),
                    content: guestbook_content(reply),
                    images: html_images(&value_text(reply, &["htmlContent"]).unwrap_or_default()),
                })
                .collect();
        }
    } else if module == "favorites" {
        result.kind = Some(match raw.get("type").and_then(Value::as_u64) {
            Some(1) => "网页",
            Some(2 | 4) => "照片",
            Some(3) => "日志",
            Some(5) => "说说",
            Some(6) => "文字",
            Some(7) => "分享",
            _ => "其他",
        });
        let info = favorite_info(raw);
        result.source_url = field_url(info, &["url", "source_url"])
            .or_else(|| field_url(raw, &["url", "source_url"]));
        result.images = images(module, raw);
        if let Some(videos) = info.get("video_list").and_then(Value::as_array) {
            result.videos = videos
                .iter()
                .map(|video| {
                    let info = video.get("video_info").unwrap_or(video);
                    LibraryVideo {
                        url: field_url(info, &["url", "video_url", "download_url", "play_url"]),
                        cover_url: field_url(info, &["preview_img", "pre", "cover_url"]),
                        title: value_text(info, &["title", "name"])
                            .unwrap_or_else(|| "收藏视频".into()),
                    }
                })
                .collect();
        }
        if let Some(music) = info.get("music_list").and_then(Value::as_array) {
            result.audio_urls = music
                .iter()
                .filter_map(|music| {
                    field_url(
                        music.get("music_info").unwrap_or(music),
                        &["url", "music_url", "play_url"],
                    )
                })
                .collect();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn guestbook_preserves_paragraphs_entities_private_flag_and_replies() {
        let raw = json!({"secret":1,"htmlContent":"<p>你 &amp; 我<br>还记得吗？</p><script>bad()</script><img src='//qpic.cn/thumb.jpg' orgsrc='https://qpic.cn/original.jpg'><img alt='[微笑]' src='https://qpic.cn/em.gif'>", "replyList":[{"uin":"200","nickname":"朋友","pubtime":1700000000,"htmlContent":"<div>当然记得 &quot;那天&quot;</div>"}]});
        assert_eq!(guestbook_content(&raw), "你 & 我\n还记得吗？\n[微笑]");
        let result = details("guestbook", &raw, false);
        assert!(result.private_message);
        assert_eq!(result.images.len(), 2);
        assert_eq!(result.replies[0].content, "当然记得 \"那天\"");
        assert_eq!(result.replies[0].created_at, 1700000000);
    }

    #[test]
    fn favorites_separate_source_link_thumbnails_originals_and_video() {
        let raw = json!({"type":5,"url":"javascript:alert(1)","avatar":"https://qpic.cn/avatar.jpg","img_list":["//qpic.cn/thumb.jpg"],"origin_img_list":["https://qpic.cn/original.jpg","https://qpic.cn/extra.jpg"],"shuoshuo_info":{"url":"https://user.qzone.qq.com/100/mood/1","video_list":[{"video_info":{"url":"https://qq.com/video?id=1","preview_img":"https://qpic.cn/video.jpg"}}]}});
        let result = details("favorites", &raw, false);
        assert_eq!(result.kind, Some("说说"));
        assert_eq!(result.images.len(), 2);
        assert!(result
            .images
            .iter()
            .all(|image| !image.url.contains("avatar")));
        assert_eq!(result.videos.len(), 1);
        assert_eq!(
            result.source_url.as_deref(),
            Some("https://user.qzone.qq.com/100/mood/1")
        );
        assert!(web_url("javascript:alert(1)").is_none());
        assert!(web_url("https://user:secret@qq.com/a").is_none());
    }
}
