//! Contact statistics share the archive decoder and retain only one post's raw interactions.
use super::*;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    uin: String,
    nickname: String,
    posts: usize,
    dialogue_posts: usize,
    first_at: i64,
    last_at: i64,
    last_message: Option<ContactMessage>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContactMessage {
    content: String,
    created_at: i64,
}

struct ContactRecord {
    metrics: Contact,
    post_ids: Vec<i64>,
    name_at: i64,
}

struct ContactSnapshot {
    owner: String,
    id: u64,
    updated_at: i64,
    contacts: HashMap<String, ContactRecord>,
}

#[derive(Default)]
pub struct ContactsState(tokio::sync::Mutex<Option<ContactSnapshot>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactDirectory {
    snapshot_id: u64,
    updated_at: i64,
    contacts: Vec<Contact>,
}

#[derive(Serialize)]
pub struct ContactFeedPage {
    items: Vec<ArchiveItem>,
    total: usize,
}

fn contact_record<'a>(
    contacts: &'a mut HashMap<String, ContactRecord>,
    owner: &str,
    uin: Option<&str>,
    name: Option<&str>,
    time: i64,
) -> Option<&'a mut ContactRecord> {
    let uin = uin.filter(|uin| {
        !uin.is_empty() && *uin != owner && uin.bytes().all(|byte| byte.is_ascii_digit())
    })?;
    let record = contacts
        .entry(uin.to_owned())
        .or_insert_with(|| ContactRecord {
            metrics: Contact {
                uin: uin.to_owned(),
                nickname: uin.to_owned(),
                posts: 0,
                dialogue_posts: 0,
                first_at: 0,
                last_at: 0,
                last_message: None,
            },
            post_ids: Vec::new(),
            name_at: 0,
        });
    if let Some(name) = name
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != uin)
    {
        if record.metrics.nickname == uin || time >= record.name_at {
            record.metrics.nickname = name.to_owned();
            record.name_at = time;
        }
    }
    if time > 0 {
        let first = &mut record.metrics.first_at;
        *first = if *first == 0 {
            time
        } else {
            (*first).min(time)
        };
        record.metrics.last_at = record.metrics.last_at.max(time);
    }
    Some(record)
}

fn add_post(record: &mut ContactRecord, id: i64) {
    if record.post_ids.last() != Some(&id) {
        record.post_ids.push(id);
        record.metrics.posts += 1;
    }
}

fn remember_message(record: &mut ContactRecord, content: &str, time: i64) {
    let content = content.trim();
    if !content.is_empty()
        && record
            .metrics
            .last_message
            .as_ref()
            .is_none_or(|message| time > message.created_at)
    {
        record.metrics.last_message = Some(ContactMessage {
            content: content.to_owned(),
            created_at: time,
        });
    }
}

fn summarize_post(snapshot: &mut ContactSnapshot, id: i64, comments: Vec<ArchiveComment>) {
    let mut dialogues = HashSet::new();
    for comment in merge_comments(comments) {
        // Both people must speak in this thread and have a direct reply between them.
        let speakers: HashSet<_> = comment
            .uin
            .as_deref()
            .into_iter()
            .chain(
                comment
                    .replies
                    .iter()
                    .filter_map(|reply| reply.uin.as_deref()),
            )
            .collect();
        if speakers.contains(snapshot.owner.as_str()) {
            for reply in &comment.replies {
                let target = reply.reply_to_uin.as_deref().or(comment.uin.as_deref());
                let partner = if reply.uin.as_deref() == Some(&snapshot.owner) {
                    target
                } else if target == Some(&snapshot.owner) {
                    reply.uin.as_deref()
                } else {
                    None
                };
                if let Some(uin) =
                    partner.filter(|uin| *uin != snapshot.owner && speakers.contains(uin))
                {
                    dialogues.insert(uin.to_owned());
                }
            }
        }
        if let Some(record) = contact_record(
            &mut snapshot.contacts,
            &snapshot.owner,
            comment.uin.as_deref(),
            comment.nickname.as_deref(),
            comment.created_at,
        ) {
            remember_message(record, &comment.content, comment.created_at);
            add_post(record, id);
        }
        for reply in comment.replies {
            if reply.uin.as_deref() == Some(&snapshot.owner) {
                // A reply without an explicit target addresses the parent comment.
                let target = reply.reply_to_uin.as_deref().or(comment.uin.as_deref());
                let name = if reply.reply_to_uin.is_some() {
                    reply.reply_to_nickname.as_deref()
                } else {
                    comment.nickname.as_deref()
                };
                if let Some(record) =
                    contact_record(&mut snapshot.contacts, &snapshot.owner, target, name, 0)
                {
                    add_post(record, id);
                }
            } else if let Some(record) = contact_record(
                &mut snapshot.contacts,
                &snapshot.owner,
                reply.uin.as_deref(),
                reply.nickname.as_deref(),
                reply.created_at,
            ) {
                remember_message(record, &reply.content, reply.created_at);
                add_post(record, id);
            }
        }
    }
    for uin in dialogues {
        if let Some(record) = snapshot.contacts.get_mut(&uin) {
            record.metrics.dialogue_posts += 1;
        }
    }
}

fn read_contacts(db: &Connection, owner: &str, id: u64) -> Result<ContactSnapshot, String> {
    let mut snapshot = ContactSnapshot {
        owner: owner.to_owned(),
        id,
        updated_at: now(),
        contacts: HashMap::new(),
    };
    let mut statement = db.prepare(&format!(
        "SELECT d.id,f.event_type,f.actor_uin,f.actor_name,f.event_summary,f.event_time,f.comments_json,f.feed_key
         FROM archive_dynamics d JOIN archive_feeds f ON f.owner_uin=d.owner_uin AND f.cell_id=d.cell_id
         WHERE d.owner_uin=?1 AND {OWN_MOOD_PREDICATE} AND f.event_type IN (2,217,311)
         ORDER BY d.published_at DESC,d.id DESC,f.event_time ASC,f.id ASC"
    )).map_err(|e| format!("准备联系人统计失败：{e}"))?;
    let mut rows = statement
        .query([owner])
        .map_err(|e| format!("查询联系人统计失败：{e}"))?;
    let mut post = None;
    let mut comments = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| format!("读取联系人记录失败：{e}"))?
    {
        let id = row
            .get::<_, i64>(0)
            .map_err(|e| format!("读取关联说说失败：{e}"))?;
        if post.is_some_and(|previous| previous != id) {
            summarize_post(&mut snapshot, post.unwrap(), std::mem::take(&mut comments));
        }
        post = Some(id);
        let interaction = (|| -> rusqlite::Result<()> {
            if row.get::<_, i64>(1)? == 217 {
                let key: String = row.get(7)?;
                let uin: Option<String> = row.get(2)?;
                let name: Option<String> = row.get(3)?;
                // Visible-like dates are post publication dates, not interaction dates.
                let time = if key.starts_with("visible-like:") {
                    0
                } else {
                    row.get(5)?
                };
                if let Some(record) = contact_record(
                    &mut snapshot.contacts,
                    owner,
                    uin.as_deref(),
                    name.as_deref(),
                    time,
                ) {
                    add_post(record, id);
                }
            } else {
                comments.push(comment_from_values(
                    row.get(6)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ));
            }
            Ok(())
        })();
        interaction.map_err(|e| format!("解析联系人记录失败：{e}"))?;
    }
    if let Some(id) = post {
        summarize_post(&mut snapshot, id, comments);
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn list_archive_contacts(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    state: tauri::State<'_, ContactsState>,
) -> Result<ContactDirectory, String> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    // One active snapshot bounds memory and keeps pagination consistent with the displayed metrics.
    let mut guard = state.0.lock().await;
    let owner = login.qzone_auth().await?.uin;
    let query_owner = owner.clone();
    let id = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let snapshot = blocking(move || read_contacts(&open_database(&app)?, &query_owner, id)).await?;
    if login.qzone_auth().await?.uin != owner {
        return Err("登录账号已切换，请刷新联系人".into());
    }
    let contacts = snapshot
        .contacts
        .values()
        .map(|record| record.metrics.clone())
        .collect();
    let result = ContactDirectory {
        snapshot_id: id,
        updated_at: snapshot.updated_at,
        contacts,
    };
    *guard = Some(snapshot);
    Ok(result)
}

fn read_contact_feeds(
    db: &Connection,
    owner: &str,
    ids: &[i64],
) -> Result<Vec<ArchiveItem>, String> {
    let mut statement = db.prepare(&format!(
        "SELECT d.id,d.owner_uin,d.cell_id,d.published_at,d.content,d.author_uin,d.author_name,d.pictures_json,d.video_json
         FROM archive_dynamics d WHERE d.owner_uin=?1 AND {OWN_MOOD_PREDICATE} AND d.id IN (SELECT value FROM json_each(?2))
         ORDER BY d.published_at DESC,d.id DESC"
    )).map_err(|e| format!("准备联系人说说查询失败：{e}"))?;
    let ids = serde_json::to_string(ids).map_err(|e| format!("序列化说说选择失败：{e}"))?;
    let mut items = statement
        .query_map(params![owner, ids], archived_item_from_row)
        .map_err(|e| format!("查询联系人说说失败：{e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("读取联系人说说失败：{e}"))?;
    drop(statement);
    hydrate_archive_item_interactions(db, &mut items)?;
    Ok(items)
}

#[tauri::command]
pub async fn list_contact_feeds(
    app: tauri::AppHandle,
    login: tauri::State<'_, QLoginState>,
    state: tauri::State<'_, ContactsState>,
    snapshot_id: u64,
    uin: String,
    offset: usize,
    limit: usize,
) -> Result<ContactFeedPage, String> {
    let owner = login.qzone_auth().await?.uin;
    let (ids, total) = {
        let guard = state.0.lock().await;
        let snapshot = guard
            .as_ref()
            .filter(|snapshot| snapshot.owner == owner && snapshot.id == snapshot_id)
            .ok_or("联系人统计已更新，请刷新后继续")?;
        let record = snapshot.contacts.get(&uin).ok_or("联系人不在当前归档中")?;
        let ids = record
            .post_ids
            .iter()
            .skip(offset)
            .take(limit.clamp(1, 50))
            .copied()
            .collect::<Vec<_>>();
        (ids, record.post_ids.len())
    };
    blocking(move || {
        Ok(ContactFeedPage {
            items: read_contact_feeds(&open_database(&app)?, &owner, &ids)?,
            total,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_merge_sources_include_reply_authors_and_keep_unknown_like_dates_unknown() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize_database(&mut db).unwrap();
        let tx = db.transaction().unwrap();
        save_feed_rows(&tx,"100",&[
            json!({"comm":{"feedskey":"post","subid":0},"original":{"cell_id":{"cellid":"p"},"cell_comm":{"appid":311},"cell_userinfo":{"user":{"uin":"100"}},"cell_summary":{"summary":"正文"}}}),
            json!({"comm":{"feedskey":"visible-like:p:300","subid":217,"time":5},"userinfo":{"user":{"uin":"300","nickname":"点赞用户"}},"original":{"cell_id":{"cellid":"p"}}}),
            json!({"comm":{"feedskey":"c1","subid":2,"time":10},"userinfo":{"user":{"uin":"200","nickname":"评论用户"}},"original":{"cell_id":{"cellid":"p"},"cell_comment":{"main_comment":{"commentid":"c","content":"你好","date":10,"user":{"uin":"200","nickname":"评论用户"},"replys":[{"content":"回复","date":20,"user":{"uin":"400","nickname":"回复用户"}},{"content":"谢谢","date":30,"user":{"uin":"100"},"replyuser":{"uin":"200"}}]}}}}),
            json!({"comm":{"feedskey":"c-copy","subid":311,"time":10},"userinfo":{"user":{"uin":"200"}},"summary":{"summary":"你好"},"original":{"cell_id":{"cellid":"p"}}}),
        ]).unwrap();
        tx.commit().unwrap();
        let snapshot = read_contacts(&db, "100", 1).unwrap();
        assert_eq!(snapshot.contacts.len(), 3);
        let author = &snapshot.contacts["200"].metrics;
        assert_eq!((author.posts, author.dialogue_posts), (1, 1));
        assert_eq!((author.first_at, author.last_at), (10, 10));
        assert_eq!(author.last_message.as_ref().unwrap().content, "你好");
        assert_eq!(snapshot.contacts["400"].metrics.posts, 1);
        assert_eq!(snapshot.contacts["400"].metrics.dialogue_posts, 0);
        assert_eq!(snapshot.contacts["300"].metrics.first_at, 0);
        assert!(snapshot.contacts["300"].metrics.last_message.is_none());
        assert!(!snapshot.contacts.contains_key("100"));
        assert!(read_contacts(&db, "999", 2).unwrap().contacts.is_empty());
        let ids = &snapshot.contacts["300"].post_ids;
        assert_eq!(read_contact_feeds(&db, "100", ids).unwrap().len(), 1);
        assert!(read_contact_feeds(&db, "999", ids).unwrap().is_empty());
    }

    #[test]
    fn contact_pagination_retains_all_posts_with_overlapping_like_sources() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize_database(&mut db).unwrap();
        let tx = db.transaction().unwrap();
        for index in 1..=61 {
            let cell = format!("p-{index}");
            save_feed_rows(&tx, "100", &[
                json!({"comm":{"feedskey":format!("post-{index}"),"subid":0,"time":index},"original":{"cell_id":{"cellid":cell},"cell_comm":{"appid":311},"cell_userinfo":{"user":{"uin":"100"}},"cell_summary":{"summary":"正文"}}}),
                json!({"comm":{"feedskey":format!("visible-like:{cell}:300"),"subid":217,"time":index},"userinfo":{"user":{"uin":"300","nickname":"当前昵称"}},"original":{"cell_id":{"cellid":cell}}}),
                json!({"comm":{"feedskey":format!("history-v2-event:{cell}:300"),"subid":217,"time":1000+index},"userinfo":{"user":{"uin":"300","nickname":"点赞用户"}},"original":{"cell_id":{"cellid":cell}}}),
            ]).unwrap();
        }
        tx.commit().unwrap();
        let snapshot = read_contacts(&db, "100", 1).unwrap();
        let record = &snapshot.contacts["300"];
        assert_eq!(
            (record.metrics.posts, record.metrics.dialogue_posts),
            (61, 0)
        );
        assert_eq!(
            (record.metrics.first_at, record.metrics.last_at),
            (1001, 1061)
        );
        let pages = record
            .post_ids
            .chunks(20)
            .map(|ids| read_contact_feeds(&db, "100", ids).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            pages.iter().map(Vec::len).collect::<Vec<_>>(),
            [20, 20, 20, 1]
        );
        let all = pages.into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(
            all.iter().map(|item| item.id).collect::<Vec<_>>(),
            record.post_ids
        );
        assert_eq!(
            all.iter().map(|item| item.id).collect::<HashSet<_>>().len(),
            61
        );
    }

    #[test]
    fn dialogue_requires_both_speakers_and_direct_replies_in_the_same_thread() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize_database(&mut db).unwrap();
        let mut snapshot = read_contacts(&db, "100", 1).unwrap();
        let thread = |uin: &str, replies: Value| {
            comment_from_values(
            Some(json!({"main_comment":{"user":{"uin":uin},"content":uin,"date":10,"replys":replies}}).to_string()),
            None, None, None, 0,
        )
        };
        let reply = |author: &str, target: &str, time: i64| {
            json!({
                "user":{"uin":author},"replyuser":{"uin":target},"content":format!("{author}-{time}"),"date":time,
            })
        };
        summarize_post(
            &mut snapshot,
            1,
            vec![
                thread(
                    "200",
                    json!([
                        reply("100", "200", 20),
                        reply("200", "100", 30),
                        reply("100", "200", 40)
                    ]),
                ),
                thread("200", json!([reply("100", "200", 50)])),
                thread(
                    "400",
                    json!([reply("500", "400", 20), reply("100", "600", 30)]),
                ),
                thread(
                    "100",
                    json!([{"user":{"uin":"700"},"content":"回复本人评论","date":20}]),
                ),
                thread("800", json!([reply("900", "800", 20)])),
            ],
        );
        assert_eq!(snapshot.contacts["200"].metrics.dialogue_posts, 1);
        let message = snapshot.contacts["200"]
            .metrics
            .last_message
            .as_ref()
            .unwrap();
        assert_eq!((&*message.content, message.created_at), ("200-30", 30));
        assert_eq!(snapshot.contacts["700"].metrics.dialogue_posts, 1);
        for uin in ["400", "500", "600", "800", "900"] {
            assert_eq!(snapshot.contacts[uin].metrics.dialogue_posts, 0, "{uin}");
            assert_eq!(snapshot.contacts[uin].metrics.posts, 1);
        }
        assert_eq!(
            (
                snapshot.contacts["200"].metrics.first_at,
                snapshot.contacts["200"].metrics.last_at
            ),
            (10, 30)
        );
        assert!(!snapshot.contacts.contains_key("100"));
    }
}
