use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode, Url};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
    time::sleep,
};
use tokio_util::sync::CancellationToken;

const MAX_PARALLEL_DOWNLOADS: usize = 4;
const MAX_ATTEMPTS: u8 = 3;

#[derive(Clone)]
pub struct DownloadService {
    client: Client,
    root: PathBuf,
    slots: Arc<Mutex<Arc<Semaphore>>>,
    cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
    retry_requests: Arc<Mutex<HashMap<String, DownloadRequest>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub file_name: String,
    pub state: String,
    pub bytes_received: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    pub error: Option<String>,
    pub profile_id: Option<String>,
}

#[derive(Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub file_name: String,
    pub expected_sha256: Option<String>,
    pub profile_id: Option<String>,
    pub max_bytes: Option<u64>,
}

pub struct DownloadReceipt {
    pub id: String,
    pub bytes: u64,
    pub sha256: String,
}

impl DownloadService {
    pub fn new(root: PathBuf) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .user_agent(concat!("SNineLauncher/", env!("CARGO_PKG_VERSION")))
                .build()?,
            root,
            slots: Arc::new(Mutex::new(Arc::new(Semaphore::new(MAX_PARALLEL_DOWNLOADS)))),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            retry_requests: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Updates the bounded concurrency used by future queue acquisitions.
    /// Already-running transfers retain their permit from the previous limit.
    pub fn set_concurrency(&self, limit: usize) -> Result<(), String> {
        if !(1..=16).contains(&limit) {
            return Err("Download concurrency must be between 1 and 16".into());
        }
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| "Download queue is unavailable")?;
        *slots = Arc::new(Semaphore::new(limit));
        Ok(())
    }

    fn semaphore(&self) -> Result<Arc<Semaphore>, String> {
        self.slots
            .lock()
            .map(|slots| Arc::clone(&slots))
            .map_err(|_| "Download queue is unavailable".into())
    }

    pub fn enqueue<R: Runtime>(
        self: &Arc<Self>,
        app: AppHandle<R>,
        request: DownloadRequest,
    ) -> Result<String, String> {
        validate_request(&request)?;
        let id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut requests) = self.retry_requests.lock() {
            if requests.len() >= 100 {
                if let Some(oldest) = requests.keys().next().cloned() {
                    requests.remove(&oldest);
                }
            }
            requests.insert(id.clone(), request.clone());
        }
        let token = CancellationToken::new();
        self.cancellations
            .lock()
            .map_err(|_| "Download queue is unavailable".to_string())?
            .insert(id.clone(), token.clone());
        emit_progress(
            &app,
            DownloadProgress {
                id: id.clone(),
                file_name: request.file_name.clone(),
                state: "queued".into(),
                bytes_received: 0,
                total_bytes: None,
                bytes_per_second: 0,
                eta_seconds: None,
                error: None,
                profile_id: request.profile_id.clone(),
            },
        );

        let service = Arc::clone(self);
        let job_id = id.clone();
        tokio::spawn(async move {
            let slots = service.semaphore();
            let result = tokio::select! {
                _ = token.cancelled() => Err(TransferError::Cancelled),
                permit = async { match slots { Ok(slots) => slots.acquire_owned().await.map_err(|_| ()), Err(_) => Err(()) } } => match permit {
                    Ok(_permit) => service.transfer(&app, &job_id, &request, &token).await,
                    Err(_) => Err(TransferError::Failed("Download queue unavailable".into())),
                }
            };
            if let Err(error) = result {
                let state = if matches!(error, TransferError::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                };
                emit_progress(
                    &app,
                    DownloadProgress {
                        id: job_id.clone(),
                        file_name: request.file_name.clone(),
                        state: state.into(),
                        bytes_received: 0,
                        total_bytes: None,
                        bytes_per_second: 0,
                        eta_seconds: None,
                        error: if state == "failed" {
                            Some(error.to_string())
                        } else {
                            None
                        },
                        profile_id: request.profile_id.clone(),
                    },
                );
            }
            if let Ok(mut jobs) = service.cancellations.lock() {
                jobs.remove(&job_id);
            }
        });
        Ok(id)
    }

    pub fn retry<R: Runtime>(
        self: &Arc<Self>,
        app: AppHandle<R>,
        failed_id: &str,
    ) -> Result<String, String> {
        let request = self
            .retry_requests
            .lock()
            .map_err(|_| "Download retry history is unavailable".to_string())?
            .get(failed_id)
            .cloned()
            .ok_or_else(|| {
                "This download can no longer be retried. Start it again from the profile."
                    .to_string()
            })?;
        self.enqueue(app, request)
    }

    pub fn cancel(&self, id: &str) -> Result<bool, String> {
        let jobs = self
            .cancellations
            .lock()
            .map_err(|_| "Download queue is unavailable".to_string())?;
        if let Some(token) = jobs.get(id) {
            token.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Streams a file to a caller-selected managed staging path using the shared
    /// client, bounded transfer slots, retry policy and resumable partial file.
    pub async fn download_to<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        request: &DownloadRequest,
        destination: &Path,
    ) -> Result<DownloadReceipt, String> {
        self.download_to_inner(app, request, destination, true)
            .await
    }

    /// Same shared streaming/resume/checksum pipeline as `download_to`, while
    /// allowing a parent installer to publish one coalesced aggregate task.
    pub async fn download_to_quiet<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        request: &DownloadRequest,
        destination: &Path,
    ) -> Result<DownloadReceipt, String> {
        self.download_to_inner(app, request, destination, false)
            .await
    }

    async fn download_to_inner<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        request: &DownloadRequest,
        destination: &Path,
        publish_events: bool,
    ) -> Result<DownloadReceipt, String> {
        validate_request(request)?;
        let id = uuid::Uuid::new_v4().to_string();
        let token = CancellationToken::new();
        self.cancellations
            .lock()
            .map_err(|_| "Download queue is unavailable".to_string())?
            .insert(id.clone(), token.clone());
        if publish_events {
            emit_progress(
                app,
                DownloadProgress {
                    id: id.clone(),
                    file_name: request.file_name.clone(),
                    state: "queued".into(),
                    bytes_received: 0,
                    total_bytes: None,
                    bytes_per_second: 0,
                    eta_seconds: None,
                    error: None,
                    profile_id: request.profile_id.clone(),
                },
            );
        }
        let outcome = tokio::select! {
            _ = token.cancelled() => Err(TransferError::Cancelled),
            permit = self.semaphore()?.acquire_owned() => match permit {
                Ok(_permit) => self.transfer_to(app, &id, request, destination, &token, publish_events).await,
                Err(_) => Err(TransferError::Failed("Download queue stopped".into())),
            }
        };
        if let Ok(mut jobs) = self.cancellations.lock() {
            jobs.remove(&id);
        }
        if publish_events {
            if let Err(error) = &outcome {
                let cancelled = matches!(error, TransferError::Cancelled);
                emit_progress(
                    app,
                    DownloadProgress {
                        id: id.clone(),
                        file_name: request.file_name.clone(),
                        state: if cancelled { "cancelled" } else { "failed" }.into(),
                        bytes_received: 0,
                        total_bytes: None,
                        bytes_per_second: 0,
                        eta_seconds: None,
                        error: if cancelled {
                            None
                        } else {
                            Some(error.to_string())
                        },
                        profile_id: request.profile_id.clone(),
                    },
                );
            }
        }
        outcome.map_err(|error| error.to_string())
    }

    async fn transfer_to<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        request: &DownloadRequest,
        destination: &Path,
        token: &CancellationToken,
        publish_events: bool,
    ) -> Result<DownloadReceipt, TransferError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await.map_err(io_error)?;
        }
        let partial = destination.with_extension("part");
        let url = Url::parse(&request.url).map_err(|e| TransferError::Failed(e.to_string()))?;
        let mut last_error = None;
        for attempt in 0..MAX_ATTEMPTS {
            if token.is_cancelled() {
                return Err(TransferError::Cancelled);
            }
            match self
                .transfer_once(app, id, request, &url, &partial, token, publish_events)
                .await
            {
                Ok(()) => {
                    let (bytes, sha256) = hash_file(&partial).await.map_err(io_error)?;
                    publish_replace(&partial, destination)
                        .await
                        .map_err(io_error)?;
                    if publish_events {
                        emit_progress(
                            app,
                            DownloadProgress {
                                id: id.into(),
                                file_name: request.file_name.clone(),
                                state: "downloaded".into(),
                                bytes_received: bytes,
                                total_bytes: Some(bytes),
                                bytes_per_second: 0,
                                eta_seconds: Some(0),
                                error: None,
                                profile_id: request.profile_id.clone(),
                            },
                        );
                    }
                    return Ok(DownloadReceipt {
                        id: id.into(),
                        bytes,
                        sha256,
                    });
                }
                Err(TransferError::Cancelled) => return Err(TransferError::Cancelled),
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < MAX_ATTEMPTS {
                        sleep(Duration::from_millis(250 * (1 << attempt))).await;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| TransferError::Failed("Download failed".into())))
    }

    async fn transfer<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        request: &DownloadRequest,
        token: &CancellationToken,
    ) -> Result<(), TransferError> {
        fs::create_dir_all(&self.root).await.map_err(io_error)?;
        let destination = self.root.join(&request.file_name);
        if fs::try_exists(&destination).await.map_err(io_error)? {
            return Err(TransferError::Failed(
                "A file with that name already exists".into(),
            ));
        }
        let partial = self.root.join(format!("{}.part", request.file_name));
        let url = Url::parse(&request.url).map_err(|e| TransferError::Failed(e.to_string()))?;
        let mut last_error = None;
        for attempt in 0..MAX_ATTEMPTS {
            if token.is_cancelled() {
                return Err(TransferError::Cancelled);
            }
            match self
                .transfer_once(app, id, request, &url, &partial, token, true)
                .await
            {
                Ok(()) => {
                    fs::rename(&partial, &destination).await.map_err(io_error)?;
                    emit_progress(
                        app,
                        DownloadProgress {
                            id: id.into(),
                            file_name: request.file_name.clone(),
                            state: "completed".into(),
                            bytes_received: fs::metadata(&destination)
                                .await
                                .map_err(io_error)?
                                .len(),
                            total_bytes: fs::metadata(&destination).await.ok().map(|m| m.len()),
                            bytes_per_second: 0,
                            eta_seconds: Some(0),
                            error: None,
                            profile_id: request.profile_id.clone(),
                        },
                    );
                    return Ok(());
                }
                Err(TransferError::Cancelled) => return Err(TransferError::Cancelled),
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < MAX_ATTEMPTS {
                        sleep(Duration::from_millis(250 * (1 << attempt))).await;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| TransferError::Failed("Download failed".into())))
    }

    // Keeping transfer metadata explicit makes resume/range behavior auditable.
    #[allow(clippy::too_many_arguments)]
    async fn transfer_once<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        request: &DownloadRequest,
        url: &Url,
        partial: &Path,
        token: &CancellationToken,
        publish_events: bool,
    ) -> Result<(), TransferError> {
        let offset = match fs::metadata(partial).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(io_error(error)),
        };
        let mut builder = self.client.get(url.clone());
        if offset > 0 {
            builder = builder.header(header::RANGE, format!("bytes={offset}-"));
        }
        let response = tokio::select! {
            _ = token.cancelled() => return Err(TransferError::Cancelled),
            result = builder.send() => result.map_err(|e| TransferError::Failed(e.to_string()))?,
        };
        let status = response.status();
        if !(status.is_success() || (offset > 0 && status == StatusCode::PARTIAL_CONTENT)) {
            return Err(TransferError::Failed(format!(
                "Server returned HTTP {status}"
            )));
        }
        let append = offset > 0 && status == StatusCode::PARTIAL_CONTENT;
        if append {
            let range_start = response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(content_range_start);
            if range_start != Some(offset) {
                fs::remove_file(partial).await.map_err(io_error)?;
                return Err(TransferError::Failed(
                    "Discarded partial file because the server returned an invalid byte range"
                        .into(),
                ));
            }
        }
        let start = if append { offset } else { 0 };
        let total = response
            .content_length()
            .map(|length| length.saturating_add(start));
        let mut hasher = Sha256::new();
        if append {
            let mut old_file = File::open(partial).await.map_err(io_error)?;
            let mut buffer = vec![0; 64 * 1024];
            loop {
                let count = old_file.read(&mut buffer).await.map_err(io_error)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(partial)
            .await
            .map_err(io_error)?;
        let mut stream = response.bytes_stream();
        let mut received = start;
        let mut last_emit = Instant::now();
        let started = Instant::now();
        let mut last_sample_bytes = received;
        let mut last_sample_time = started;
        let mut speed = 0u64;
        loop {
            let chunk = tokio::select! {
                _ = token.cancelled() => return Err(TransferError::Cancelled),
                next = stream.next() => next,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|e| TransferError::Failed(e.to_string()))?;
            file.write_all(&chunk).await.map_err(io_error)?;
            hasher.update(&chunk);
            received += chunk.len() as u64;
            if request.max_bytes.is_some_and(|limit| received > limit) {
                return Err(TransferError::Failed(
                    "Download exceeded its declared size limit".into(),
                ));
            }
            if last_sample_time.elapsed() >= Duration::from_millis(500) {
                let elapsed = last_sample_time.elapsed().as_secs_f64();
                speed = ((received - last_sample_bytes) as f64 / elapsed) as u64;
                last_sample_bytes = received;
                last_sample_time = Instant::now();
            }
            if publish_events && last_emit.elapsed() >= Duration::from_millis(180) {
                emit_progress(app, progress(id, request, received, total, speed));
                last_emit = Instant::now();
            }
        }
        file.flush().await.map_err(io_error)?;
        file.sync_all().await.map_err(io_error)?;
        if let Some(expected) = &request.expected_sha256 {
            let actual = format!("{:x}", hasher.finalize());
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(TransferError::Failed(
                    "SHA-256 checksum did not match".into(),
                ));
            }
        }
        if publish_events {
            emit_progress(app, progress(id, request, received, total, speed));
        }
        Ok(())
    }
}

fn progress(
    id: &str,
    request: &DownloadRequest,
    received: u64,
    total: Option<u64>,
    speed: u64,
) -> DownloadProgress {
    DownloadProgress {
        id: id.into(),
        file_name: request.file_name.clone(),
        state: "downloading".into(),
        bytes_received: received,
        total_bytes: total,
        bytes_per_second: speed,
        eta_seconds: total
            .filter(|_| speed > 0)
            .map(|total| total.saturating_sub(received) / speed),
        error: None,
        profile_id: request.profile_id.clone(),
    }
}

async fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        hasher.update(&buffer[..count]);
    }
    Ok((bytes, format!("{:x}", hasher.finalize())))
}

fn content_range_start(value: &str) -> Option<u64> {
    let range = value.strip_prefix("bytes ")?.split('/').next()?;
    range.split('-').next()?.parse().ok()
}

async fn publish_replace(partial: &Path, destination: &Path) -> std::io::Result<()> {
    if !fs::try_exists(destination).await? {
        return fs::rename(partial, destination).await;
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    let backup = destination.with_file_name(format!(".{name}.{}.previous", uuid::Uuid::new_v4()));
    fs::rename(destination, &backup).await?;
    if let Err(error) = fs::rename(partial, destination).await {
        let _ = fs::rename(&backup, destination).await;
        return Err(error);
    }
    fs::remove_file(backup).await
}

fn emit_progress<R: Runtime>(app: &AppHandle<R>, progress: DownloadProgress) {
    let _ = app.emit("download-progress", progress);
}

fn validate_request(request: &DownloadRequest) -> Result<(), String> {
    let url = Url::parse(&request.url).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS downloads are supported".into());
    }
    let path = Path::new(&request.file_name);
    if request.file_name.is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err("Download file name must be a single safe path component".into());
    }
    if let Some(hash) = &request.expected_sha256 {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Expected SHA-256 must contain 64 hexadecimal characters".into());
        }
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> TransferError {
    TransferError::Failed(error.to_string())
}

#[derive(Debug)]
enum TransferError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("cancelled"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_request, DownloadRequest};

    fn request(file_name: &str) -> DownloadRequest {
        DownloadRequest {
            url: "https://downloads.example.test/client.zip".into(),
            file_name: file_name.into(),
            expected_sha256: None,
            profile_id: None,
            max_bytes: None,
        }
    }

    #[test]
    fn accepts_safe_filename_and_http_url() {
        assert!(validate_request(&request("client.zip")).is_ok());
    }

    #[test]
    fn rejects_path_traversal_and_non_http_protocols() {
        assert!(validate_request(&request("../client.zip")).is_err());
        assert!(validate_request(&DownloadRequest {
            url: "file:///client.zip".into(),
            file_name: "client.zip".into(),
            expected_sha256: None,
            profile_id: None,
            max_bytes: None,
        })
        .is_err());
    }

    #[test]
    fn validates_optional_sha256() {
        let mut item = request("client.zip");
        item.expected_sha256 = Some("a".repeat(64));
        assert!(validate_request(&item).is_ok());
        item.expected_sha256 = Some("not-a-hash".into());
        assert!(validate_request(&item).is_err());
    }

    #[test]
    fn parses_content_range_offset() {
        assert_eq!(super::content_range_start("bytes 123-456/789"), Some(123));
        assert_eq!(super::content_range_start("bytes */789"), None);
    }
}
