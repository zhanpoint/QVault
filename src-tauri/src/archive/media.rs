//! Bounded media transfers. Only complete, validated files enter the cache.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::{Mutex as AsyncMutex, OwnedMutexGuard},
};

#[derive(Default)]
pub(super) struct DownloadLocks(Mutex<HashMap<PathBuf, Weak<AsyncMutex<()>>>>);

impl DownloadLocks {
    pub async fn acquire(&self, key: &Path) -> Result<OwnedMutexGuard<()>, String> {
        let lock = {
            let mut locks = self.0.lock().map_err(|_| "媒体下载锁已损坏")?;
            locks.retain(|_, lock| lock.strong_count() > 0);
            match locks.get(key).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(AsyncMutex::new(()));
                    locks.insert(key.to_owned(), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        Ok(lock.lock_owned().await)
    }
}

pub(super) struct Download {
    file: NamedTempFile,
    pub prefix: Vec<u8>,
    pub length: u64,
}

impl Download {
    pub async fn publish(self, destination: PathBuf) -> Result<String, String> {
        super::blocking(move || {
            // persist replaces atomically on the same filesystem, including Windows.
            self.file
                .persist(&destination)
                .map_err(|e| format!("保存媒体归档失败：{e}"))?;
            Ok(destination.to_string_lossy().into_owned())
        })
        .await
    }
}

/// Signed QQ CDN URLs usually work without cookies. Authenticate only when the
/// CDN rejects access; changing four header combinations never repairs a dead URL.
pub(super) async fn image(
    client: &reqwest::Client,
    directory: &Path,
    urls: &[url::Url],
    cookie: Option<&str>,
    user_agent: &str,
) -> Result<(Download, &'static str), String> {
    let mut last_error = "没有可用图片地址".to_owned();
    for url in urls {
        let host = url.host_str().unwrap_or_default();
        let cookie = cookie.filter(|_| host == "qq.com" || host.ends_with(".qq.com"));
        for authenticated in [false, true] {
            if authenticated && cookie.is_none() {
                break;
            }
            let mut request = client
                .get(url.clone())
                .header(reqwest::header::USER_AGENT, user_agent)
                .header(reqwest::header::REFERER, "https://user.qzone.qq.com/")
                .header(
                    reqwest::header::ACCEPT,
                    "image/avif,image/webp,image/png,image/jpeg,image/*,*/*;q=0.8",
                );
            if authenticated {
                request = request.header(reqwest::header::COOKIE, cookie.unwrap());
            }
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    last_error = format!("请求图片失败：{}", error.without_url());
                    break;
                }
            };
            let status = response.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err("图片服务限流，请稍后重试".into());
            }
            if !status.is_success() {
                last_error = format!("图片服务返回 HTTP {status}");
                if !authenticated && matches!(status.as_u16(), 401 | 403) {
                    continue;
                }
                break;
            }
            let download = match receive(response, directory).await {
                Ok(download) => download,
                Err(error) => {
                    last_error = error;
                    break;
                }
            };
            if let Some(extension) = super::archived_image_extension(&download.prefix) {
                if download.length > 32
                    && !super::is_qq_missing_image_placeholder_header(
                        &download.prefix,
                        download.length,
                    )
                {
                    return Ok((download, extension));
                }
                last_error = "QQ 返回图片不存在或无权访问的占位图".into();
            } else {
                last_error = "QQ 返回了非图片内容".into();
                break;
            }
        }
    }
    Err(format!("图片地址均加载失败：{last_error}"))
}

pub(super) async fn receive(
    mut response: reqwest::Response,
    directory: &Path,
) -> Result<Download, String> {
    let directory = directory.to_owned();
    let (temporary, file) = super::blocking(move || {
        let temporary = tempfile::Builder::new()
            .prefix("qvault-")
            .suffix(".part")
            .tempfile_in(directory)
            .map_err(|e| format!("创建媒体临时文件失败：{e}"))?;
        let file = temporary
            .reopen()
            .map_err(|e| format!("打开媒体临时文件失败：{e}"))?;
        Ok((temporary, file))
    })
    .await?;
    let mut writer = BufWriter::with_capacity(64 * 1024, tokio::fs::File::from_std(file));
    let mut prefix = Vec::with_capacity(32);
    let mut length = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取媒体数据失败：{e}"))?
    {
        length += chunk.len() as u64;
        let take = (32 - prefix.len()).min(chunk.len());
        prefix.extend_from_slice(&chunk[..take]);
        writer
            .write_all(&chunk)
            .await
            .map_err(|e| format!("写入媒体文件失败：{e}"))?;
    }
    writer
        .flush()
        .await
        .map_err(|e| format!("提交媒体文件失败：{e}"))?;
    drop(writer);
    Ok(Download {
        file: temporary,
        prefix,
        length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
    };

    // Local HTTP only; the producer also uses bounded memory.
    fn server(bytes: usize, advertised: Option<usize>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&socket);
            let mut line = String::new();
            loop {
                assert!(
                    reader.read_line(&mut line).unwrap() > 0,
                    "incomplete HTTP request"
                );
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            drop(reader);
            write!(socket, "HTTP/1.1 200 OK\r\nConnection: close\r\n").unwrap();
            if let Some(size) = advertised {
                write!(socket, "Content-Length: {size}\r\n").unwrap();
            }
            write!(socket, "\r\n").unwrap();
            let chunk = [7u8; 8192];
            let mut remaining = bytes;
            while remaining > 0 {
                let count = remaining.min(chunk.len());
                if socket.write_all(&chunk[..count]).is_err() {
                    break;
                }
                remaining -= count;
            }
        });
        (format!("http://{address}"), handle)
    }

    #[tokio::test]
    async fn image_falls_back_from_html_without_repeating_header_combinations() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let valid = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==",
        )
        .unwrap();
        let expected = valid.clone();
        let server = std::thread::spawn(move || {
            for body in [b"<html>expired image address</html>".to_vec(), valid] {
                let (mut socket, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&socket);
                let mut line = String::new();
                loop {
                    reader.read_line(&mut line).unwrap();
                    assert!(!line.to_lowercase().starts_with("cookie:"));
                    if line == "\r\n" {
                        break;
                    }
                    line.clear();
                }
                drop(reader);
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .unwrap();
                socket.write_all(&body).unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let urls = [
            url::Url::parse(&format!("http://{address}/stale")).unwrap(),
            url::Url::parse(&format!("http://{address}/fresh")).unwrap(),
        ];
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let (download, extension) = image(
            &client,
            dir.path(),
            &urls,
            Some("secret=never-send"),
            "test",
        )
        .await
        .unwrap();
        assert_eq!(extension, "gif");
        let path = dir.path().join("photo.gif");
        download.publish(path.clone()).await.unwrap();
        assert_eq!(std::fs::read(path).unwrap(), expected);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn image_rate_limit_stops_before_trying_other_addresses() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&socket);
            let mut line = String::new();
            loop {
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            drop(reader);
            write!(
                socket,
                "HTTP/1.1 429 Too Many Requests\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let urls = [
            url::Url::parse(&format!("http://{address}/limited")).unwrap(),
            url::Url::parse("http://127.0.0.1:1/must-not-be-requested").unwrap(),
        ];
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert!(image(&client, dir.path(), &urls, None, "test")
            .await
            .err()
            .unwrap()
            .contains("限流"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn large_transfer_publishes_only_after_completion_and_keeps_small_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("video.mp4");
        let size = 52 * 1024 * 1024;
        let (url, server) = server(size, Some(size));
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        let download = receive(response, dir.path()).await.unwrap();
        assert_eq!(download.length, size as u64);
        assert_eq!(download.prefix.len(), 32);
        assert!(!destination.exists());
        download.publish(destination.clone()).await.unwrap();
        assert_eq!(destination.metadata().unwrap().len(), size as u64);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn truncated_transfer_removes_partial_file_and_preserves_existing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("video.mp4");
        std::fs::write(&destination, b"previous complete video").unwrap();
        let (url, server) = server(4096, Some(8192));
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        assert!(receive(response, dir.path()).await.is_err());
        assert_eq!(
            std::fs::read(destination).unwrap(),
            b"previous complete video"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn streams_unknown_length_without_imposing_a_body_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (url, server) = server(32 * 1024, None);
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        assert_eq!(response.content_length(), None);
        let download = receive(response, dir.path()).await.unwrap();
        assert_eq!(download.length, 32 * 1024);
        drop(download);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn same_resource_is_serialized_without_blocking_other_resources() {
        let locks = DownloadLocks::default();
        let first = locks.acquire(Path::new("100/1")).await.unwrap();
        let lock = locks
            .0
            .lock()
            .unwrap()
            .get(Path::new("100/1"))
            .unwrap()
            .upgrade()
            .unwrap();
        assert!(lock.try_lock_owned().is_err());
        let other = locks.acquire(Path::new("100/2")).await.unwrap();
        drop(first);
        drop(other);
        let _next = locks.acquire(Path::new("200/1")).await.unwrap();
        assert_eq!(locks.0.lock().unwrap().len(), 1);
    }
}
