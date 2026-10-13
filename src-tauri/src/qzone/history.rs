//! Legacy history decoding and HTML structure parsing. No HTTP or database access.
use scraper::{ElementRef, Html, Selector};
use serde_json::{json, Value};
use std::sync::LazyLock;
use url::Url;

fn trailing_qq_number(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    (!digits.is_empty()).then_some(digits)
}

fn current_utc_year() -> i32 {
    let mut days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let mut year = 1970_i32;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let year_days = if leap { 366 } else { 365 };
        if days < year_days {
            return year;
        }
        days -= year_days;
        year += 1;
    }
}

fn history_date_to_timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return 0;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day > month_days[month as usize - 1] {
        return 0;
    }
    // Howard Hinnant's civil-date conversion. QQ displays these timestamps in
    // China Standard Time, so convert the wall clock from UTC+8 to Unix time.
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era as i64 * 146_097 + day_of_era as i64 - 719_468;
    days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second) - 8 * 3_600
}

pub(crate) fn parse_history_timestamp(value: &str) -> i64 {
    static ATTR: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"data-(?:time|timestamp|abstime)=["'](\d{9,13})["']"#)
            .expect("fixed timestamp attribute regex")
    });
    if let Some(raw) = ATTR
        .captures(value)
        .and_then(|captures| captures.get(1))
        .and_then(|number| number.as_str().parse::<i64>().ok())
    {
        return if raw > 10_000_000_000 {
            raw / 1_000
        } else {
            raw
        };
    }
    static FULL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"(?P<year>\d{4})[年/-](?P<month>\d{1,2})[月/-](?P<day>\d{1,2})日?\s+(?P<hour>\d{1,2}):(?P<minute>\d{1,2})(?::(?P<second>\d{1,2}))?"
    ).expect("fixed history timestamp regex")
    });
    static SHORT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"(?P<month>\d{1,2})月(?P<day>\d{1,2})日\s+(?P<hour>\d{1,2}):(?P<minute>\d{1,2})(?::(?P<second>\d{1,2}))?"
    ).expect("fixed short history timestamp regex")
    });
    let captures = FULL.captures(value).or_else(|| SHORT.captures(value));
    let Some(captures) = captures else {
        return 0;
    };
    let number = |name: &str| {
        captures
            .name(name)
            .and_then(|value| value.as_str().parse::<u32>().ok())
    };
    let year = captures
        .name("year")
        .and_then(|value| value.as_str().parse::<i32>().ok())
        .unwrap_or_else(current_utc_year);
    history_date_to_timestamp(
        year,
        number("month").unwrap_or(1),
        number("day").unwrap_or(1),
        number("hour").unwrap_or(0),
        number("minute").unwrap_or(0),
        number("second").unwrap_or(0),
    )
}

pub(super) fn decode_history_html(response: &str) -> Result<String, String> {
    static HTML: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?s)html\s*:\s*'(.*)',\s*opuin").expect("fixed history response regex")
    });
    static HEX: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\\x([0-9a-fA-F]{2})").expect("fixed hex escape regex")
    });
    let capture = HTML
        .captures(response)
        .and_then(|captures| captures.get(1))
        .ok_or("历史消息响应中缺少 html 数据")?;
    let decoded = HEX.replace_all(capture.as_str(), |captures: &regex::Captures<'_>| {
        u8::from_str_radix(&captures[1], 16)
            .map(char::from)
            .unwrap_or('\u{fffd}')
            .to_string()
    });
    Ok(decoded
        .replace("\\/", "/")
        .replace("\\'", "'")
        .replace("\\\"", "\"")
        .replace("\\t", " ")
        .replace("\\r", " ")
        .replace("\\n", "\n")
        .replace("\\\\", "\\"))
}

struct HistorySelectors {
    cards: Selector,
    people: Selector,
    actor: Selector,
    content: Selector,
    time: Selector,
    comment: Selector,
    comment_item: Selector,
    images: Selector,
    links: Selector,
}
static SELECTORS: LazyLock<HistorySelectors> = LazyLock::new(|| {
    let selector = |value| Selector::parse(value).expect("fixed history selector");
    HistorySelectors {
        cards: selector("li.f-single.f-s-s"),
        people: selector("a.q_namecard"),
        actor: selector("a.f-name"),
        content: selector("p.txt-box-title"),
        time: selector("div.info-detail"),
        comment: selector(".comments-content"),
        comment_item: selector(".comments-item"),
        images: selector("a.img-item img"),
        links: selector("a[href]"),
    }
});

fn history_plain_text(element: ElementRef<'_>) -> String {
    // HTML5 parsing decodes entities once and keeps nested inline text intact.
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn history_comment_content(card: ElementRef<'_>) -> Option<String> {
    let element = card
        .select(&SELECTORS.comment)
        .next()
        .or_else(|| card.select(&SELECTORS.comment_item).next())?;
    let mut content = history_plain_text(element);
    if let Some(index) = content
        .char_indices()
        .find_map(|(index, character)| matches!(character, ':' | '：').then_some(index))
    {
        let prefix = content[..index].trim();
        if !prefix.is_empty() && prefix.chars().count() <= 40 {
            content = content[index + content[index..].chars().next()?.len_utf8()..]
                .trim()
                .to_owned();
        }
    }
    (!content.is_empty()).then_some(content)
}

fn history_event_key(value: &str) -> Option<(String, i64, i64, i64)> {
    static KEY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^fct_(\d+)_(\d+)_(\d+)_(\d+)(?:_|$)")
            .expect("fixed history event key regex")
    });
    let captures = KEY.captures(value)?;
    Some((
        captures.get(1)?.as_str().to_owned(),
        captures.get(2)?.as_str().parse().ok()?,
        captures.get(3)?.as_str().parse().ok()?,
        captures.get(4)?.as_str().parse().ok()?,
    ))
}

fn history_content_media(card: ElementRef<'_>) -> Vec<String> {
    let mut urls = card
        .select(&SELECTORS.images)
        .filter_map(|image| {
            image
                .value()
                .attr("src")
                .or_else(|| image.value().attr("data-src"))
        })
        .filter_map(super::usable_content_media_url)
        .filter(|url| {
            let lower = url.to_ascii_lowercase();
            lower.contains("qpic.cn") || lower.contains("photo.store.qq.com")
        })
        .collect::<Vec<_>>();
    urls.sort();
    urls.dedup();
    urls
}

fn history_mood_cell_id(card: ElementRef<'_>, owner_uin: &str) -> Option<String> {
    card.select(&SELECTORS.links)
        .filter_map(|link| link.value().attr("href"))
        .find_map(|href| {
            let href = if href.starts_with("//") {
                format!("https:{href}")
            } else {
                href.to_owned()
            };
            let url = Url::parse(&href).ok()?;
            if url.host_str() != Some("user.qzone.qq.com") {
                return None;
            }
            let mut parts = url.path_segments()?;
            if parts.next()? != owner_uin || parts.next()? != "mood" {
                return None;
            }
            let cell_id = parts.next()?.to_owned();
            if cell_id.is_empty() {
                return None;
            }
            // History comment/like URLs use tid. and tid.1 for the same mood.
            match cell_id.rsplit_once('.') {
                Some((base, suffix))
                    if !base.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) =>
                {
                    Some(base.to_owned())
                }
                _ => Some(cell_id),
            }
        })
}

fn little_endian_hex_u32(encoded: &[u8]) -> Option<u32> {
    let encoded = encoded.get(..8)?;
    let mut bytes = [0_u8; 4];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let high = (encoded[index * 2] as char).to_digit(16)? as u8;
        let low = (encoded[index * 2 + 1] as char).to_digit(16)? as u8;
        *byte = high << 4 | low;
    }
    Some(u32::from_le_bytes(bytes))
}

/// QQ mood ids start with the owner UIN and original publish time, both as
/// little-endian u32 values. Legacy notification cards expose only the later
/// interaction time in their HTML, so decoding the canonical mood id is the
/// only surviving authoritative timestamp for many deleted posts.
pub(crate) fn mood_cell_id_published_at(
    cell_id: &str,
    owner_uin: &str,
    event_time: i64,
) -> Option<i64> {
    if cell_id.len() < 16 {
        return None;
    }
    let encoded = cell_id.as_bytes();
    let encoded_owner = little_endian_hex_u32(encoded.get(..8)?)?;
    let owner = owner_uin.parse::<u32>().ok()?;
    if encoded_owner != owner {
        return None;
    }
    let published_at = i64::from(little_endian_hex_u32(encoded.get(8..16)?)?);
    const QZONE_LAUNCH: i64 = 1_104_537_600; // 2005-01-01 UTC
    if published_at < QZONE_LAUNCH || (event_time > 0 && published_at > event_time) {
        return None;
    }
    Some(published_at)
}

fn history_original_content(content: &str, owner_name: Option<&str>) -> Option<String> {
    let mut content = content.trim().to_owned();
    let owner_name = owner_name.map(str::trim).filter(|name| !name.is_empty())?;
    if !content.starts_with(owner_name) {
        return None;
    }
    content = content[owner_name.len()..]
        .trim_start_matches(|character: char| {
            character.is_whitespace()
                || matches!(character, ':' | '：' | '，' | ',' | '。' | '·' | '-')
        })
        .trim()
        .to_owned();
    for marker in ["的主页", "的说说"] {
        if let Some(rest) = content.strip_prefix(marker) {
            content = rest
                .trim_start_matches(|character: char| {
                    character.is_whitespace()
                        || matches!(character, ':' | '：' | '，' | ',' | '。' | '·' | '-')
                })
                .trim()
                .to_owned();
        }
    }
    (!content.is_empty()).then_some(content)
}

fn history_guestbook_content(content: &str) -> String {
    content
        .trim()
        .strip_prefix("留言")
        .unwrap_or(content.trim())
        .trim_start_matches(|character: char| {
            character.is_whitespace()
                || matches!(character, ':' | '：' | '，' | ',' | '。' | '·' | '-')
        })
        .trim()
        .to_owned()
}

pub(super) fn history_html_as_feeds(
    html: &str,
    owner_uin: &str,
    owner_name: Option<&str>,
) -> (usize, Vec<Value>) {
    let document = Html::parse_fragment(html);
    let cards = document.select(&SELECTORS.cards).collect::<Vec<_>>();
    let scanned_count = cards.len();
    let feeds = cards
        .into_iter()
        .filter_map(|card| {
            let stable_id = card.value().attr("data-key").or_else(|| card.value().attr("id"))?;
            let (actor_uin, family, subtype, key_time) = history_event_key(stable_id)?;
            // GetQzonehistory keeps every historical card whose summary belongs
            // to the current account. Some old publish/share notification
            // families still contain recoverable post text even though they are
            // neither a like (217) nor a comment (311). Preserve their original
            // family number instead of silently dropping the whole post.
            let event_type = if family == 311 { 2 } else { family };
            let is_guestbook = event_type == 334;
            let people = card.select(&SELECTORS.people).filter_map(|person| {
                let uin = ["link", "data-uin", "href"].iter().find_map(|name| {
                    person.value().attr(name).and_then(trailing_qq_number)
                })?;
                Some((uin, history_plain_text(person)))
            }).collect::<Vec<_>>();
            let actor_name = people
                .iter()
                .find(|(uin, _)| uin == &actor_uin)
                .map(|(_, name)| name.clone())
                .filter(|name| !name.is_empty())
                // The legacy endpoint sometimes puts the operator UIN only in
                // the card key while the first visible name card still carries
                // the correct nickname. This mirrors GetQzonehistory's parser.
                .or_else(|| {
                    card.select(&SELECTORS.actor).next().map(history_plain_text)
                        .filter(|name| !name.is_empty())
                })
                .or_else(|| {
                    people
                        .iter()
                        .map(|(_, name)| name.clone())
                        .find(|name| !name.is_empty())
                })
                .unwrap_or_else(|| actor_uin.clone());
            let content_element = card.select(&SELECTORS.content).next()?;
            let mood_id = history_mood_cell_id(card, owner_uin);
            let linked_owner_name = content_element.select(&SELECTORS.people).find_map(|person| {
                let matches_owner = ["link", "data-uin", "href"].iter().any(|attribute| {
                    person.value().attr(attribute).and_then(trailing_qq_number).as_deref() == Some(owner_uin)
                });
                matches_owner.then(|| history_plain_text(person)).filter(|name| !name.is_empty())
            });
            let owner_name = linked_owner_name.as_deref().or(owner_name);
            let text = history_plain_text(content_element);
            // A canonical owner/mood URL is identity evidence even when the nickname
            // service is unavailable or the account has changed its display name.
            let content = history_original_content(&text, owner_name).or_else(|| {
                (mood_id.is_some() && !text.trim().is_empty()).then_some(text)
            })?;
            let guestbook_content = is_guestbook.then(|| history_guestbook_content(&content));
            let parsed_time = card.select(&SELECTORS.time).next().map(|node| {
                let timestamp = ["data-time", "data-timestamp", "data-abstime"].iter()
                    .find_map(|name| node.value().attr(name).and_then(|value| value.parse::<i64>().ok()));
                timestamp.map(|value| if value>10_000_000_000 {value/1_000} else {value})
                    .unwrap_or_else(|| parse_history_timestamp(&history_plain_text(node)))
            }).unwrap_or(0);
            let event_time = if key_time > 0 { key_time } else { parsed_time };
            let media_urls = history_content_media(card);
            let pictures = media_urls
                .iter()
                .map(|url| json!({ "photourl": [{ "url": url }] }))
                .collect::<Vec<_>>();
            let cell_id = if is_guestbook {
                format!("history-guestbook:{stable_id}")
            } else {
                mood_id
                    .unwrap_or_else(|| format!("history-v3:{stable_id}"))
            };
            // The legacy card's visible time is when the interaction happened,
            // not when the original mood was published. Never invent a publish
            // time when the canonical mood id is absent or fails validation.
            let published_at = if is_guestbook {
                event_time
            } else {
                mood_cell_id_published_at(&cell_id, owner_uin, event_time).unwrap_or(0)
            };
            let target_name = people
                .iter()
                .find(|(uin, _)| uin != &actor_uin && uin != owner_uin)
                .map(|(_, name)| name.as_str());
            let comment_content = (event_type == 2)
                .then(|| history_comment_content(card))
                .flatten();
            let event_summary = match event_type {
                217 => "点赞了这条说说".to_owned(),
                334 => guestbook_content.clone().unwrap_or_default(),
                2 if comment_content.is_some() => comment_content.clone().unwrap_or_default(),
                2 if subtype == 14 || subtype == 35 => target_name
                    .map(|name| format!("回复了 {name}（旧历史接口未保留回复正文）"))
                    .unwrap_or_else(|| "历史回复（旧历史接口未保留回复正文）".to_owned()),
                2 => "历史评论（旧历史接口未保留评论正文）".to_owned(),
                _ => "历史动态残留".to_owned(),
            };
            let comments = (event_type == 2).then(|| {
                json!({
                    "main_comment": {
                        "commentid": stable_id,
                        "content": event_summary,
                        "date": event_time,
                        "user": { "uin": actor_uin, "nickname": actor_name },
                        "replys": [],
                    }
                })
            });
            Some(json!({
                "comm": {
                    "subid": event_type,
                    "time": event_time,
                    "feedskey": format!("history-v2-event:{stable_id}"),
                },
                "userinfo": { "user": { "uin": actor_uin, "nickname": actor_name } },
                "summary": { "summary": event_summary },
                "original": {
                    "cell_id": { "cellid": cell_id },
                    "cell_comm": {
                        "appid": if is_guestbook { 334 } else { 311 },
                        "time": published_at,
                        "feedskey": format!("history-original:{stable_id}"),
                    },
                    "cell_userinfo": { "user": {
                        "uin": if is_guestbook { actor_uin.as_str() } else { owner_uin },
                        "nickname": if is_guestbook { Some(actor_name.as_str()) } else { owner_name }
                    } },
                    "cell_summary": { "summary": guestbook_content.unwrap_or(content) },
                    "cell_pic": { "picdata": { "pic": pictures } },
                    "cell_comment": comments,
                },
            }))
        })
        .collect();
    (scanned_count, feeds)
}

pub(super) fn decode_history_response(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    for encoding in [encoding_rs::GB18030, encoding_rs::BIG5] {
        let (decoded, _, had_errors) = encoding.decode(bytes);
        if !had_errors {
            return decoded.into_owned();
        }
    }
    encoding_rs::GBK.decode(bytes).0.into_owned()
}
