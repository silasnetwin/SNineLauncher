use crate::downloads::{DownloadRequest, DownloadService};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::Mutex as AsyncMutex;

const MAX_RUNTIME_ARCHIVE_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCandidate {
    pub executable: String,
    pub major: Option<u32>,
    pub architecture: Option<String>,
    pub valid: bool,
    pub compatible: bool,
    pub detail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub required_major: u32,
    pub compatible: Option<RuntimeCandidate>,
    pub candidates: Vec<RuntimeCandidate>,
    pub managed: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeResult {
    pub executable: String,
    pub major: u32,
    pub version: String,
    pub bytes: u64,
    pub sha256: String,
}

pub struct RuntimeService {
    root: PathBuf,
    downloads: Arc<DownloadService>,
    client: Client,
    cache: Mutex<HashMap<u32, (Instant, RuntimeStatus)>>,
    install_lock: AsyncMutex<()>,
}

#[derive(Deserialize)]
struct TemurinAsset {
    binary: TemurinBinary,
    version: TemurinVersion,
}
#[derive(Deserialize)]
struct TemurinBinary {
    architecture: String,
    os: String,
    image_type: String,
    package: TemurinPackage,
}
#[derive(Deserialize)]
struct TemurinPackage {
    name: String,
    link: String,
    checksum: String,
    size: u64,
}
#[derive(Deserialize)]
struct TemurinVersion {
    major: u32,
    openjdk_version: String,
}

impl RuntimeService {
    pub fn new(root: PathBuf, downloads: Arc<DownloadService>) -> Result<Self, reqwest::Error> {
        Ok(Self {
            root,
            downloads,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("SNineLauncher/", env!("CARGO_PKG_VERSION")))
                .http2_adaptive_window(true)
                .build()?,
            cache: Mutex::new(HashMap::new()),
            install_lock: AsyncMutex::new(()),
        })
    }

    pub fn detect(&self, required_major: u32) -> RuntimeStatus {
        if let Ok(cache) = self.cache.lock() {
            if let Some((at, status)) = cache.get(&required_major) {
                if at.elapsed() < Duration::from_secs(30) {
                    return status.clone();
                }
            }
        }
        let mut paths = discover();
        let managed_path = self.managed_java_path(required_major);
        paths.insert(managed_path.clone());
        let mut candidates = paths
            .into_iter()
            .map(|path| inspect(path, required_major))
            .collect::<Vec<_>>();
        let compatible_candidate = |candidate: &&RuntimeCandidate| {
            candidate.valid && candidate.compatible && is_x64(candidate)
        };
        // Prefer the launcher-managed runtime for deterministic launches.
        let compatible = candidates
            .iter()
            .find(|candidate| {
                compatible_candidate(candidate) && same_path(&candidate.executable, &managed_path)
            })
            .or_else(|| candidates.iter().find(compatible_candidate))
            .cloned();
        let managed = compatible
            .as_ref()
            .is_some_and(|candidate| same_path(&candidate.executable, &managed_path));
        if compatible.is_none() {
            candidates.sort_by(|a, b| a.executable.cmp(&b.executable));
        }
        let status = RuntimeStatus {
            required_major,
            compatible,
            candidates,
            managed,
        };
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(required_major, (Instant::now(), status.clone()));
        }
        status
    }

    pub async fn install_managed_java<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        major: u32,
    ) -> Result<ManagedRuntimeResult, String> {
        if !matches!(major, 21 | 25) {
            return Err(format!(
                "Managed Java {major} installation is not supported."
            ));
        }
        let _guard = self.install_lock.lock().await;
        if let Some(runtime) = self.detect(major).compatible {
            return Ok(ManagedRuntimeResult {
                executable: runtime.executable,
                major,
                version: runtime.detail,
                bytes: 0,
                sha256: String::new(),
            });
        }
        emit_runtime_stage(app, "resolving", None);
        let response = self
            .client
            .get(format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows"))
            .send()
            .await
            .map_err(|_| "Could not reach the Temurin runtime service.".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Temurin runtime service returned HTTP {}",
                response.status()
            ));
        }
        let mut assets = response
            .json::<Vec<TemurinAsset>>()
            .await
            .map_err(|_| "Temurin returned invalid runtime metadata.".to_string())?;
        let asset = assets
            .drain(..)
            .find(|asset| {
                asset.binary.os.eq_ignore_ascii_case("windows")
                    && asset.binary.architecture.eq_ignore_ascii_case("x64")
                    && asset.binary.image_type.eq_ignore_ascii_case("jre")
                    && asset
                        .binary
                        .package
                        .name
                        .to_ascii_lowercase()
                        .ends_with(".zip")
                    && asset.version.major == major
                    && valid_sha256(&asset.binary.package.checksum)
                    && valid_package_url(&asset.binary.package.link)
                    && asset.binary.package.size > 0
                    && asset.binary.package.size <= MAX_RUNTIME_ARCHIVE_BYTES
            })
            .ok_or_else(|| {
                format!("No verified Windows x64 Java {major} runtime package is available.")
            })?;
        let archive_size = asset.binary.package.size;
        let available = fs2::available_space(&self.root)
            .map_err(|_| format!("Could not check free disk space for Java {major}."))?;
        let required = archive_size
            .saturating_mul(5)
            .saturating_add(64 * 1024 * 1024);
        if available < required {
            return Err(format!(
                "DiskFull: Java {major} needs about {required} bytes of free space."
            ));
        }

        let managed_root = self.managed_root(major);
        let staging = self
            .root
            .join("runtimes")
            .join(format!(".java-{major}-staging"));
        fs::create_dir_all(staging.parent().unwrap_or(&self.root))
            .map_err(|_| "Could not prepare the managed Java directory.".to_string())?;
        if staging.exists() {
            validate_child_path(&self.root.join("runtimes"), &staging)?;
            fs::remove_dir_all(&staging)
                .map_err(|_| "Could not clear incomplete Java installation staging.".to_string())?;
        }
        fs::create_dir_all(&staging)
            .map_err(|_| "Could not create Java installation staging.".to_string())?;
        let archive = staging.join(&asset.binary.package.name);
        let request = DownloadRequest {
            url: asset.binary.package.link.clone(),
            file_name: asset.binary.package.name.clone(),
            expected_sha256: Some(asset.binary.package.checksum.clone()),
            profile_id: None,
            max_bytes: Some(archive_size),
        };
        let receipt = match self.downloads.download_to(app, &request, &archive).await {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!("Java {major} download failed: {error}"));
            }
        };
        if receipt.bytes != archive_size {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!(
                "Java {major} download size did not match its published metadata."
            ));
        }

        emit_runtime_stage(
            app,
            "installing",
            Some(if major == 21 {
                "Extracting verified Java 21 runtime"
            } else {
                "Extracting verified Java 25 runtime"
            }),
        );
        let extraction = staging.join("runtime");
        let archive_for_extract = archive.clone();
        let extraction_for_extract = extraction.clone();
        tokio::task::spawn_blocking(move || {
            safe_extract_zip(&archive_for_extract, &extraction_for_extract)
        })
        .await
        .map_err(|_| format!("Java {major} extraction task failed."))??;
        fs::remove_file(&archive)
            .map_err(|_| "Could not remove temporary Java archive.".to_string())?;
        let executable = find_java_executable(&extraction).ok_or_else(|| {
            "Verified Java archive did not contain a Java executable.".to_string()
        })?;
        let candidate = inspect(executable.clone(), major);
        if !candidate.valid || !candidate.compatible || !is_x64(&candidate) {
            let _ = fs::remove_dir_all(&staging);
            return Err("Installed Java runtime failed version or architecture validation.".into());
        }
        let relative_java = executable
            .strip_prefix(&extraction)
            .map_err(|_| "Java executable escaped staging directory.".to_string())?;
        let metadata = serde_json::json!({
            "vendor": "Eclipse Temurin",
            "major": major,
            "version": asset.version.openjdk_version,
            "architecture": "x64",
            "source": "api.adoptium.net",
            "packageSha256": receipt.sha256,
            "javaPath": relative_java,
            "verifiedAt": unix_now()
        });
        fs::write(
            extraction.join("snine-runtime.json"),
            serde_json::to_vec_pretty(&metadata).unwrap_or_default(),
        )
        .map_err(|_| "Could not write Java runtime metadata.".to_string())?;

        let runtime_parent = managed_root
            .parent()
            .ok_or("Invalid managed runtime path")?;
        fs::create_dir_all(runtime_parent)
            .map_err(|_| "Could not prepare managed runtime destination.".to_string())?;
        let publish = runtime_parent.join(format!("java-{major}-publish"));
        if publish.exists() {
            validate_child_path(runtime_parent, &publish)?;
            fs::remove_dir_all(&publish)
                .map_err(|_| "Could not clean previous runtime staging.".to_string())?;
        }
        fs::rename(&extraction, &publish)
            .map_err(|_| "Could not prepare Java runtime for activation.".to_string())?;
        let backup = runtime_parent.join(format!("java-{major}-previous"));
        let had_previous = managed_root.exists();
        if had_previous {
            validate_child_path(runtime_parent, &managed_root)?;
            if backup.exists() {
                validate_child_path(runtime_parent, &backup)?;
                fs::remove_dir_all(&backup)
                    .map_err(|_| "Could not rotate previous managed runtime.".to_string())?;
            }
            fs::rename(&managed_root, &backup)
                .map_err(|_| "Could not preserve the previous managed runtime.".to_string())?;
        }
        if let Err(error) = fs::rename(&publish, &managed_root) {
            if had_previous {
                let _ = fs::rename(&backup, &managed_root);
            }
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("Could not activate Java {major} runtime: {error}"));
        }
        if had_previous {
            let _ = fs::remove_dir_all(backup);
        }
        let _ = fs::remove_dir_all(&staging);
        self.invalidate_cache();
        emit_runtime_stage(
            app,
            "ready",
            Some(if major == 21 {
                "Java 21 is installed and verified"
            } else {
                "Java 25 is installed and verified"
            }),
        );
        Ok(ManagedRuntimeResult {
            executable: self.managed_java_path(major).display().to_string(),
            major,
            version: asset.version.openjdk_version,
            bytes: receipt.bytes,
            sha256: receipt.sha256,
        })
    }

    fn managed_root(&self, major: u32) -> PathBuf {
        self.root.join("runtimes").join(format!("java-{major}"))
    }

    fn managed_java_path(&self, major: u32) -> PathBuf {
        let root = self.managed_root(major);
        let metadata = fs::read(root.join("snine-runtime.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        metadata
            .and_then(|value| value.get("javaPath")?.as_str().map(PathBuf::from))
            .filter(|relative| {
                relative
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
            })
            .map(|relative| root.join(relative))
            .unwrap_or_else(|| root.join("bin").join("java.exe"))
    }

    fn invalidate_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

fn emit_runtime_stage<R: Runtime>(app: &AppHandle<R>, state: &str, detail: Option<&str>) {
    let _ = app.emit(
        "runtime-install-progress",
        serde_json::json!({"state":state,"detail":detail}),
    );
}

fn discover() -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    if let Some(home) = env::var_os("JAVA_HOME") {
        paths.insert(java_executable(PathBuf::from(home)));
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join(if cfg!(windows) { "java.exe" } else { "java" });
            if candidate.is_file() {
                paths.insert(candidate);
            }
        }
    }
    #[cfg(windows)]
    {
        for root in [
            env::var_os("ProgramFiles").map(PathBuf::from),
            env::var_os("ProgramFiles(x86)").map(PathBuf::from),
            env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("Programs")),
        ]
        .into_iter()
        .flatten()
        {
            for vendor in ["Java", "Eclipse Adoptium", "Microsoft"] {
                let vendor_root = root.join(vendor);
                if let Ok(versions) = fs::read_dir(vendor_root) {
                    for version in versions.flatten() {
                        let candidate = version.path().join("bin").join("java.exe");
                        if candidate.is_file() {
                            paths.insert(candidate);
                        }
                    }
                }
            }
        }
    }
    paths
}

fn java_executable(home: PathBuf) -> PathBuf {
    if home.is_dir() {
        home.join("bin")
            .join(if cfg!(windows) { "java.exe" } else { "java" })
    } else {
        home
    }
}

fn inspect(path: PathBuf, required_major: u32) -> RuntimeCandidate {
    let mut command = Command::new(&path);
    command.arg("-XshowSettings:properties").arg("-version");
    // Runtime discovery runs Java probes in the background. Keep those short-lived
    // console processes from flashing a terminal window on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command.output();
    let result = output
        .map_err(|error| error.to_string())
        .and_then(|output| {
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if !output.status.success() {
                return Err(format!("java -version exited with {}", output.status));
            }
            let version = text.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("java.version = ")
                    .or_else(|| line.trim().strip_prefix("openjdk version \""))
                    .or_else(|| line.trim().strip_prefix("java version \""))
                    .map(|value| value.trim_end_matches('"').to_owned())
            });
            let architecture = text.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("os.arch = ")
                    .map(str::trim)
                    .map(str::to_owned)
            });
            let major = version.as_deref().and_then(parse_major);
            Ok((major, architecture))
        });
    match result {
        Ok((major, architecture)) => {
            let compatible = compatible_major(major, required_major)
                && architecture.as_deref().is_some_and(is_x64_arch);
            RuntimeCandidate {
                executable: path.display().to_string(),
                major,
                architecture,
                valid: major.is_some(),
                compatible,
                detail: major
                    .map(|value| format!("Java {value}"))
                    .unwrap_or_else(|| "Could not parse Java version".into()),
            }
        }
        Err(detail) => RuntimeCandidate {
            executable: path.display().to_string(),
            major: None,
            architecture: None,
            valid: false,
            compatible: false,
            detail,
        },
    }
}

fn compatible_major(actual: Option<u32>, required: u32) -> bool {
    actual == Some(required)
}

fn is_x64(candidate: &RuntimeCandidate) -> bool {
    candidate.architecture.as_deref().is_some_and(is_x64_arch)
}

fn is_x64_arch(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "amd64" | "x86_64")
}

fn same_path(left: &str, right: &Path) -> bool {
    Path::new(left).canonicalize().ok() == right.canonicalize().ok()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_package_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
            && (url
                .path()
                .starts_with("/adoptium/temurin21-binaries/releases/download/")
                || url
                    .path()
                    .starts_with("/adoptium/temurin25-binaries/releases/download/"))
    })
}

fn validate_child_path(parent: &Path, child: &Path) -> Result<(), String> {
    let parent = parent
        .canonicalize()
        .map_err(|_| "Managed runtime parent is unavailable".to_string())?;
    let actual = child
        .canonicalize()
        .map_err(|_| "Managed runtime path is unavailable".to_string())?;
    if !actual.starts_with(&parent) || actual == parent {
        return Err("Managed runtime path escaped its controlled root".into());
    }
    Ok(())
}

fn safe_extract_zip(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path)
        .map_err(|_| "Could not open verified Java archive".to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| "Verified Java archive is invalid".to_string())?;
    let mut total_unpacked = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "Could not read Java archive entry".to_string())?;
        total_unpacked = total_unpacked.saturating_add(entry.size());
        if entry.size() > 256 * 1024 * 1024 || total_unpacked > 1024 * 1024 * 1024 {
            return Err("Java archive exceeds the safe extraction size limit".into());
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| "Java archive contains an unsafe path".to_string())?
            .to_path_buf();
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Java archive contains an unsupported symbolic link".into());
        }
        let output = destination.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&output)
                .map_err(|_| "Could not create Java runtime directory".to_string())?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .map_err(|_| "Could not create Java runtime directory".to_string())?;
            }
            let mut file = fs::File::create(&output)
                .map_err(|_| "Could not create Java runtime file".to_string())?;
            std::io::copy(&mut entry, &mut file)
                .map_err(|_| "Could not extract Java runtime file".to_string())?;
        }
    }
    Ok(())
}

fn find_java_executable(root: &Path) -> Option<PathBuf> {
    fn visit(path: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 5 {
            return None;
        }
        for entry in fs::read_dir(path).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("java.exe"))
            {
                return Some(path);
            }
            if entry.file_type().ok()?.is_dir() {
                if let Some(found) = visit(&path, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    visit(root, 0)
}

fn parse_major(value: &str) -> Option<u32> {
    let first = value.split(['.', '+', '-']).next()?.parse::<u32>().ok()?;
    if first == 1 {
        value.split('.').nth(1)?.parse().ok()
    } else {
        Some(first)
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{compatible_major, is_x64_arch, parse_major, valid_package_url, valid_sha256};

    #[test]
    fn parses_modern_and_legacy_java_versions() {
        assert_eq!(parse_major("21.0.2+13"), Some(21));
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("not-java"), None);
        assert!(compatible_major(parse_major("21.0.2"), 21));
        assert!(!compatible_major(parse_major("25.0.1"), 21));
    }

    #[test]
    fn validates_managed_runtime_metadata() {
        assert!(valid_sha256(&"a".repeat(64)));
        assert!(!valid_sha256("not-a-hash"));
        assert!(valid_package_url(
            "https://github.com/adoptium/temurin21-binaries/releases/download/build/file.zip"
        ));
        assert!(valid_package_url(
            "https://github.com/adoptium/temurin25-binaries/releases/download/build/file.zip"
        ));
        assert!(!valid_package_url(
            "http://github.com/adoptium/temurin21-binaries/releases/download/build/file.zip"
        ));
        assert!(!valid_package_url(
            "https://example.com/adoptium/temurin21-binaries/releases/download/build/file.zip"
        ));
        assert!(is_x64_arch("amd64"));
        assert!(is_x64_arch("x86_64"));
        assert!(!is_x64_arch("aarch64"));
    }

    #[test]
    fn rejects_traversal_entries_in_managed_runtime_archives() {
        let path =
            std::env::temp_dir().join(format!("snine-runtime-slip-{}.zip", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("../escaped.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        use std::io::Write;
        writer.write_all(b"not extracted").unwrap();
        writer.finish().unwrap();
        let destination =
            std::env::temp_dir().join(format!("snine-runtime-extract-{}", uuid::Uuid::new_v4()));
        assert!(super::safe_extract_zip(&path, &destination).is_err());
        assert!(!destination.parent().unwrap().join("escaped.txt").exists());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(destination);
    }
}
