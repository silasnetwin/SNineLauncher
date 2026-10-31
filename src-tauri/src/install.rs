use crate::{
    database::Database,
    downloads::{DownloadRequest, DownloadService},
};
use reqwest::{header, Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::fs;

const CLIENT_RELEASES_API: &str =
    "https://api.github.com/repos/silasnetwin/SNineLauncher/releases?per_page=100";
const CLIENT_UPDATE_MANIFEST_URL: &str = "https://snc.silasnet.win/updates/manifest.json";
const CLIENT_RELEASE_PREFIX: &str = "client-v";
const MAX_CLIENT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct Installer {
    root: PathBuf,
    client: Client,
    downloads: Arc<DownloadService>,
    db: Arc<Database>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub profile_id: String,
    pub client_version: String,
    pub minecraft_version: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientRequirements {
    mod_id: String,
    client_version: String,
    minecraft_version: String,
    java_requirement: Option<String>,
    fabric_loader_requirement: Option<String>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct ClientRelease {
    version: String,
    size: u64,
    sha256: String,
    download_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct ClientReleaseManifest {
    version: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    sha256: String,
    #[serde(rename = "assetName")]
    #[serde(default)]
    asset_name: String,
    #[serde(default)]
    artifacts: BTreeMap<String, ClientReleaseArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HomeClientUpdateManifest {
    schema_version: u8,
    channel: String,
    version: String,
    artifacts: BTreeMap<String, HomeClientArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HomeClientArtifact {
    minecraft_version: String,
    version: String,
    size: u64,
    sha256: String,
    asset_name: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct ClientReleaseArtifact {
    size: u64,
    sha256: String,
    #[serde(rename = "assetName")]
    asset_name: String,
}

impl Installer {
    pub fn new(
        root: PathBuf,
        downloads: Arc<DownloadService>,
        db: Arc<Database>,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            root,
            downloads,
            db,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .user_agent(concat!("SNineLauncher/", env!("CARGO_PKG_VERSION")))
                .build()?,
        })
    }

    pub async fn install_client(
        &self,
        app: &AppHandle,
        profile_id: String,
    ) -> Result<InstallResult, String> {
        validate_profile_id(&profile_id)?;
        if !self
            .db
            .profile_exists(&profile_id)
            .map_err(|e| e.to_string())?
        {
            return Err("Unknown profile".into());
        }
        let target_profile = self
            .db
            .bootstrap()
            .map_err(|e| e.to_string())?
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or("Unknown profile")?;
        if !matches!(
            target_profile.minecraft_version.as_str(),
            "1.21.11" | "26.1.2" | "26.3"
        ) {
            return Err(
                "SNineClient has no launcher-managed build for this Minecraft version.".into(),
            );
        }
        emit_stage(app, &profile_id, "resolving", None);
        let release = self
            .resolve_release(&target_profile.minecraft_version)
            .await?;
        let version = release.version.clone();
        let expected_size = release.size;
        if expected_size == 0 || expected_size > MAX_CLIENT_BYTES {
            return Err("SNine client artifact has an invalid or unsupported size".into());
        }
        let canonical_profile = crate::minecraft::ensure_profile_root(&self.root, &profile_id)?;
        let profile_root = canonical_profile.clone();
        let available = fs2::available_space(&canonical_profile)
            .map_err(|e| format!("Could not check free disk space: {e}"))?;
        let required = expected_size
            .saturating_mul(2)
            .saturating_add(16 * 1024 * 1024);
        if available < required {
            return Err(format!("DiskFull: client install needs {required} bytes; only {available} bytes are available"));
        }

        let staging = profile_root.join(".snine-install");
        fs::create_dir_all(&staging)
            .await
            .map_err(|e| e.to_string())?;
        let canonical_staging = fs::canonicalize(&staging)
            .await
            .map_err(|e| e.to_string())?;
        if !canonical_staging.starts_with(&canonical_profile) {
            return Err("Install staging path escaped the managed profile".into());
        }
        let staged_jar = staging.join("snineclient.jar");
        if fs::try_exists(&staged_jar)
            .await
            .map_err(|e| e.to_string())?
        {
            fs::remove_file(&staged_jar)
                .await
                .map_err(|e| format!("Could not clear previous staged client: {e}"))?;
        }
        let request = DownloadRequest {
            url: release.download_url.clone(),
            file_name: "snineclient.jar".into(),
            expected_sha256: Some(release.sha256.clone()),
            profile_id: Some(profile_id.clone()),
            max_bytes: Some(expected_size),
        };
        emit_stage(app, &profile_id, "downloading", Some("snineclient.jar"));
        let receipt = self
            .downloads
            .download_to(app, &request, &staged_jar)
            .await?;
        let (receipt_id, receipt_bytes, receipt_sha256) =
            (receipt.id, receipt.bytes, receipt.sha256);
        if receipt_bytes != expected_size {
            return Err(format!("SNineClient artifact size did not match metadata (expected {expected_size}, received {receipt_bytes})"));
        }

        emit_stage(app, &profile_id, "verifying", Some("snineclient.jar"));
        emit_task_progress(
            app,
            &receipt_id,
            &profile_id,
            "verifying",
            receipt_bytes,
            expected_size,
            None,
        );
        let jar = staged_jar.clone();
        let requirements = match tokio::task::spawn_blocking(move || verify_client_jar(&jar))
            .await
            .map_err(|e| e.to_string())?
        {
            Ok(requirements) => requirements,
            Err(error) => {
                emit_task_progress(
                    app,
                    &receipt_id,
                    &profile_id,
                    "failed",
                    receipt_bytes,
                    expected_size,
                    Some(&error),
                );
                return Err(error);
            }
        };
        if requirements.minecraft_version != target_profile.minecraft_version {
            return Err(format!(
                "SNineClient build targets Minecraft {}, but this profile uses {}.",
                requirements.minecraft_version, target_profile.minecraft_version
            ));
        }

        emit_stage(app, &profile_id, "installing", Some("snineclient.jar"));
        emit_task_progress(
            app,
            &receipt_id,
            &profile_id,
            "installing",
            receipt_bytes,
            expected_size,
            None,
        );
        let mods = profile_root.join("instance").join("mods");
        fs::create_dir_all(&mods).await.map_err(|e| e.to_string())?;
        let canonical_mods = fs::canonicalize(&mods).await.map_err(|e| e.to_string())?;
        if !canonical_mods.starts_with(&canonical_profile) {
            return Err("Managed mods directory escaped the profile root".into());
        }
        let active = mods.join("snineclient.jar");
        let backup = mods.join("snineclient.jar.previous");
        if fs::try_exists(&active).await.map_err(|e| e.to_string())? {
            if fs::try_exists(&backup).await.map_err(|e| e.to_string())? {
                fs::remove_file(&backup).await.map_err(|e| e.to_string())?;
            }
            let prior_metadata = profile_root.join("client-install.json");
            let prior_backup = profile_root.join("client-install.json.previous");
            if fs::try_exists(&prior_backup)
                .await
                .map_err(|e| e.to_string())?
            {
                fs::remove_file(&prior_backup)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            if fs::try_exists(&prior_metadata)
                .await
                .map_err(|e| e.to_string())?
            {
                fs::copy(&prior_metadata, &prior_backup)
                    .await
                    .map_err(|e| format!("Could not preserve prior client metadata: {e}"))?;
            }
            fs::rename(&active, &backup)
                .await
                .map_err(|e| format!("Could not preserve previous client install: {e}"))?;
        }
        if let Err(error) = fs::rename(&staged_jar, &active).await {
            if fs::try_exists(&backup).await.unwrap_or(false) {
                let _ = fs::rename(&backup, &active).await;
            }
            return Err(format!("Could not activate verified client: {error}"));
        }
        let metadata = serde_json::json!({"serverRelease": version, "clientVersion": requirements.client_version, "minecraftVersion": requirements.minecraft_version, "requirements": requirements, "sha256":receipt_sha256, "size":receipt_bytes, "sha256Source":"github-release-manifest", "installedAt": chrono_free_now()});
        let metadata_temp = staging.join("client-install.json.tmp");
        fs::write(
            &metadata_temp,
            serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
        atomic_replace_file(&metadata_temp, &profile_root.join("client-install.json")).await?;
        let final_version = requirements.client_version.clone();
        let final_minecraft_version = requirements.minecraft_version.clone();
        let final_profile_id = profile_id.clone();
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            db.mark_client_installed(&final_profile_id, &final_version, &final_minecraft_version)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        emit_task_progress(
            app,
            &receipt_id,
            &profile_id,
            "completed",
            receipt_bytes,
            expected_size,
            None,
        );
        emit_stage(app, &profile_id, "client-installed-runtime-required", Some("Verified SNine client installed; Minecraft runtime and authentication are not installed"));
        Ok(InstallResult {
            profile_id,
            client_version: requirements.client_version,
            minecraft_version: requirements.minecraft_version,
            bytes: receipt_bytes,
            sha256: receipt_sha256,
        })
    }

    pub async fn client_needs_repair(&self, profile_id: &str) -> Result<bool, String> {
        validate_profile_id(profile_id)?;
        if !self
            .db
            .profile_exists(profile_id)
            .map_err(|e| e.to_string())?
        {
            return Err("Unknown profile".into());
        }
        let root = self.root.join("profiles").join(profile_id);
        let record_path = root.join("client-install.json");
        let jar_path = root.join("instance/mods/snineclient.jar");
        let Ok(record_bytes) = fs::read(record_path).await else {
            return Ok(true);
        };
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(&record_bytes) else {
            return Ok(true);
        };
        let expected_size = record.get("size").and_then(serde_json::Value::as_u64);
        let expected_hash = record
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let Ok(metadata) = fs::metadata(&jar_path).await else {
            return Ok(true);
        };
        if !metadata.is_file() || expected_size != Some(metadata.len()) {
            return Ok(true);
        }
        let Some(expected_hash) = expected_hash else {
            return Ok(true);
        };
        let hash_path = jar_path.clone();
        let hash_matches =
            tokio::task::spawn_blocking(move || file_matches_sha256(&hash_path, &expected_hash))
                .await
                .map_err(|e| e.to_string())??;
        if !hash_matches {
            return Ok(true);
        }
        let version = record
            .get("minecraftVersion")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        let requirements = record.get("requirements").cloned().unwrap_or_default();
        let jar = jar_path.clone();
        tokio::task::spawn_blocking(move || {
            let actual = verify_client_jar(&jar)?;
            let expected_client = requirements
                .get("clientVersion")
                .and_then(serde_json::Value::as_str);
            let expected_mc = requirements
                .get("minecraftVersion")
                .and_then(serde_json::Value::as_str)
                .or(Some(version.as_str()));
            let valid = expected_client.is_some_and(|expected| expected == actual.client_version)
                && expected_mc.is_some_and(|expected| expected == actual.minecraft_version);
            Ok::<bool, String>(!valid)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn check_client_update(&self, profile_id: &str) -> Result<ClientUpdateCheck, String> {
        validate_profile_id(profile_id)?;
        if !self
            .db
            .profile_exists(profile_id)
            .map_err(|e| e.to_string())?
        {
            return Err("Unknown profile".into());
        }
        let profile = self
            .db
            .bootstrap()
            .map_err(|error| error.to_string())?
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or("Unknown profile")?;
        let latest = self.resolve_release(&profile.minecraft_version).await?.version;
        let path = self
            .root
            .join("profiles")
            .join(profile_id)
            .join("client-install.json");
        let current = fs::read(path)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                let server_release = value
                    .get("serverRelease")
                    .and_then(serde_json::Value::as_str)
                    .filter(|version| parse_semver_triplet(version).is_some())
                    .map(str::to_owned);
                server_release.or_else(|| {
                    value
                        .get("clientVersion")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
            });
        let comparable = current
            .as_deref()
            .and_then(parse_semver_triplet)
            .zip(parse_semver_triplet(&latest));
        Ok(ClientUpdateCheck {
            available: comparable.is_some_and(|(current, latest)| latest > current),
            comparable: comparable.is_some(),
            current_version: current,
            latest_version: latest,
        })
    }

    pub async fn restore_previous_client(&self, profile_id: &str) -> Result<bool, String> {
        validate_profile_id(profile_id)?;
        let profile = self.root.join("profiles").join(profile_id);
        let mods = profile.join("instance/mods");
        let old_jar = mods.join("snineclient.jar.previous");
        let old_record = profile.join("client-install.json.previous");
        if !old_jar.is_file() || !old_record.is_file() {
            return Ok(false);
        }
        let active = mods.join("snineclient.jar");
        if fs::try_exists(&active).await.unwrap_or(false) {
            fs::remove_file(&active).await.map_err(|e| e.to_string())?;
        }
        fs::rename(old_jar, active)
            .await
            .map_err(|e| e.to_string())?;
        let record = profile.join("client-install.json");
        if fs::try_exists(&record).await.unwrap_or(false) {
            fs::remove_file(&record).await.map_err(|e| e.to_string())?;
        }
        fs::rename(old_record, record)
            .await
            .map_err(|e| e.to_string())?;
        let bytes = fs::read(profile.join("client-install.json"))
            .await
            .map_err(|e| e.to_string())?;
        let metadata: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if let (Some(version), Some(mc)) = (
            metadata
                .get("clientVersion")
                .and_then(serde_json::Value::as_str),
            metadata
                .get("minecraftVersion")
                .and_then(serde_json::Value::as_str),
        ) {
            let db = Arc::clone(&self.db);
            let id = profile_id.to_owned();
            let version = version.to_owned();
            let mc = mc.to_owned();
            tokio::task::spawn_blocking(move || db.mark_client_installed(&id, &version, &mc))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
        }
        Ok(true)
    }

    pub async fn discard_previous_client(&self, profile_id: &str) -> Result<(), String> {
        validate_profile_id(profile_id)?;
        let profile = self.root.join("profiles").join(profile_id);
        for path in [
            profile.join("instance/mods/snineclient.jar.previous"),
            profile.join("client-install.json.previous"),
        ] {
            if fs::try_exists(&path).await.map_err(|e| e.to_string())? {
                fs::remove_file(path).await.map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    async fn resolve_release(&self, minecraft_version: &str) -> Result<ClientRelease, String> {
        match self.resolve_home_manifest(minecraft_version).await {
            Ok(release) => Ok(release),
            Err(home_error) => self
                .resolve_github_release(minecraft_version)
                .await
                .map_err(|github_error| {
                    format!("Home update manifest failed ({home_error}); GitHub fallback failed ({github_error})")
                }),
        }
    }

    async fn resolve_home_manifest(&self, minecraft_version: &str) -> Result<ClientRelease, String> {
        let manifest = self
            .client
            .get(CLIENT_UPDATE_MANIFEST_URL)
            .send()
            .await
            .map_err(|error| format!("Could not contact SNine update manifest: {error}"))?
            .error_for_status()
            .map_err(|error| format!("SNine update manifest returned an error: {error}"))?
            .json::<HomeClientUpdateManifest>()
            .await
            .map_err(|error| format!("Could not read SNine update manifest: {error}"))?;
        let artifact = manifest
            .artifacts
            .get(minecraft_version)
            .ok_or_else(|| format!("No SNineClient artifact is published for Minecraft {minecraft_version}"))?;
        let expected_name = if minecraft_version == "1.21.11" {
            format!("snineclient-{}.jar", manifest.version)
        } else {
            format!("snineclient-{minecraft_version}-{}.jar", manifest.version)
        };
        if manifest.schema_version != 1
            || manifest.channel != "stable"
            || parse_semver_triplet(&manifest.version).is_none()
            || artifact.version != manifest.version
            || artifact.minecraft_version != minecraft_version
            || artifact.asset_name != expected_name
            || artifact.size == 0
            || artifact.size > MAX_CLIENT_BYTES
            || artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("SNine update manifest contains invalid artifact metadata".into());
        }
        let asset_url = Url::parse(&artifact.url)
            .map_err(|_| "SNine update artifact URL is invalid")?;
        if asset_url.scheme() != "https"
            || asset_url.host_str() != Some("snc.silasnet.win")
            || !asset_url.username().is_empty()
            || asset_url.password().is_some()
            || asset_url.path() != format!("/updates/{expected_name}")
            || asset_url.query().is_some()
            || asset_url.fragment().is_some()
        {
            return Err("SNine update artifact URL is outside the trusted update host".into());
        }
        Ok(ClientRelease {
            version: manifest.version,
            size: artifact.size,
            sha256: artifact.sha256.to_ascii_lowercase(),
            download_url: artifact.url.clone(),
        })
    }

    async fn resolve_github_release(&self, minecraft_version: &str) -> Result<ClientRelease, String> {
        let releases = self
            .client
            .get(CLIENT_RELEASES_API)
            .header(header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| format!("Could not contact GitHub client releases: {error}"))?
            .error_for_status()
            .map_err(|error| format!("GitHub client releases returned an error: {error}"))?
            .json::<Vec<GitHubRelease>>()
            .await
            .map_err(|error| format!("Could not read GitHub client release metadata: {error}"))?;

        for release in releases {
            if release.draft
                || !release.prerelease
                || !release.tag_name.starts_with(CLIENT_RELEASE_PREFIX)
            {
                continue;
            }
            let Some(body) = release.body else { continue };
            let Ok(manifest) = serde_json::from_str::<ClientReleaseManifest>(body.trim()) else {
                continue;
            };
            let expected_tag = format!("{CLIENT_RELEASE_PREFIX}{}", manifest.version);
            let artifact = manifest.artifacts.get(minecraft_version);
            let (size, sha256, asset_name) = if let Some(artifact) = artifact {
                (artifact.size, artifact.sha256.as_str(), artifact.asset_name.as_str())
            } else if minecraft_version == "1.21.11" {
                (manifest.size, manifest.sha256.as_str(), manifest.asset_name.as_str())
            } else {
                continue;
            };
            let expected_name = if minecraft_version == "1.21.11" {
                format!("snineclient-{}.jar", manifest.version)
            } else {
                format!("snineclient-{minecraft_version}-{}.jar", manifest.version)
            };
            if release.tag_name != expected_tag
                || parse_semver_triplet(&manifest.version).is_none()
                || size == 0
                || size > MAX_CLIENT_BYTES
                || sha256.len() != 64
                || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                || asset_name != expected_name
            {
                continue;
            }
            let Some(asset) = release
                .assets
                .iter()
                .find(|asset| asset.name == expected_name && asset.size == size)
            else {
                continue;
            };
            let asset_url = Url::parse(&asset.browser_download_url)
                .map_err(|_| "GitHub client asset URL is invalid")?;
            let expected_path = format!(
                "/silasnetwin/SNineLauncher/releases/download/{}/{expected_name}",
                release.tag_name
            );
            if asset_url.scheme() != "https"
                || asset_url.host_str() != Some("github.com")
                || !asset_url.username().is_empty()
                || asset_url.password().is_some()
                || asset_url.path() != expected_path
            {
                continue;
            }
            return Ok(ClientRelease {
                version: manifest.version,
                size,
                sha256: sha256.to_ascii_lowercase(),
                download_url: asset.browser_download_url.clone(),
            });
        }
        Err("No valid SNineClient client-v release with a verified JAR was found on GitHub".into())
    }
}

fn file_matches_sha256(path: &Path, expected: &str) -> Result<bool, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected))
}

async fn atomic_replace_file(source: &Path, target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(source_wide.as_ptr()),
                windows::core::PCWSTR(target_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| {
            format!(
                "Atomic client metadata replacement failed (HRESULT 0x{:08X})",
                error.code().0 as u32
            )
        })?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        tokio::fs::rename(source, target)
            .await
            .map_err(|e| e.to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientUpdateCheck {
    pub available: bool,
    pub comparable: bool,
    pub current_version: Option<String>,
    pub latest_version: String,
}

fn parse_semver_triplet(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim().trim_start_matches(['v', 'V']);
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()?
        .split(|ch: char| !ch.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn emit_stage(app: &AppHandle, profile_id: &str, state: &str, detail: Option<&str>) {
    let _ = app.emit(
        "install-progress",
        serde_json::json!({"profileId":profile_id,"state":state,"detail":detail}),
    );
}

fn emit_task_progress(
    app: &AppHandle,
    id: &str,
    profile_id: &str,
    state: &str,
    bytes: u64,
    total: u64,
    error: Option<&str>,
) {
    let _ = app.emit(
        "download-progress",
        crate::downloads::DownloadProgress {
            id: id.into(),
            file_name: "snineclient.jar".into(),
            state: state.into(),
            bytes_received: bytes,
            total_bytes: Some(total),
            bytes_per_second: 0,
            eta_seconds: None,
            error: error.map(str::to_owned),
            profile_id: Some(profile_id.into()),
        },
    );
}

fn verify_client_jar(path: &Path) -> Result<ClientRequirements, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Could not open downloaded client JAR: {e}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Downloaded client is not a valid JAR archive: {e}"))?;
    let mut descriptor = archive
        .by_name("fabric.mod.json")
        .map_err(|_| "Downloaded JAR does not contain fabric.mod.json".to_string())?;
    let mut text = String::new();
    use std::io::Read;
    descriptor
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid Fabric mod descriptor: {e}"))?;
    let mod_id = manifest
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if mod_id != "snineclient" {
        return Err("Downloaded JAR is not the SNine client (Fabric id mismatch)".into());
    }
    let depends = manifest
        .get("depends")
        .and_then(|v| v.as_object())
        .ok_or("SNine client has no Fabric dependencies metadata")?;
    let dependencies = depends
        .iter()
        .filter_map(|(name, value)| value.as_str().map(|value| (name.clone(), value.to_owned())))
        .collect::<BTreeMap<_, _>>();
    let minecraft = dependencies
        .get("minecraft")
        .map(String::as_str)
        .unwrap_or("");
    let Some(minecraft_version) = ["1.21.11", "26.1.2", "26.3"]
        .into_iter()
        .find(|version| minecraft.contains(version))
    else {
        return Err(format!(
            "SNine client Minecraft compatibility is not a supported launcher build: {minecraft}"
        ));
    };
    let required_java = if matches!(minecraft_version, "26.1.2" | "26.2" | "26.3") {
        "25"
    } else {
        "21"
    };
    if dependencies
        .get("java")
        .is_some_and(|requirement| !requirement.contains(required_java))
    {
        return Err(format!(
            "SNine client Java requirement is incompatible with Java {required_java}."
        ));
    }
    Ok(ClientRequirements {
        mod_id: mod_id.to_owned(),
        client_version: manifest
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_owned(),
        minecraft_version: minecraft_version.into(),
        java_requirement: dependencies.get("java").cloned(),
        fabric_loader_requirement: dependencies.get("fabricloader").cloned(),
        dependencies,
    })
}

fn validate_profile_id(id: &str) -> Result<(), String> {
    let path = Path::new(id);
    if id.is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || id.contains(['/', '\\', ':'])
    {
        return Err("Invalid managed profile ID".into());
    }
    Ok(())
}
fn chrono_free_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        file_matches_sha256, parse_semver_triplet, validate_profile_id, verify_client_jar,
        Installer,
    };
    use crate::{database::Database, downloads::DownloadService};
    use std::{path::PathBuf, sync::Arc};

    fn write_client_fixture(path: &std::path::Path, version: &str) -> Vec<u8> {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        let descriptor = format!(
            r#"{{"id":"snineclient","version":"{version}","depends":{{"minecraft":"1.21.11","fabricloader":">=0.19.0","java":">=21"}}}}"#
        );
        zip.write_all(descriptor.as_bytes()).unwrap();
        zip.finish().unwrap();
        std::fs::read(path).unwrap()
    }
    #[test]
    fn compares_only_explicit_semantic_release_versions() {
        assert!(parse_semver_triplet("1.2.4") > parse_semver_triplet("1.2.3"));
        assert_eq!(parse_semver_triplet("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver_triplet("\"etag-123\""), None);
        assert_eq!(
            parse_semver_triplet("latest (unversioned server response)"),
            None
        );
    }
    #[test]
    fn rejects_profile_path_traversal() {
        for value in ["../escape", "C:\\escape", "a/b", ""] {
            assert!(validate_profile_id(value).is_err());
        }
        assert!(validate_profile_id("snineclient-default").is_ok());
    }
    #[test]
    fn verifies_client_id_and_minecraft_contract() {
        let path = std::env::temp_dir().join(format!("snine-fixture-{}.jar", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        use std::io::Write;
        zip.write_all(br#"{"id":"snineclient","depends":{"minecraft":"1.21.11"}}"#)
            .unwrap();
        zip.finish().unwrap();
        let requirements = verify_client_jar(&path).unwrap();
        assert_eq!(requirements.mod_id, "snineclient");
        assert_eq!(requirements.minecraft_version, "1.21.11");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bundled_26_1_2_client_artifact_matches_its_fabric_and_java_contract() {
        let path =
            std::env::temp_dir().join(format!("snine-26-fixture-{}.jar", uuid::Uuid::new_v4()));
        std::fs::write(&path, super::SNINE_26_1_2_JAR).unwrap();
        let requirements = verify_client_jar(&path).unwrap();
        assert_eq!(requirements.mod_id, "snineclient");
        assert_eq!(requirements.client_version, "1.1.5");
        assert_eq!(requirements.minecraft_version, "26.1.2");
        assert_eq!(requirements.java_requirement.as_deref(), Some(">=25"));
        assert_eq!(
            requirements.fabric_loader_requirement.as_deref(),
            Some(">=0.19.3")
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn failed_update_fixture_restores_previous_jar_and_metadata() {
        let root =
            std::env::temp_dir().join(format!("snine-update-rollback-{}", uuid::Uuid::new_v4()));
        let db = Arc::new(Database::open(":memory:").unwrap());
        let profile = db.create_profile("Rollback fixture".into()).unwrap();
        let profile_root = root.join("profiles").join(&profile.id);
        let mods = profile_root.join("instance/mods");
        tokio::fs::create_dir_all(&mods).await.unwrap();
        tokio::fs::write(mods.join("snineclient.jar"), b"staged-v1.1.6")
            .await
            .unwrap();
        tokio::fs::write(mods.join("snineclient.jar.previous"), b"known-good-v1.1.5")
            .await
            .unwrap();
        tokio::fs::write(
            profile_root.join("client-install.json"),
            br#"{"clientVersion":"1.1.6","minecraftVersion":"1.21.11"}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            profile_root.join("client-install.json.previous"),
            br#"{"clientVersion":"1.1.5","minecraftVersion":"1.21.11"}"#,
        )
        .await
        .unwrap();

        let downloads = Arc::new(DownloadService::new(root.join("downloads")).unwrap());
        let installer = Installer::new(PathBuf::from(&root), downloads, Arc::clone(&db)).unwrap();
        assert!(installer
            .restore_previous_client(&profile.id)
            .await
            .unwrap());
        assert_eq!(
            tokio::fs::read(mods.join("snineclient.jar")).await.unwrap(),
            b"known-good-v1.1.5"
        );
        let metadata: serde_json::Value = serde_json::from_slice(
            &tokio::fs::read(profile_root.join("client-install.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["clientVersion"], "1.1.5");
        assert!(!mods.join("snineclient.jar.previous").exists());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn repair_scan_accepts_valid_client_and_detects_corrupt_or_missing_jar() {
        use sha2::{Digest, Sha256};
        let root =
            std::env::temp_dir().join(format!("snine-repair-client-{}", uuid::Uuid::new_v4()));
        let db = Arc::new(Database::open(":memory:").unwrap());
        let profile = db.create_profile("Repair fixture".into()).unwrap();
        let profile_root = root.join("profiles").join(&profile.id);
        let mods = profile_root.join("instance/mods");
        std::fs::create_dir_all(&mods).unwrap();
        let jar = mods.join("snineclient.jar");
        let bytes = write_client_fixture(&jar, "1.1.5");
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(verify_client_jar(&jar).unwrap().client_version, "1.1.5");
        assert!(file_matches_sha256(&jar, &digest).unwrap());
        let metadata = serde_json::json!({"size":bytes.len(),"sha256":digest,"minecraftVersion":"1.21.11","requirements":{"clientVersion":"1.1.5","minecraftVersion":"1.21.11"}});
        std::fs::write(
            profile_root.join("client-install.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        assert_eq!(
            metadata["size"].as_u64(),
            Some(std::fs::metadata(&jar).unwrap().len())
        );
        assert_eq!(
            metadata["requirements"]["clientVersion"].as_str(),
            Some("1.1.5")
        );
        assert_eq!(
            metadata["requirements"]["minecraftVersion"].as_str(),
            Some("1.21.11")
        );
        let downloads = Arc::new(DownloadService::new(root.join("downloads")).unwrap());
        let installer = Installer::new(root.clone(), downloads, db).unwrap();

        let needs_repair = installer.client_needs_repair(&profile.id).await.unwrap();
        assert!(!needs_repair, "valid repair fixture was rejected");
        std::fs::write(&jar, b"corrupt JAR fixture").unwrap();
        assert!(installer.client_needs_repair(&profile.id).await.unwrap());
        std::fs::remove_file(&jar).unwrap();
        assert!(installer.client_needs_repair(&profile.id).await.unwrap());
        let _ = std::fs::remove_dir_all(root);
    }
}
