//! Mojang/Fabric managed installation and structured launch support.
//!
//! Metadata is resolved from Mojang and Fabric at install time and cached in
//! app-data. Immutable libraries/assets are shared between profiles.

use crate::{
    auth::AuthService,
    database::{Database, LoaderType, ManagedMod, SafeAccount},
    downloads::{DownloadProgress, DownloadRequest, DownloadService},
    runtime::RuntimeService,
};
use futures_util::{stream, StreamExt};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Sha256, Sha512};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::{process::Command, sync::Mutex as AsyncMutex};

const VERSION_MANIFEST: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const QUILT_META: &str = "https://meta.quiltmc.org/v3";
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const MAX_METADATA_BYTES: usize = 32 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const ASSET_CONCURRENCY: usize = 8;
const MAX_RESOURCE_PACK_BYTES: usize = 128 * 1024 * 1024;
const MAX_ICON_CLIENT_JAR_BYTES: usize = 128 * 1024 * 1024;
const LATEST_ICON_RELEASE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePackInfo {
    pub filename: String,
    pub name: String,
    pub description: String,
    pub pack_format: Option<u32>,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileContentItem {
    pub name: String,
    pub directory: bool,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLogTail {
    pub file_name: String,
    pub content: String,
    pub bytes: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSymlink {
    pub name: String,
    pub directory: bool,
    pub target_exists: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftInstallResult {
    pub profile_id: String,
    pub minecraft_version: String,
    pub loader_version: String,
    pub main_class: String,
    pub library_count: usize,
    pub asset_count: usize,
    pub bytes_downloaded: u64,
    pub ready: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchState {
    pub profile_id: String,
    pub state: String,
    pub pid: Option<u32>,
    pub message: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchVerification {
    pub profile_id: String,
    pub fabric_loaded: bool,
    pub loader_type: String,
    pub snine_client_loaded: bool,
    pub snine_client_expected: bool,
    pub loader_version: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileState {
    pub profile_id: String,
    pub state: String,
    pub runtime_ready: bool,
    pub minecraft_version: Option<String>,
    pub loader_version: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileManageInfo {
    pub profile_id: String,
    pub installed: bool,
    pub minecraft_version: Option<String>,
    pub loader_version: Option<String>,
    pub client_version: Option<String>,
    pub runtime: Option<String>,
    pub directory: String,
    pub storage_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub profile_instances_bytes: u64,
    pub libraries_bytes: u64,
    pub assets_bytes: u64,
    pub runtimes_bytes: u64,
    pub download_cache_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdateStatus {
    pub project_id: String,
    pub slug: String,
    pub current_version_id: String,
    pub latest_version_id: String,
    pub update_available: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftVersionInfo {
    pub id: String,
    pub version_type: String,
    pub release_time: String,
    pub url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockIconEntry {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub source_version: String,
    pub preview_data_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockIconIndexEntry {
    id: String,
    kind: String,
}

pub struct MinecraftService {
    root: PathBuf,
    downloads: Arc<DownloadService>,
    runtime: Arc<RuntimeService>,
    database: Arc<Database>,
    client: Client,
    install_lock: AsyncMutex<()>,
    mod_install_lock: AsyncMutex<()>,
    launch_lock: AsyncMutex<()>,
    running: Mutex<BTreeMap<String, u32>>,
    latest_icon_release: AsyncMutex<Option<(Instant, MinecraftVersionInfo)>>,
}

impl MinecraftService {
    pub fn new(
        root: PathBuf,
        downloads: Arc<DownloadService>,
        runtime: Arc<RuntimeService>,
        database: Arc<Database>,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            root,
            downloads,
            runtime,
            database,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .timeout(Duration::from_secs(180))
                .user_agent(concat!("SNineLauncher/", env!("CARGO_PKG_VERSION")))
                .build()?,
            install_lock: AsyncMutex::new(()),
            mod_install_lock: AsyncMutex::new(()),
            launch_lock: AsyncMutex::new(()),
            running: Mutex::new(BTreeMap::new()),
            latest_icon_release: AsyncMutex::new(None),
        })
    }

    pub fn storage_info(&self) -> StorageInfo {
        let minecraft = self.root.join("minecraft");
        StorageInfo {
            profile_instances_bytes: directory_size(&self.root.join("profiles")),
            libraries_bytes: directory_size(&minecraft.join("libraries")),
            assets_bytes: directory_size(&minecraft.join("assets")),
            runtimes_bytes: directory_size(&self.root.join("runtimes")),
            download_cache_bytes: directory_size(&self.root.join("downloads")),
        }
    }

    pub async fn minecraft_versions(
        &self,
        include_snapshots: bool,
        refresh: bool,
    ) -> Result<Vec<MinecraftVersionInfo>, String> {
        let cache = self
            .root
            .join("minecraft/metadata/version_manifest_v2.json");
        let manifest = if refresh || !cache.is_file() {
            match self.fetch_bytes(VERSION_MANIFEST, MAX_METADATA_BYTES).await {
                Ok(bytes) => {
                    atomic_bytes(&cache, &bytes)?;
                    serde_json::from_slice::<Value>(&bytes)
                        .map_err(|_| "Mojang version manifest is invalid")?
                }
                Err(error) if cache.is_file() => read_json(&cache).map_err(|_| error)?,
                Err(error) => return Err(error),
            }
        } else {
            read_json(&cache)?
        };
        parse_minecraft_versions(&manifest, include_snapshots)
    }

    pub async fn fabric_loader_versions(
        &self,
        minecraft_version: &str,
    ) -> Result<Vec<String>, String> {
        if minecraft_version.is_empty()
            || !minecraft_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        {
            return Err("Invalid Minecraft version id".into());
        }
        let url = format!("{FABRIC_META}/versions/loader/{minecraft_version}");
        let bytes = self.fetch_bytes(&url, 2 * 1024 * 1024).await?;
        let data: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Fabric loader metadata is invalid")?;
        let versions = data
            .as_array()
            .ok_or("Fabric loader metadata has no versions")?;
        let mut resolved = versions
            .iter()
            .filter_map(|row| {
                let loader = row.get("loader")?;
                if !loader
                    .get("stable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    return None;
                }
                loader.get("version")?.as_str().map(str::to_owned)
            })
            .collect::<Vec<_>>();
        resolved.sort_by(|a, b| compare_versions(b, a));
        Ok(resolved)
    }

    pub async fn quilt_loader_versions(
        &self,
        minecraft_version: &str,
    ) -> Result<Vec<String>, String> {
        validate_version_id(minecraft_version)?;
        let url = format!("{QUILT_META}/versions/loader/{minecraft_version}");
        let bytes = self.fetch_bytes(&url, 2 * 1024 * 1024).await?;
        let data: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Quilt loader metadata is invalid")?;
        parse_quilt_loader_versions(&data)
    }

    pub async fn supported_loader_types(
        &self,
        minecraft_version: &str,
    ) -> Result<Vec<LoaderType>, String> {
        validate_version_id(minecraft_version)?;
        let known_minecraft = self
            .minecraft_versions(true, false)
            .await?
            .iter()
            .any(|version| version.id == minecraft_version);
        if !known_minecraft {
            return Ok(Vec::new());
        }
        let mut providers = vec![LoaderType::Vanilla];
        if self
            .fabric_loader_versions(minecraft_version)
            .await
            .is_ok_and(|versions| !versions.is_empty())
        {
            providers.push(LoaderType::Fabric);
        }
        if self
            .quilt_loader_versions(minecraft_version)
            .await
            .is_ok_and(|versions| !versions.is_empty())
        {
            providers.push(LoaderType::Quilt);
        }
        Ok(providers)
    }

    pub fn block_icon_catalog(
        &self,
        minecraft_version: &str,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<BlockIconEntry>, String> {
        validate_version_id(minecraft_version)?;
        let client_jar = self
            .root
            .join("minecraft/versions")
            .join(minecraft_version)
            .join(format!("{minecraft_version}.jar"));
        block_icon_catalog_from_jar(
            &self.root,
            minecraft_version,
            &client_jar,
            query,
            offset,
            limit,
        )
    }

    pub async fn latest_block_icon_catalog(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<BlockIconEntry>, String> {
        let release = self.resolve_latest_icon_release().await?;
        let client_jar = self.ensure_icon_client_jar(&release).await?;
        let root = self.root.clone();
        let version = release.id;
        let query = query.to_owned();
        tokio::task::spawn_blocking(move || {
            block_icon_catalog_from_jar(&root, &version, &client_jar, &query, offset, limit)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    async fn resolve_latest_icon_release(&self) -> Result<MinecraftVersionInfo, String> {
        let mut cached = self.latest_icon_release.lock().await;
        if let Some((checked_at, release)) = cached.as_ref() {
            if checked_at.elapsed() < LATEST_ICON_RELEASE_TTL {
                return Ok(release.clone());
            }
        }
        let release = self
            .minecraft_versions(false, true)
            .await?
            .into_iter()
            .find(|version| version.version_type == "release")
            .ok_or("Mojang did not publish a Minecraft release")?;
        *cached = Some((Instant::now(), release.clone()));
        Ok(release)
    }

    async fn ensure_icon_client_jar(
        &self,
        release: &MinecraftVersionInfo,
    ) -> Result<PathBuf, String> {
        let version_bytes = self.fetch_bytes(&release.url, MAX_METADATA_BYTES).await?;
        let version_json: Value = serde_json::from_slice(&version_bytes)
            .map_err(|_| "Latest Minecraft version metadata is invalid")?;
        validate_version_json(&version_json, &release.id)?;
        let client = version_json
            .pointer("/downloads/client")
            .ok_or("Latest Minecraft release has no client archive")?;
        let url = required_string(client, "url")?;
        let expected_sha1 = required_string(client, "sha1")?;
        if expected_sha1.len() != 40 || !expected_sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Latest Minecraft client archive has invalid SHA-1 metadata".into());
        }
        let expected_size = client
            .get("size")
            .and_then(Value::as_u64)
            .filter(|size| *size > 0 && *size <= MAX_ICON_CLIENT_JAR_BYTES as u64)
            .ok_or("Latest Minecraft client archive has an unsafe or invalid size")?;
        let cache_root = self.root.join("cache/block-icons/source");
        let cached = cache_root.join(format!("{}-{expected_sha1}.jar", release.id));
        if let Ok(bytes) = fs::read(&cached) {
            if bytes.len() as u64 == expected_size
                && digest_sha1(&bytes).eq_ignore_ascii_case(expected_sha1)
            {
                return Ok(cached);
            }
        }
        let bytes = self.fetch_bytes(url, MAX_ICON_CLIENT_JAR_BYTES).await?;
        if bytes.len() as u64 != expected_size {
            return Err("Latest Minecraft client archive has an unexpected size".into());
        }
        if !digest_sha1(&bytes).eq_ignore_ascii_case(expected_sha1) {
            return Err("Latest Minecraft client archive failed SHA-1 verification".into());
        }
        atomic_bytes(&cached, &bytes)?;
        Ok(cached)
    }

    pub async fn install_profile<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        profile_id: &str,
    ) -> Result<MinecraftInstallResult, String> {
        let _guard = self.install_lock.lock().await;
        validate_profile_id(profile_id)?;
        if !self
            .database
            .profile_exists(profile_id)
            .map_err(|e| e.to_string())?
        {
            return Err("Unknown profile".into());
        }
        let profile_root = checked_profile_root(&self.root, profile_id)?;
        let saved_profile = self
            .database
            .bootstrap()
            .map_err(|e| e.to_string())?
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or("Unknown profile")?;
        if !matches!(
            saved_profile.loader_type,
            LoaderType::Vanilla | LoaderType::Fabric | LoaderType::Quilt
        ) {
            return Err("This loader provider is not available yet.".into());
        }
        if saved_profile.snineclient_enabled
            && (saved_profile.loader_type != LoaderType::Fabric
                || !matches!(
                    saved_profile.minecraft_version.as_str(),
                    "1.21.11" | "26.1.2" | "26.3"
                ))
        {
            return Err(
                "SNineClient requires Fabric on Minecraft 1.21.11, 26.1.2, or 26.3.".into(),
            );
        }
        let install_record =
            read_json(&profile_root.join("client-install.json")).unwrap_or(Value::Null);
        let requirements = install_record.get("requirements").unwrap_or(&Value::Null);
        let resolved_client_version = requirements
            .get("minecraftVersion")
            .and_then(Value::as_str)
            .or_else(|| {
                install_record
                    .get("minecraftVersion")
                    .and_then(Value::as_str)
            })
            .filter(|version| *version != "Latest supported");
        let minecraft_version = if saved_profile.snineclient_enabled {
            resolved_client_version.unwrap_or(&saved_profile.minecraft_version)
        } else {
            &saved_profile.minecraft_version
        };
        if minecraft_version == "Latest supported" {
            return Err("Choose a Minecraft version before installing this profile.".into());
        }
        let mod_path = profile_root.join("instance/mods/snineclient.jar");
        let mod_metadata = if saved_profile.snineclient_enabled {
            let expected_client_size = install_record
                .get("size")
                .and_then(Value::as_u64)
                .ok_or("SNineClient install metadata has no recorded size")?;
            let expected_client_sha256 = install_record
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or("SNineClient install metadata has no recorded digest")?;
            if !file_matches_sha256(&mod_path, expected_client_size, expected_client_sha256)? {
                return Err("The installed SNineClient JAR changed since it was verified. Reinstall the client before continuing.".into());
            }
            Some(inspect_snine_mod(&mod_path, minecraft_version)?)
        } else {
            None
        };
        let minimum_loader = requirements
            .get("fabricLoaderRequirement")
            .and_then(Value::as_str)
            .or_else(|| {
                mod_metadata
                    .as_ref()
                    .and_then(|meta| meta.dependencies.get("fabricloader").map(String::as_str))
            })
            .unwrap_or(">=0.0.0");
        let required_java = if matches!(
            saved_profile.minecraft_version.as_str(),
            "26.1.2" | "26.2" | "26.3"
        ) {
            25
        } else {
            21
        };
        let runtime_status = self.runtime.detect(required_java);
        if runtime_status.compatible.is_none() {
            return Err(format!(
                "RUNTIME_REQUIRED: Install a compatible 64-bit Java {required_java} runtime first."
            ));
        }
        let shared = self.root.join("minecraft");
        let cache = shared.join("metadata");
        let versions_root = shared.join("versions");
        let libraries_root = shared.join("libraries");
        let assets_root = shared.join("assets");
        let natives_root = shared
            .join("natives")
            .join(profile_id)
            .join(minecraft_version);
        for directory in [
            &cache,
            &versions_root,
            &libraries_root,
            &assets_root,
            &natives_root,
        ] {
            create_managed_directory(&self.root, directory)
                .map_err(|e| format!("Could not prepare managed Minecraft storage: {e}"))?;
        }
        emit_stage(
            app,
            profile_id,
            "resolving",
            if matches!(
                saved_profile.loader_type,
                LoaderType::Fabric | LoaderType::Quilt
            ) {
                "Resolving official Minecraft and Fabric metadata"
            } else {
                "Resolving official Minecraft metadata"
            },
        );
        let manifest: Value = self
            .fetch_cached_json(&cache.join("version_manifest_v2.json"), VERSION_MANIFEST)
            .await?;
        let version_entry = manifest.get("versions").and_then(Value::as_array)
            .and_then(|entries| entries.iter().find(|entry| entry.get("id").and_then(Value::as_str) == Some(minecraft_version)))
            .ok_or_else(|| format!("Minecraft {minecraft_version} is not present in Mojang's official version manifest"))?;
        let version_url = required_string(version_entry, "url")?;
        let version_sha1 = required_string(version_entry, "sha1")?;
        let version_cache = cache.join(format!("{minecraft_version}.json"));
        let version_json = self
            .fetch_verified_json(&version_cache, version_url, Some(version_sha1))
            .await?;
        validate_version_json(&version_json, minecraft_version)?;
        let required_java = version_json
            .pointer("/javaVersion/majorVersion")
            .and_then(Value::as_u64)
            .unwrap_or(21) as u32;
        if saved_profile.snineclient_enabled && required_java != 21 && required_java != 25 {
            return Err(format!("SNineClient has no verified Java compatibility for this profile (Minecraft requires Java {required_java})."));
        }
        if required_java > 21 && self.runtime.detect(required_java).compatible.is_none() {
            return Err(format!("RUNTIME_REQUIRED: Minecraft {minecraft_version} requires Java {required_java}; install a compatible runtime first."));
        }

        // Resolve the asset index before any multi-hundred-megabyte payloads,
        // allowing a real preflight estimate rather than discovering a full
        // disk near the end of installation.
        let asset_index = version_json
            .get("assetIndex")
            .ok_or("Mojang asset index metadata is missing")?;
        let asset_index_id = required_string(asset_index, "id")?;
        let index_path = assets_root
            .join("indexes")
            .join(format!("{asset_index_id}.json"));
        let mut bytes_downloaded = self
            .ensure_mojang_file(app, profile_id, asset_index, &index_path, "asset index")
            .await?;
        let index_json = read_json(&index_path)?;
        let objects = index_json
            .get("objects")
            .and_then(Value::as_object)
            .ok_or("Minecraft asset index is invalid")?;
        let mut estimated_missing = 512_u64 * 1024 * 1024; // logs, launch files, extraction and safety margin
        let client_download = version_json
            .pointer("/downloads/client")
            .ok_or("Mojang version metadata has no client download")?;
        estimated_missing = estimated_missing.saturating_add(missing_artifact_bytes(
            client_download,
            &versions_root
                .join(minecraft_version)
                .join(format!("{minecraft_version}.jar")),
        )?);
        let libraries = version_json
            .get("libraries")
            .and_then(Value::as_array)
            .ok_or("Mojang library metadata is missing")?;
        for library in libraries {
            if !library_allowed(library, "windows", "x86_64", &BTreeMap::new()) {
                continue;
            }
            let downloads = library.get("downloads").unwrap_or(&Value::Null);
            if let Some(artifact) = downloads.get("artifact") {
                let target = libraries_root.join(artifact_path(artifact)?);
                estimated_missing =
                    estimated_missing.saturating_add(missing_artifact_bytes(artifact, &target)?);
            }
            if let Some(classifiers) = downloads.get("classifiers") {
                if let Some(classifier) = native_classifier(library, "windows", "64") {
                    if let Some(artifact) = classifiers.get(&classifier) {
                        let target = libraries_root.join(artifact_path(artifact)?);
                        estimated_missing = estimated_missing
                            .saturating_add(missing_artifact_bytes(artifact, &target)?);
                    }
                }
            }
        }
        if let Some(logging) = version_json.pointer("/logging/client") {
            let file_metadata = logging
                .get("file")
                .ok_or("Minecraft client logging config is incomplete")?;
            let file_name = required_string(file_metadata, "id")?;
            let logging_path = assets_root.join("log_configs").join(file_name);
            estimated_missing = estimated_missing
                .saturating_add(missing_artifact_bytes(file_metadata, &logging_path)?);
        }
        for object in objects.values() {
            let hash = required_string(object, "hash")?;
            let size = object
                .get("size")
                .and_then(Value::as_u64)
                .ok_or("Minecraft asset size is invalid")?;
            if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Minecraft asset hash is invalid".into());
            }
            let target = assets_root.join("objects").join(&hash[..2]).join(hash);
            if !file_matches_sha1(&target, Some(size), Some(hash))? {
                estimated_missing = estimated_missing.saturating_add(size);
            }
        }
        let available = fs2::available_space(&shared)
            .map_err(|_| "Could not check free disk space for Minecraft installation")?;
        if available < estimated_missing {
            return Err(format!("DiskFull: Minecraft setup requires about {estimated_missing} bytes (including a safety margin); {available} bytes are available."));
        }

        emit_stage(
            app,
            profile_id,
            "downloading-minecraft",
            "Installing Minecraft client and libraries",
        );
        let version_dir = versions_root.join(minecraft_version);
        fs::create_dir_all(&version_dir).map_err(|e| e.to_string())?;
        let client_download = version_json
            .pointer("/downloads/client")
            .ok_or("Mojang version metadata has no client download")?;
        let client_path = version_dir.join(format!("{minecraft_version}.jar"));
        bytes_downloaded += self
            .ensure_mojang_file(
                app,
                profile_id,
                client_download,
                &client_path,
                "Minecraft client",
            )
            .await?;

        let mut classpath = Vec::<PathBuf>::new();
        let mut native_archives = Vec::<PathBuf>::new();
        let libraries = version_json
            .get("libraries")
            .and_then(Value::as_array)
            .ok_or("Mojang library metadata is missing")?;
        for library in libraries {
            if !library_allowed(library, "windows", "x86_64", &BTreeMap::new()) {
                continue;
            }
            let downloads = library.get("downloads").unwrap_or(&Value::Null);
            if let Some(artifact) = downloads.get("artifact") {
                let path = artifact_path(artifact)?;
                let target = libraries_root.join(&path);
                bytes_downloaded += self
                    .ensure_mojang_file(app, profile_id, artifact, &target, &path.to_string_lossy())
                    .await?;
                classpath.push(target);
            }
            if let Some(classifiers) = downloads.get("classifiers") {
                if let Some(classifier) = native_classifier(library, "windows", "64") {
                    if let Some(artifact) = classifiers.get(&classifier) {
                        let path = libraries_root.join(artifact_path(artifact)?);
                        bytes_downloaded += self
                            .ensure_mojang_file(app, profile_id, artifact, &path, &classifier)
                            .await?;
                        native_archives.push(path);
                    }
                }
            }
        }
        let logging_argument = if let Some(logging) = version_json.pointer("/logging/client") {
            let argument = required_string(logging, "argument")?;
            let file_metadata = logging
                .get("file")
                .ok_or("Minecraft client logging config is incomplete")?;
            let file_name = required_string(file_metadata, "id")?;
            let logging_path = assets_root.join("log_configs").join(file_name);
            bytes_downloaded = bytes_downloaded.saturating_add(
                self.ensure_mojang_file(
                    app,
                    profile_id,
                    file_metadata,
                    &logging_path,
                    "Minecraft logging configuration",
                )
                .await?,
            );
            Some(argument.replace("${path}", &logging_path.to_string_lossy()))
        } else {
            None
        };

        emit_stage(
            app,
            profile_id,
            "downloading-assets",
            "Installing shared Minecraft assets",
        );
        let mut object_jobs = Vec::new();
        for object in objects.values() {
            let hash = required_string(object, "hash")?;
            let size = object
                .get("size")
                .and_then(Value::as_u64)
                .ok_or("Minecraft asset size is invalid")?;
            if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Minecraft asset hash is invalid".into());
            }
            let target = assets_root.join("objects").join(&hash[..2]).join(hash);
            if !file_matches_sha1(&target, Some(size), Some(hash))? {
                object_jobs.push((hash.to_owned(), size, target));
            }
        }
        let asset_task_id = format!("minecraft-assets-{}", uuid::Uuid::new_v4());
        let asset_total = object_jobs.iter().map(|(_, size, _)| *size).sum::<u64>();
        emit_download_task(
            app,
            &asset_task_id,
            profile_id,
            "Minecraft assets",
            "downloading",
            0,
            asset_total,
            0,
            None,
        );
        let download_service = Arc::clone(&self.downloads);
        let app_ref = app.clone();
        let profile = profile_id.to_owned();
        let results = stream::iter(object_jobs.into_iter().map(|(hash, size, target)| {
            let service = Arc::clone(&download_service);
            let app = app_ref.clone();
            let profile = profile.clone();
            async move {
                ensure_hash_file(
                    &service,
                    &app,
                    &profile,
                    &format!(
                        "https://resources.download.minecraft.net/{}/{hash}",
                        &hash[..2]
                    ),
                    &target,
                    size,
                    &hash,
                )
                .await
            }
        }))
        .buffer_unordered(ASSET_CONCURRENCY);
        tokio::pin!(results);
        let started = std::time::Instant::now();
        let mut last_emit = std::time::Instant::now();
        let mut asset_bytes = 0_u64;
        while let Some(result) = results.next().await {
            match result {
                Ok(bytes) => asset_bytes = asset_bytes.saturating_add(bytes),
                Err(error) => {
                    emit_download_task(
                        app,
                        &asset_task_id,
                        profile_id,
                        "Minecraft assets",
                        "failed",
                        asset_bytes,
                        asset_total,
                        0,
                        Some(&error),
                    );
                    return Err(error);
                }
            }
            if last_emit.elapsed() >= Duration::from_millis(250) {
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let speed = (asset_bytes as f64 / elapsed) as u64;
                emit_download_task(
                    app,
                    &asset_task_id,
                    profile_id,
                    "Minecraft assets",
                    "downloading",
                    asset_bytes,
                    asset_total,
                    speed,
                    None,
                );
                last_emit = std::time::Instant::now();
            }
        }
        bytes_downloaded = bytes_downloaded.saturating_add(asset_bytes);
        emit_download_task(
            app,
            &asset_task_id,
            profile_id,
            "Minecraft assets",
            "completed",
            asset_bytes,
            asset_total,
            0,
            None,
        );

        let (loader_version, fabric_profile, main_class) = if matches!(
            saved_profile.loader_type,
            LoaderType::Fabric | LoaderType::Quilt
        ) {
            emit_stage(
                app,
                profile_id,
                if saved_profile.loader_type == LoaderType::Quilt {
                    "installing-quilt"
                } else {
                    "installing-fabric"
                },
                if saved_profile.loader_type == LoaderType::Quilt {
                    "Resolving Quilt Loader metadata"
                } else {
                    "Resolving Fabric Loader metadata"
                },
            );
            let requested = saved_profile
                .loader_version
                .as_deref()
                .unwrap_or(minimum_loader);
            let is_quilt = saved_profile.loader_type == LoaderType::Quilt;
            let loader_version = if is_quilt {
                self.resolve_quilt_loader(minecraft_version, requested)
                    .await?
            } else {
                match self
                    .resolve_fabric_loader(minecraft_version, requested)
                    .await
                {
                    Ok(version) => version,
                    // A profile can retain an older, exact Fabric pin from a
                    // previous launcher build. When SNineClient is enabled,
                    // its mod metadata is the compatibility contract; retry
                    // against that minimum so a compatible current stable
                    // Loader remains installable.
                    Err(error)
                        if saved_profile.snineclient_enabled
                            && requested != minimum_loader
                            && error.starts_with("No stable Fabric Loader version") =>
                    {
                        self.resolve_fabric_loader(minecraft_version, minimum_loader)
                            .await?
                    }
                    Err(error) => return Err(error),
                }
            };
            let fabric_profile_url = if is_quilt {
                format!("{QUILT_META}/versions/loader/{minecraft_version}/{loader_version}/profile/json")
            } else {
                format!("{FABRIC_META}/versions/loader/{minecraft_version}/{loader_version}/profile/json")
            };
            let fabric_cache = cache.join(format!(
                "{}-{minecraft_version}-{loader_version}.json",
                if is_quilt { "quilt" } else { "fabric" }
            ));
            let fabric_profile = self
                .fetch_cached_json(&fabric_cache, &fabric_profile_url)
                .await?;
            let main = required_string(&fabric_profile, "mainClass")?;
            let valid_main = if is_quilt {
                main.starts_with("org.quiltmc.loader.")
            } else {
                main.starts_with("net.fabricmc.loader.")
            };
            if !valid_main {
                return Err("Mod loader metadata returned an unexpected main class".into());
            }
            let main = main.to_owned();
            let fabric_libraries = fabric_profile
                .get("libraries")
                .and_then(Value::as_array)
                .ok_or("Fabric profile has no libraries")?;
            for library in fabric_libraries {
                let (relative, url, sha1, size) = fabric_library_artifact(library)?;
                let target = libraries_root.join(relative);
                bytes_downloaded += self
                    .ensure_external_file(
                        app,
                        profile_id,
                        &url,
                        &target,
                        size,
                        sha1.as_deref(),
                        &target.file_name().unwrap_or_default().to_string_lossy(),
                    )
                    .await?;
                classpath.push(target);
            }
            (loader_version, fabric_profile, main)
        } else {
            let main = required_string(&version_json, "mainClass")?.to_owned();
            ("vanilla".to_owned(), Value::Null, main)
        };
        classpath.push(client_path.clone());
        let mods_dir = profile_root.join("instance/mods");
        create_managed_directory(&profile_root, &mods_dir)?;
        let mut pending = mod_metadata
            .as_ref()
            .map(|meta| mod_slugs(&meta.dependencies))
            .unwrap_or_default()
            .into_iter()
            .map(|slug| {
                (
                    slug.to_owned(),
                    mod_metadata
                        .as_ref()
                        .and_then(|meta| meta.dependencies.get(slug).cloned()),
                )
            })
            .collect::<VecDeque<_>>();
        let mut resolved_projects = HashSet::new();
        while let Some((slug, minimum)) = pending.pop_front() {
            if !resolved_projects.insert(slug.clone()) {
                continue;
            }
            let mod_file = self
                .resolve_modrinth_mod(&slug, minecraft_version, minimum.as_deref())
                .await?;
            for project_id in &mod_file.required_projects {
                if !resolved_projects.contains(project_id) {
                    pending.push_back((project_id.clone(), None));
                }
            }
            let target = mods_dir.join(&mod_file.filename);
            safe_relative(&mod_file.filename)?;
            if !file_matches_modrinth(&target, &mod_file)? {
                let receipt = self
                    .downloads
                    .download_to(
                        app,
                        &DownloadRequest {
                            url: mod_file.url.clone(),
                            file_name: mod_file.filename.clone(),
                            expected_sha256: None,
                            profile_id: Some(profile_id.into()),
                            max_bytes: Some(mod_file.size),
                        },
                        &target,
                    )
                    .await?;
                if receipt.bytes != mod_file.size || !file_matches_modrinth(&target, &mod_file)? {
                    let _ = fs::remove_file(&target);
                    return Err(format!(
                        "SNineClient dependency {} failed integrity validation",
                        mod_file.filename
                    ));
                }
                bytes_downloaded += receipt.bytes;
            }
        }
        if saved_profile.snineclient_enabled && !mod_path.is_file() {
            return Err("SNineClient mod is missing from the profile".into());
        }

        emit_stage(
            app,
            profile_id,
            "verifying",
            "Preparing native libraries and checking installed artifacts",
        );
        fs::create_dir_all(&natives_root).map_err(|e| e.to_string())?;
        for archive in native_archives {
            extract_natives(&archive, &natives_root)?;
        }
        let classpath_string = std::env::join_paths(classpath.iter())
            .map_err(|_| "Minecraft classpath contains an invalid path")?
            .to_string_lossy()
            .to_string();
        let launch_data = ResolvedInstall {
            schema: 1,
            profile_id: profile_id.into(),
            minecraft_version: minecraft_version.into(),
            loader_version: loader_version.clone(),
            snineclient_enabled: saved_profile.snineclient_enabled,
            main_class: main_class.clone(),
            client_jar: client_path.display().to_string(),
            classpath: classpath
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            classpath_string,
            native_directory: natives_root.display().to_string(),
            assets_directory: assets_root.display().to_string(),
            asset_index: asset_index_id.into(),
            version_json: version_json.clone(),
            fabric_profile: fabric_profile.clone(),
            logging_argument,
            library_count: classpath.len().saturating_sub(1),
            asset_object_count: objects.len(),
        };
        let metadata_dir = profile_root.join("instance");
        create_managed_directory(&profile_root, &metadata_dir)?;
        atomic_json(&metadata_dir.join("snine-launch.json"), &launch_data)?;
        let client_info = if saved_profile.snineclient_enabled {
            read_json(&profile_root.join("client-install.json"))?
        } else {
            Value::Null
        };
        let client_download = version_json
            .pointer("/downloads/client")
            .ok_or("Minecraft client metadata is incomplete")?;
        let now = unix_seconds();
        let old_manifest = read_json(&metadata_dir.join("installed-manifest.json")).ok();
        let installed_at = old_manifest
            .as_ref()
            .and_then(|value| value.get("installedAtUnix"))
            .and_then(Value::as_u64)
            .unwrap_or(now);
        let runtime_path = self
            .runtime
            .detect(required_java)
            .compatible
            .map(|candidate| candidate.executable);
        let mut managed_artifacts = vec![
            ManagedArtifact {
                logical_id: "minecraft-client".into(),
                relative_path: format!(
                    "minecraft/versions/{minecraft_version}/{minecraft_version}.jar"
                ),
                expected_hash: client_download
                    .get("sha1")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                expected_size: client_download.get("size").and_then(Value::as_u64),
                source: "Mojang version metadata".into(),
                integrity: "Mojang SHA-1".into(),
            },
            ManagedArtifact {
                logical_id: format!("asset-index:{asset_index_id}"),
                relative_path: format!("minecraft/assets/indexes/{asset_index_id}.json"),
                expected_hash: asset_index
                    .get("sha1")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                expected_size: asset_index.get("size").and_then(Value::as_u64),
                source: "Mojang version metadata".into(),
                integrity: "Mojang SHA-1".into(),
            },
        ];
        if saved_profile.snineclient_enabled {
            managed_artifacts.push(ManagedArtifact {
                logical_id: "snineclient".into(),
                relative_path: "instance/mods/snineclient.jar".into(),
                expected_hash: client_info
                    .get("sha256")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                expected_size: client_info.get("size").and_then(Value::as_u64),
                source: "https://github.com/silasnetwin/SNineLauncher/releases".into(),
                integrity: "GitHub release manifest SHA-256".into(),
            });
        }
        let installed_manifest = InstalledManifest {
            schema: 1,
            profile_id: profile_id.into(),
            minecraft_version: minecraft_version.into(),
            fabric_loader_version: loader_version.clone(),
            snine_client_version: if saved_profile.snineclient_enabled {
                client_info
                    .get("clientVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .into()
            } else {
                "disabled".into()
            },
            java_major: required_java,
            java_path: runtime_path,
            installed_at_unix: installed_at,
            last_verified_at_unix: now,
            mojang_version_metadata: format!("minecraft/metadata/{minecraft_version}.json"),
            fabric_profile_id: fabric_profile
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(&loader_version)
                .into(),
            asset_index: asset_index_id.into(),
            managed_artifacts,
            library_count: launch_data.library_count,
            asset_object_count: launch_data.asset_object_count,
        };
        atomic_json(
            &metadata_dir.join("installed-manifest.json"),
            &installed_manifest,
        )?;
        self.database
            .set_profile_installed(profile_id, true)
            .map_err(|e| e.to_string())?;
        if matches!(
            saved_profile.loader_type,
            LoaderType::Fabric | LoaderType::Quilt
        ) {
            self.database
                .set_profile_loader_version(profile_id, &loader_version)
                .map_err(|e| e.to_string())?;
        }
        self.database
            .record_profile_verified(
                profile_id,
                if saved_profile.loader_type == LoaderType::Fabric {
                    "Minecraft and Fabric files validated"
                } else {
                    "Vanilla Minecraft files validated"
                },
            )
            .map_err(|e| e.to_string())?;
        emit_stage(
            app,
            profile_id,
            "ready",
            if saved_profile.snineclient_enabled {
                "Minecraft, Fabric and SNineClient dependencies are installed"
            } else if matches!(
                saved_profile.loader_type,
                LoaderType::Fabric | LoaderType::Quilt
            ) {
                if saved_profile.loader_type == LoaderType::Quilt {
                    "Minecraft and Quilt are installed"
                } else {
                    "Minecraft and Fabric are installed"
                }
            } else {
                "Vanilla Minecraft is installed"
            },
        );
        Ok(MinecraftInstallResult {
            profile_id: profile_id.into(),
            minecraft_version: minecraft_version.into(),
            loader_version,
            main_class,
            library_count: launch_data.library_count,
            asset_count: launch_data.asset_object_count,
            bytes_downloaded,
            ready: true,
        })
    }

    pub async fn launch_profile<R: Runtime>(
        self: &Arc<Self>,
        app: AppHandle<R>,
        auth: Arc<AuthService>,
        profile_id: String,
    ) -> Result<LaunchState, String> {
        let play_started = std::time::Instant::now();
        play_timing("PLAY_CLICK", &profile_id, play_started);
        let _launch_guard = self.launch_lock.lock().await;
        validate_profile_id(&profile_id)?;
        if self
            .running
            .lock()
            .map_err(|_| "Process state unavailable")?
            .contains_key(&profile_id)
        {
            return Err("This profile is already running.".into());
        }
        let (account, access_token, xuid) = auth.require_valid_minecraft_session().await?;
        play_timing("AUTH_READY", &profile_id, play_started);
        let access_token = zeroize::Zeroizing::new(access_token);
        let xuid = zeroize::Zeroizing::new(xuid.unwrap_or_default());
        let profile_root = checked_profile_root(&self.root, &profile_id)?;
        let launch_path = profile_root.join("instance/snine-launch.json");
        let launch: ResolvedInstall = serde_json::from_slice(
            &fs::read(&launch_path)
                .map_err(|_| "Install this Minecraft profile before launching it.")?,
        )
        .map_err(|_| "Installed launch metadata is invalid; repair the profile.")?;
        if launch.profile_id != profile_id
            || !validate_launch_install(&self.root, &profile_id, &launch)
        {
            return Err("Managed Minecraft installation is incomplete; repair the profile.".into());
        }
        play_timing("INSTALL_STATE_READY", &profile_id, play_started);
        let required_java = launch
            .version_json
            .pointer("/javaVersion/majorVersion")
            .and_then(Value::as_u64)
            .unwrap_or(21) as u32;
        let java = self
            .runtime
            .detect(required_java)
            .compatible
            .ok_or_else(|| format!("No compatible Java {required_java} runtime is available."))?;
        play_timing("RUNTIME_READY", &profile_id, play_started);
        let java_path = java_gui_executable(Path::new(&java.executable));
        let game_dir = profile_root.join("instance");
        let mut args = build_launch_args(
            &launch,
            &account,
            &access_token,
            &xuid,
            crate::auth::CLIENT_ID,
            &game_dir,
            self.database
                .effective_profile_memory_mb(&profile_id)
                .map_err(|e| e.to_string())?,
        )?;
        let window = self
            .database
            .minecraft_window_settings()
            .map_err(|e| e.to_string())?;
        apply_window_settings(&mut args, &window);
        let args = zeroize::Zeroizing::new(args);
        play_timing("LAUNCH_CONFIG_READY", &profile_id, play_started);
        let mut command = Command::new(&java_path);
        command
            .args(args.iter())
            .current_dir(&game_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // `java.exe` is a console-subsystem executable. Without this flag Windows
        // creates a console window for every game process even though its output
        // is already captured by the launcher log readers below.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.as_std_mut().creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let spawned = command.spawn();
        let mut child = spawned.map_err(|error| format!("Could not start Java: {error}"))?;
        play_timing("JAVA_SPAWNED", &profile_id, play_started);
        let pid = child
            .id()
            .ok_or("Java process did not expose a process id")?;
        self.running
            .lock()
            .map_err(|_| "Process state unavailable")?
            .insert(profile_id.clone(), pid);
        if let Err(error) = self
            .database
            .record_launch_started(&profile_id, &account.username)
        {
            #[cfg(debug_assertions)]
            eprintln!("[launch] activity record failed for profile {profile_id}: {error}");
            #[cfg(not(debug_assertions))]
            let _ = error;
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let log = self
            .root
            .join("logs")
            .join(format!("minecraft-{profile_id}.log"));
        let stdout_log = log.clone();
        let verification_log = game_dir.join("logs/latest.log");
        let stdout_task = tokio::spawn(async move {
            append_child_stream(stdout, stdout_log, "stdout").await;
        });
        let stderr_task = tokio::spawn(async move {
            append_child_stream(stderr, log, "stderr").await;
        });
        let service = Arc::clone(self);
        let app_clone = app.clone();
        let profile = profile_id.clone();
        let username = account.username.clone();
        emit_launch(&app, &profile_id, "running", Some(pid), None);
        tokio::spawn(async move {
            let status = wait_child_to_completion(child).await;
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            if let Ok(mut running) = service.running.lock() {
                running.remove(&profile);
            }
            let verification = inspect_launch_log(&verification_log, &launch.loader_version).await;
            let _ = app_clone.emit(
                "launch-verification",
                LaunchVerification {
                    profile_id: profile.clone(),
                    fabric_loaded: verification.0,
                    loader_type: if launch.main_class.starts_with("org.quiltmc.loader.") {
                        "quilt".into()
                    } else if launch.main_class.starts_with("net.fabricmc.loader.") {
                        "fabric".into()
                    } else {
                        "vanilla".into()
                    },
                    snine_client_loaded: verification.1,
                    snine_client_expected: launch.snineclient_enabled,
                    loader_version: launch.loader_version.clone(),
                },
            );
            let state = match &status {
                Ok(code) => classify_exit_code(code.code()),
                Err(_) => "exited",
            };
            let detail = match &status {
                Ok(code) if code.success() => format!("{username} exited Minecraft normally"),
                Ok(code) => format!(
                    "{username}'s Minecraft process exited with code {}",
                    code.code()
                        .map_or_else(|| "unknown".into(), |value| value.to_string())
                ),
                Err(_) => format!(
                    "{username}'s Minecraft process ended; the exit status could not be read"
                ),
            };
            let _ = service
                .database
                .record_launch_ended(&profile, &detail, state == "crashed");
            emit_launch(&app_clone, &profile, state, Some(pid), Some(&detail));
        });
        Ok(LaunchState {
            profile_id,
            state: "running".into(),
            pid: Some(pid),
            message: None,
        })
    }

    pub fn launch_state(&self, profile_id: &str) -> Result<LaunchState, String> {
        validate_profile_id(profile_id)?;
        let pid = self
            .running
            .lock()
            .map_err(|_| "Process state unavailable")?
            .get(profile_id)
            .copied();
        Ok(LaunchState {
            profile_id: profile_id.into(),
            state: if pid.is_some() { "running" } else { "ready" }.into(),
            pid,
            message: None,
        })
    }

    pub async fn lock_profile_maintenance(
        &self,
        profile_id: &str,
    ) -> Result<tokio::sync::MutexGuard<'_, ()>, String> {
        validate_profile_id(profile_id)?;
        let guard = self.launch_lock.lock().await;
        if self
            .running
            .lock()
            .map_err(|_| "Process state unavailable")?
            .contains_key(profile_id)
        {
            return Err("Close Minecraft before updating or repairing this profile.".into());
        }
        Ok(guard)
    }

    pub fn resolve_profile_state(&self, profile_id: &str) -> Result<ProfileState, String> {
        validate_profile_id(profile_id)?;
        let profiles = self
            .database
            .bootstrap()
            .map_err(|e| e.to_string())?
            .profiles;
        let profile = profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or("Unknown profile")?;
        let running = self
            .running
            .lock()
            .map_err(|_| "Process state unavailable")?
            .contains_key(profile_id);
        let root = self.root.join("profiles").join(profile_id);
        let launch_path = root.join("instance/snine-launch.json");
        let launch: Option<ResolvedInstall> = fs::read(&launch_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let client_path = root.join("instance/mods/snineclient.jar");
        let client_record = read_json(&root.join("client-install.json")).ok();
        let client_valid = !profile.snineclient_enabled
            || client_record.as_ref().is_some_and(|record| {
                record
                    .get("size")
                    .and_then(Value::as_u64)
                    .is_some_and(|size| {
                        fs::metadata(&client_path)
                            .is_ok_and(|meta| meta.is_file() && meta.len() == size)
                    })
            });
        let launch_valid = launch.as_ref().is_some_and(|item| {
            item.profile_id == profile_id && validate_launch_install(&self.root, profile_id, item)
        });
        let state = derive_profile_state(profile.installed, running, client_valid, launch_valid);
        let required_java = launch
            .as_ref()
            .and_then(|item| item.version_json.pointer("/javaVersion/majorVersion"))
            .and_then(Value::as_u64)
            .unwrap_or(21) as u32;
        let runtime = self.runtime.detect(required_java);
        Ok(ProfileState {
            profile_id: profile_id.into(),
            state: state.into(),
            runtime_ready: runtime.compatible.is_some(),
            minecraft_version: launch.as_ref().map(|item| item.minecraft_version.clone()),
            loader_version: launch.map(|item| item.loader_version),
            message: (state == "RepairRequired")
                .then(|| "Managed profile files are missing or incomplete.".into()),
        })
    }

    pub fn manage_info(&self, profile_id: &str) -> Result<ProfileManageInfo, String> {
        validate_profile_id(profile_id)?;
        let profile = checked_profile_root(&self.root, profile_id)?;
        let metadata = read_json(&profile.join("instance/snine-launch.json")).ok();
        let client = read_json(&profile.join("client-install.json")).ok();
        let storage_bytes = directory_size(&profile.join("instance"));
        let required_java = metadata
            .as_ref()
            .and_then(|data| data.pointer("/versionJson/javaVersion/majorVersion"))
            .and_then(Value::as_u64)
            .unwrap_or(21) as u32;
        let runtime = self
            .runtime
            .detect(required_java)
            .compatible
            .map(|candidate| candidate.executable);
        Ok(ProfileManageInfo {
            profile_id: profile_id.into(),
            installed: metadata.is_some(),
            minecraft_version: metadata
                .as_ref()
                .and_then(|data| data.get("minecraftVersion"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            loader_version: metadata
                .as_ref()
                .and_then(|data| data.get("loaderVersion"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            client_version: client
                .as_ref()
                .and_then(|data| data.get("clientVersion"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            runtime,
            directory: profile.join("instance").display().to_string(),
            storage_bytes,
        })
    }

    pub fn profile_mods(&self, profile_id: &str) -> Result<Vec<ManagedMod>, String> {
        validate_profile_id(profile_id)?;
        if !self
            .database
            .profile_exists(profile_id)
            .map_err(|e| e.to_string())?
        {
            return Err("Unknown profile".into());
        }
        let mut records = self
            .database
            .managed_mods(profile_id)
            .map_err(|e| e.to_string())?;
        let profile_root = checked_profile_root(&self.root, profile_id)?;
        let mods_dir = profile_root.join("instance/mods");
        if !mods_dir.exists() {
            return Ok(records);
        }
        let canonical_mods = mods_dir.canonicalize().map_err(|e| e.to_string())?;
        for entry in fs::read_dir(&canonical_mods).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let enabled = !name.ends_with(".jar.disabled");
            let filename = if enabled {
                name.clone()
            } else {
                name.trim_end_matches(".disabled").to_owned()
            };
            if filename == "snineclient.jar"
                || !filename.to_ascii_lowercase().ends_with(".jar")
                || records.iter().any(|item| item.filename == filename)
            {
                continue;
            }
            let Some((mod_id, version, icon_data_url)) = read_fabric_mod_identity(&path) else {
                continue;
            };
            records.push(ManagedMod {
                profile_id: profile_id.into(),
                mod_id: mod_id.clone(),
                project_id: String::new(),
                slug: mod_id,
                version_id: version,
                filename,
                hash: String::new(),
                enabled,
                managed: false,
                icon_data_url,
            });
        }
        for record in records.iter_mut().filter(|record| record.managed) {
            let filename = if record.enabled {
                record.filename.clone()
            } else {
                format!("{}.disabled", record.filename)
            };
            record.icon_data_url = read_fabric_mod_identity(&canonical_mods.join(filename))
                .and_then(|(_, _, icon_data_url)| icon_data_url);
        }
        records.sort_by(|left, right| left.mod_id.cmp(&right.mod_id));
        Ok(records)
    }

    pub async fn install_modrinth_project<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        profile_id: &str,
        project_id: &str,
        project_slug: &str,
    ) -> Result<Vec<ManagedMod>, String> {
        let _guard = self.mod_install_lock.lock().await;
        validate_profile_id(profile_id)?;
        if !valid_modrinth_id(project_id) {
            return Err("Invalid Modrinth project identifier".into());
        }
        if !valid_modrinth_slug(project_slug) {
            return Err("Invalid Modrinth project slug".into());
        }
        let profile = self
            .database
            .bootstrap()
            .map_err(|e| e.to_string())?
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or("Unknown profile")?;
        if !profile.installed {
            return Err("Install this profile before adding mods".into());
        }
        if profile.loader_type != crate::database::LoaderType::Fabric {
            return Err("Mod installation is currently supported for Fabric profiles only".into());
        }
        let profile_root = checked_profile_root(&self.root, profile_id)?;
        let installed_manifest = read_json(&profile_root.join("instance/installed-manifest.json"))?;
        if installed_manifest.get("profileId").and_then(Value::as_str) != Some(profile_id) {
            return Err("Installed profile metadata is invalid".into());
        }
        let minecraft_version =
            required_string(&installed_manifest, "minecraftVersion")?.to_owned();
        let mods_dir = profile_root.join("instance/mods");
        create_managed_directory(&self.root, &mods_dir)?;
        let mut pending = Vec::from([project_slug.to_owned()]);
        let mut queued_refs = HashSet::from([project_slug.to_owned()]);
        let mut resolved_projects = HashSet::new();
        let mut plan = Vec::<ModFile>::new();
        while !pending.is_empty() {
            if queued_refs.len() > 32 {
                return Err("Mod dependency plan exceeded the safe limit".into());
            }
            let current = std::mem::take(&mut pending);
            let resolved = stream::iter(current.into_iter().map(|project_ref| {
                let minecraft_version = minecraft_version.clone();
                async move {
                    self.resolve_modrinth_mod(&project_ref, &minecraft_version, None)
                        .await
                }
            }))
            .buffered(8)
            .collect::<Vec<_>>()
            .await;
            for result in resolved {
                let file = result?;
                if !resolved_projects.insert(file.project_id.clone()) {
                    continue;
                }
                for dependency_project in &file.required_projects {
                    if queued_refs.insert(dependency_project.clone()) {
                        pending.push(dependency_project.clone());
                    }
                }
                plan.push(file);
            }
        }
        if plan.is_empty() {
            return Err("No compatible mod version was found".into());
        }
        let existing = self
            .database
            .managed_mods(profile_id)
            .map_err(|e| e.to_string())?;
        let installed_projects = existing
            .iter()
            .filter(|item| item.managed)
            .map(|item| item.project_id.clone())
            .collect::<HashSet<_>>();
        if mod_plan_has_conflicts(&plan, &installed_projects) {
            return Err("This mod conflicts with another installed mod. Remove the conflicting mod before installing it.".into());
        }
        let stage = mods_dir.join(format!(".snine-stage-{}", uuid::Uuid::new_v4()));
        let backup = mods_dir.join(format!(".snine-backup-{}", uuid::Uuid::new_v4()));
        create_managed_directory(&self.root, &stage)?;
        create_managed_directory(&self.root, &backup)?;
        let download_results = stream::iter(plan.iter().cloned().map(|mod_file| {
            let downloads = Arc::clone(&self.downloads);
            let app = app.clone();
            let stage = stage.clone();
            let profile_id = profile_id.to_owned();
            async move {
                safe_relative(&mod_file.filename)?;
                let staged_path = stage.join(&mod_file.filename);
                let request = DownloadRequest {
                    url: mod_file.url.clone(),
                    file_name: mod_file.filename.clone(),
                    expected_sha256: None,
                    profile_id: Some(profile_id),
                    max_bytes: Some(mod_file.size),
                };
                if !staged_path.is_file() {
                    let receipt = downloads.download_to(&app, &request, &staged_path).await?;
                    if receipt.bytes != mod_file.size
                        || !file_matches_modrinth(&staged_path, &mod_file)?
                    {
                        let _ = fs::remove_file(&staged_path);
                        return Err(format!(
                            "{} failed Modrinth hash verification",
                            mod_file.filename
                        ));
                    }
                }
                Ok::<_, String>(mod_file)
            }
        }))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        let mut ready = Vec::new();
        let mut download_error = None;
        for result in download_results {
            match result {
                Ok(file) => ready.push(file),
                Err(error) if download_error.is_none() => download_error = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = download_error {
            let _ = fs::remove_dir_all(&stage);
            let _ = fs::remove_dir_all(&backup);
            return Err(error);
        }
        let owned_names = existing
            .iter()
            .map(|item| item.filename.as_str())
            .collect::<HashSet<_>>();
        let mut moved_to_backup = Vec::<(PathBuf, PathBuf)>::new();
        let mut published = Vec::<PathBuf>::new();
        let publish_result = (|| -> Result<(), String> {
            let replacement_projects = ready
                .iter()
                .map(|file| file.project_id.as_str())
                .collect::<HashSet<_>>();
            let replacement_names = ready
                .iter()
                .map(|file| file.filename.as_str())
                .collect::<HashSet<_>>();
            for old in existing
                .iter()
                .filter(|item| replacement_projects.contains(item.project_id.as_str()))
            {
                let active_path = mods_dir.join(if old.enabled {
                    old.filename.clone()
                } else {
                    format!("{}.disabled", old.filename)
                });
                if active_path.is_file()
                    && (!old.enabled || !replacement_names.contains(old.filename.as_str()))
                {
                    let saved = backup.join(format!("old-{}", old.filename));
                    fs::rename(&active_path, &saved)
                        .map_err(|e| format!("Could not stage previous mod version: {e}"))?;
                    moved_to_backup.push((saved, active_path));
                }
            }
            for file in &ready {
                let target = mods_dir.join(&file.filename);
                if fs::symlink_metadata(&target)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err("A mod target is an unsafe filesystem link".into());
                }
                if target.exists() {
                    if file_matches_modrinth(&target, file)? {
                        continue;
                    }
                    if !owned_names.contains(file.filename.as_str()) {
                        return Err(format!("A user-managed mod already uses {}", file.filename));
                    }
                    let saved = backup.join(&file.filename);
                    fs::rename(&target, &saved).map_err(|e| {
                        format!("Could not stage existing mod for replacement: {e}")
                    })?;
                    moved_to_backup.push((saved, target.clone()));
                }
                let staged_path = stage.join(&file.filename);
                if staged_path.exists() {
                    fs::rename(&staged_path, &target)
                        .map_err(|e| format!("Could not publish staged mod: {e}"))?;
                    published.push(target);
                }
            }
            Ok(())
        })();
        if let Err(error) = publish_result {
            rollback_mod_publish(&published, &moved_to_backup);
            let _ = fs::remove_dir_all(&stage);
            let _ = fs::remove_dir_all(&backup);
            return Err(error);
        }
        let records = ready
            .iter()
            .map(|file| ManagedMod {
                profile_id: profile_id.into(),
                mod_id: file.slug.clone(),
                project_id: file.project_id.clone(),
                slug: file.slug.clone(),
                version_id: file.version_id.clone(),
                filename: file.filename.clone(),
                hash: file.hash.clone().unwrap_or_default(),
                enabled: true,
                managed: true,
                icon_data_url: None,
            })
            .collect::<Vec<_>>();
        if let Err(error) = self.database.record_managed_mods(&records) {
            rollback_mod_publish(&published, &moved_to_backup);
            let _ = fs::remove_dir_all(&stage);
            let _ = fs::remove_dir_all(&backup);
            return Err(error.to_string());
        }
        let _ = fs::remove_dir_all(&stage);
        let _ = fs::remove_dir_all(&backup);
        self.database
            .managed_mods(profile_id)
            .map_err(|e| e.to_string())
    }

    pub async fn mod_update_status(
        &self,
        profile_id: &str,
    ) -> Result<Vec<ModUpdateStatus>, String> {
        validate_profile_id(profile_id)?;
        let profile_root = checked_profile_root(&self.root, profile_id)?;
        let installed_manifest = read_json(&profile_root.join("instance/installed-manifest.json"))?;
        if installed_manifest.get("profileId").and_then(Value::as_str) != Some(profile_id) {
            return Err("Installed profile metadata is invalid".into());
        }
        let minecraft_version = required_string(&installed_manifest, "minecraftVersion")?;
        let installed = self
            .database
            .managed_mods(profile_id)
            .map_err(|error| error.to_string())?;
        let managed = installed
            .into_iter()
            .filter(|item| item.managed)
            .collect::<Vec<_>>();
        let mut updates = Vec::with_capacity(managed.len());
        for item in managed {
            let latest = self
                .resolve_modrinth_mod(&item.slug, minecraft_version, None)
                .await?;
            updates.push(ModUpdateStatus {
                project_id: item.project_id,
                slug: item.slug,
                current_version_id: item.version_id.clone(),
                latest_version_id: latest.version_id.clone(),
                update_available: item.version_id != latest.version_id,
            });
        }
        Ok(updates)
    }

    pub fn set_managed_mod_enabled(
        &self,
        profile_id: &str,
        filename: &str,
        enabled: bool,
    ) -> Result<(), String> {
        validate_profile_id(profile_id)?;
        safe_relative(filename)?;
        let mods = self
            .database
            .managed_mods(profile_id)
            .map_err(|e| e.to_string())?;
        let Some(mod_item) = mods.iter().find(|item| item.filename == filename) else {
            return Err("Managed mod not found".into());
        };
        let root = checked_profile_root(&self.root, profile_id)?;
        let mods_dir = root.join("instance/mods");
        let original = mods_dir.join(filename);
        let disabled = mods_dir.join(format!("{filename}.disabled"));
        let (from, to) = if enabled {
            (&disabled, &original)
        } else {
            (&original, &disabled)
        };
        if from.exists() {
            let canonical_parent = from
                .parent()
                .ok_or("Mod path is invalid")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if canonical_parent != mods_dir.canonicalize().map_err(|e| e.to_string())? {
                return Err("Mod file escaped its managed directory".into());
            }
            fs::rename(from, to).map_err(|e| format!("Could not change mod enabled state: {e}"))?;
        } else if !to.exists() {
            return Err("Managed mod file is missing".into());
        }
        if mod_item.managed {
            self.database
                .set_mod_enabled(profile_id, filename, enabled)
                .map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    pub fn remove_managed_mod(&self, profile_id: &str, filename: &str) -> Result<(), String> {
        validate_profile_id(profile_id)?;
        safe_relative(filename)?;
        if !self
            .profile_mods(profile_id)?
            .iter()
            .any(|item| item.filename == filename)
        {
            return Err("Managed mod not found".into());
        }
        let root = checked_profile_root(&self.root, profile_id)?;
        let mods_dir = root.join("instance/mods");
        for path in [
            &mods_dir.join(filename),
            &mods_dir.join(format!("{filename}.disabled")),
        ] {
            if path.exists() {
                if path
                    .parent()
                    .ok_or("Mod path is invalid")?
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    != mods_dir.canonicalize().map_err(|e| e.to_string())?
                {
                    return Err("Mod file escaped its managed directory".into());
                }
                fs::remove_file(path).map_err(|e| format!("Could not remove managed mod: {e}"))?;
            }
        }
        self.database
            .forget_managed_mod(profile_id, filename)
            .map_err(|e| e.to_string())
    }

    pub async fn uninstall_profile(&self, profile_id: &str) -> Result<(), String> {
        let _launch_guard = self.launch_lock.lock().await;
        validate_profile_id(profile_id)?;
        if self
            .running
            .lock()
            .map_err(|_| "Process state unavailable")?
            .contains_key(profile_id)
        {
            return Err("Close Minecraft before uninstalling this profile.".into());
        }
        if !self
            .database
            .profile_exists(profile_id)
            .map_err(|e| e.to_string())?
        {
            return Err("Unknown profile".into());
        }
        let profile = checked_profile_root(&self.root, profile_id)?;
        // Remove only launcher-owned artifacts. Saves, screenshots, configs, resource packs,
        // user mods, shared assets/libraries, and runtimes are deliberately preserved.
        remove_managed_profile_files(&profile)?;
        remove_generated_natives(&self.root, profile_id)?;
        self.database
            .mark_profile_uninstalled(profile_id)
            .map_err(|e| e.to_string())
    }

    pub async fn create_profile_backup(
        &self,
        profile_id: &str,
        destination: PathBuf,
    ) -> Result<u64, String> {
        let _guard = self.lock_profile_maintenance(profile_id).await?;
        let _install_guard = self.install_lock.lock().await;
        let _mod_guard = self.mod_install_lock.lock().await;
        let profile = checked_profile_root(&self.root, profile_id)?;
        let instance = profile.join("instance");
        let instance_metadata = fs::symlink_metadata(&instance)
            .map_err(|_| "This profile has no installed instance data to back up.")?;
        if !instance_metadata.is_dir() || instance_metadata.file_type().is_symlink() {
            return Err("This profile has no installed instance data to back up.".into());
        }
        let destination_parent = destination
            .parent()
            .ok_or("Choose a valid backup destination")?;
        let parent = destination_parent
            .canonicalize()
            .map_err(|_| "Backup destination folder is unavailable")?;
        let filename = destination
            .file_name()
            .ok_or("Choose a valid backup filename")?;
        let destination = parent.join(filename);
        if destination.starts_with(&profile) {
            return Err("Save the backup outside the profile folder.".into());
        }
        if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
            return Err(
                "A file already exists at that location. Choose a new backup filename.".into(),
            );
        }
        let profile_id = profile_id.to_owned();
        tokio::task::spawn_blocking(move || {
            write_profile_backup(&instance, &profile_id, &destination)
        })
        .await
        .map_err(|_| "Profile backup task failed.".to_string())?
    }

    pub async fn restore_profile_backup(
        &self,
        profile_id: &str,
        source: PathBuf,
    ) -> Result<u64, String> {
        let _guard = self.lock_profile_maintenance(profile_id).await?;
        let _install_guard = self.install_lock.lock().await;
        let _mod_guard = self.mod_install_lock.lock().await;
        let profile = checked_profile_root(&self.root, profile_id)?;
        let source = source
            .canonicalize()
            .map_err(|_| "Backup archive is unavailable")?;
        if source.starts_with(&profile) {
            return Err("Select a backup archive stored outside the profile folder.".into());
        }
        let profile_id = profile_id.to_owned();
        tokio::task::spawn_blocking(move || {
            restore_profile_backup_files(&profile, &profile_id, &source)
        })
        .await
        .map_err(|_| "Profile restore task failed.".to_string())?
    }

    pub fn open_profile_folder(&self, profile_id: &str) -> Result<(), String> {
        let profile = checked_profile_root(&self.root, profile_id)?;
        let instance = profile.join("instance");
        fs::create_dir_all(&instance).map_err(|e| e.to_string())?;
        let canonical = instance.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(&profile) {
            return Err("Profile folder escaped its managed directory".into());
        }
        opener::open(canonical).map_err(|e| format!("Could not open profile folder: {e}"))
    }

    pub fn open_profile_logs(&self, profile_id: &str) -> Result<(), String> {
        let profile = checked_profile_root(&self.root, profile_id)?;
        let logs = profile.join("instance/logs");
        fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let canonical = logs.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(&profile) {
            return Err("Game logs escaped the managed profile directory".into());
        }
        opener::open(canonical).map_err(|e| format!("Could not open game logs: {e}"))
    }

    pub fn read_profile_log_tail(
        &self,
        profile_id: &str,
        file_name: &str,
    ) -> Result<ProfileLogTail, String> {
        const MAX_TAIL_BYTES: u64 = 1024 * 1024;
        if !valid_profile_content_component(file_name, false)
            || Path::new(file_name)
                .file_name()
                .and_then(|name| name.to_str())
                != Some(file_name)
            || !file_name.to_ascii_lowercase().ends_with(".log")
        {
            return Err("Choose a valid Minecraft log file.".into());
        }
        let profile = checked_profile_root(&self.root, profile_id)?;
        let logs = profile.join("instance/logs");
        let logs_metadata =
            fs::symlink_metadata(&logs).map_err(|_| "No game logs are available yet.")?;
        if !logs_metadata.is_dir() || logs_metadata.file_type().is_symlink() {
            return Err("The profile log folder is not a regular directory.".into());
        }
        let canonical_logs = logs
            .canonicalize()
            .map_err(|_| "Could not open the profile log folder.")?;
        if !canonical_logs.starts_with(&profile) {
            return Err("Game logs escaped the managed profile directory.".into());
        }
        let candidate = logs.join(file_name);
        let metadata =
            fs::symlink_metadata(&candidate).map_err(|_| "That log file no longer exists.")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("The selected log is not a regular file.".into());
        }
        let canonical_file = candidate
            .canonicalize()
            .map_err(|_| "Could not resolve the selected log.")?;
        if !canonical_file.starts_with(&canonical_logs) {
            return Err("The selected log escaped the profile log folder.".into());
        }
        let file_size = metadata.len();
        let truncated = file_size > MAX_TAIL_BYTES;
        let start = file_size.saturating_sub(MAX_TAIL_BYTES);
        let mut file =
            fs::File::open(canonical_file).map_err(|_| "Could not read the selected log.")?;
        file.seek(SeekFrom::Start(start))
            .map_err(|_| "Could not seek within the selected log.")?;
        let mut bytes = Vec::with_capacity(file_size.min(MAX_TAIL_BYTES) as usize);
        file.take(MAX_TAIL_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the selected log.")?;
        Ok(ProfileLogTail {
            file_name: file_name.to_owned(),
            content: String::from_utf8_lossy(&bytes).into_owned(),
            bytes: file_size,
            truncated,
        })
    }

    pub fn profile_resource_packs(
        &self,
        profile_id: &str,
    ) -> Result<Vec<ResourcePackInfo>, String> {
        let profile = checked_profile_root(&self.root, profile_id)?;
        let folder = profile.join("instance/resourcepacks");
        fs::create_dir_all(&folder).map_err(|e| format!("Could not open resource packs: {e}"))?;
        let canonical_folder = folder.canonicalize().map_err(|e| e.to_string())?;
        let mut packs = Vec::new();
        for entry in fs::read_dir(&canonical_folder).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !metadata.file_type().is_file()
                || path.extension().and_then(|v| v.to_str()) != Some("zip")
            {
                continue;
            }
            let Some(filename) = path
                .file_name()
                .and_then(|v| v.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let (name, description, pack_format) = read_resource_pack_metadata(&path)
                .unwrap_or_else(|_| {
                    (
                        filename.trim_end_matches(".zip").to_string(),
                        "Minecraft resource pack".into(),
                        None,
                    )
                });
            packs.push(ResourcePackInfo {
                filename,
                name,
                description,
                pack_format,
                bytes: metadata.len(),
            });
        }
        packs.sort_by_key(|pack| pack.name.to_lowercase());
        Ok(packs)
    }

    pub fn import_profile_resource_pack(
        &self,
        profile_id: &str,
        source_path: &str,
    ) -> Result<ResourcePackInfo, String> {
        validate_profile_id(profile_id)?;
        let source = Path::new(source_path);
        let source_metadata = fs::symlink_metadata(source)
            .map_err(|e| format!("Could not read selected resource pack: {e}"))?;
        if !source_metadata.file_type().is_file()
            || source_metadata.len() == 0
            || source_metadata.len() > MAX_RESOURCE_PACK_BYTES as u64
        {
            return Err("Choose a regular resource pack ZIP smaller than 128 MB.".into());
        }
        let source = source
            .canonicalize()
            .map_err(|e| format!("Could not read selected resource pack: {e}"))?;
        let filename = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("Selected resource pack filename is invalid")?;
        let filename = safe_resource_pack_filename(filename)?;
        let (name, description, pack_format) = read_resource_pack_metadata(&source)?;
        let profile = checked_profile_root(&self.root, profile_id)?;
        let folder = profile.join("instance/resourcepacks");
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let canonical_folder = folder.canonicalize().map_err(|e| e.to_string())?;
        let target = canonical_folder.join(&filename);
        if target.exists() {
            return Err("A resource pack with that filename is already installed.".into());
        }
        let temp = canonical_folder.join(format!(".{filename}.importing"));
        if let Err(error) = fs::copy(&source, &temp) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Could not stage resource pack: {error}"));
        }
        if fs::metadata(&temp).map_err(|e| e.to_string())?.len() != source_metadata.len() {
            let _ = fs::remove_file(&temp);
            return Err("Selected resource pack changed while it was being copied.".into());
        }
        if let Err(error) = fs::rename(&temp, &target) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Could not add resource pack: {error}"));
        }
        Ok(ResourcePackInfo {
            filename,
            name,
            description,
            pack_format,
            bytes: source_metadata.len(),
        })
    }

    pub async fn install_modrinth_resource_pack<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        profile_id: &str,
        project_id: &str,
    ) -> Result<ResourcePackInfo, String> {
        let _guard = self.mod_install_lock.lock().await;
        validate_profile_id(profile_id)?;
        if !valid_modrinth_id(project_id) {
            return Err("Invalid Modrinth project identifier".into());
        }
        let profile = self
            .database
            .bootstrap()
            .map_err(|e| e.to_string())?
            .profiles
            .into_iter()
            .find(|item| item.id == profile_id)
            .ok_or("Unknown profile")?;
        if !profile.installed {
            return Err("Install this profile before adding resource packs".into());
        }
        let root = checked_profile_root(&self.root, profile_id)?;
        let manifest = read_json(&root.join("instance/installed-manifest.json"))?;
        if manifest.get("profileId").and_then(Value::as_str) != Some(profile_id) {
            return Err("Installed profile metadata is invalid".into());
        }
        let mc = required_string(&manifest, "minecraftVersion")?;
        let url = Url::parse(&format!("{MODRINTH_API}/project/{project_id}/version"))
            .map_err(|e| e.to_string())?;
        let response = self
            .client
            .get(url)
            .query(&[("game_versions", format!("[\"{mc}\"]"))])
            .send()
            .await
            .map_err(|_| {
                "Modrinth is unavailable. Check your connection and try again.".to_string()
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "Could not resolve compatible resource pack (HTTP {}).",
                response.status().as_u16()
            ));
        }
        let versions: Vec<Value> = response
            .json()
            .await
            .map_err(|_| "Modrinth returned invalid resource pack metadata".to_string())?;
        let selected = versions
            .into_iter()
            .find(|version| version.get("version_type").and_then(Value::as_str) == Some("release"))
            .ok_or("No compatible stable resource pack version was found")?;
        let version_id = required_string(&selected, "id")?.to_owned();
        let files = selected
            .get("files")
            .and_then(Value::as_array)
            .ok_or("Resource pack version has no files")?;
        let file = files
            .iter()
            .find(|file| file.get("primary").and_then(Value::as_bool) == Some(true))
            .or_else(|| files.first())
            .ok_or("Resource pack version has no downloadable file")?;
        let source_name = safe_resource_pack_filename(required_string(file, "filename")?)?;
        let source_url = required_string(file, "url")?.to_owned();
        let parsed = Url::parse(&source_url).map_err(|e| e.to_string())?;
        if parsed.scheme() != "https" || parsed.host_str() != Some("cdn.modrinth.com") {
            return Err("Resource pack URL is not on the trusted Modrinth CDN".into());
        }
        let size = file
            .get("size")
            .and_then(Value::as_u64)
            .ok_or("Resource pack download size is missing")?;
        if size == 0 || size > MAX_RESOURCE_PACK_BYTES as u64 {
            return Err("Resource pack exceeds the 128 MB safety limit".into());
        }
        let hashes = file
            .get("hashes")
            .ok_or("Resource pack integrity metadata is missing")?;
        let (hash, algorithm) = if let Some(value) = hashes.get("sha512").and_then(Value::as_str) {
            (value.to_owned(), "sha512")
        } else if let Some(value) = hashes.get("sha1").and_then(Value::as_str) {
            (value.to_owned(), "sha1")
        } else {
            return Err("Resource pack has no supported integrity hash".into());
        };
        let downloaded_file = ModFile {
            project_id: project_id.to_owned(),
            version_id,
            slug: project_id.to_owned(),
            url: source_url.clone(),
            filename: source_name.clone(),
            size,
            hash: Some(hash),
            hash_algorithm: Some(algorithm.into()),
            required_projects: Vec::new(),
            incompatible_projects: Vec::new(),
        };
        let folder = root.join("instance/resourcepacks");
        create_managed_directory(&root, &folder)?;
        let filename = format!("snine-modrinth-{project_id}.zip");
        let target = folder.join(&filename);
        let staging = folder.join(format!(".resource-pack-{}.partial", uuid::Uuid::new_v4()));
        let backup = folder.join(format!(".resource-pack-{}.backup", uuid::Uuid::new_v4()));
        let request = DownloadRequest {
            url: source_url,
            file_name: source_name,
            expected_sha256: None,
            profile_id: Some(profile_id.to_owned()),
            max_bytes: Some(size),
        };
        let receipt = self.downloads.download_to(app, &request, &staging).await?;
        if receipt.bytes != size || !file_matches_modrinth(&staging, &downloaded_file)? {
            let _ = fs::remove_file(&staging);
            return Err("Resource pack failed Modrinth hash verification".into());
        }
        let metadata = match read_resource_pack_metadata(&staging) {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
        };
        if target.exists() {
            if fs::symlink_metadata(&target).is_ok_and(|value| value.file_type().is_symlink()) {
                let _ = fs::remove_file(&staging);
                return Err("Resource pack target is an unsafe filesystem link".into());
            }
            fs::rename(&target, &backup)
                .map_err(|e| format!("Could not stage the previous resource pack: {e}"))?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            let _ = fs::remove_file(&staging);
            return Err(format!("Could not install resource pack: {error}"));
        }
        let _ = fs::remove_file(backup);
        Ok(ResourcePackInfo {
            filename,
            name: metadata.0,
            description: metadata.1,
            pack_format: metadata.2,
            bytes: size,
        })
    }

    pub fn remove_profile_resource_pack(
        &self,
        profile_id: &str,
        filename: &str,
    ) -> Result<(), String> {
        validate_profile_id(profile_id)?;
        let filename = safe_resource_pack_filename(filename)?;
        let profile = checked_profile_root(&self.root, profile_id)?;
        let folder = profile.join("instance/resourcepacks");
        let canonical_folder = folder
            .canonicalize()
            .map_err(|_| "Resource pack folder is not available")?;
        let target = canonical_folder.join(filename);
        let metadata = fs::symlink_metadata(&target).map_err(|_| "Resource pack not found")?;
        if !metadata.file_type().is_file()
            || target.canonicalize().map_err(|e| e.to_string())?.parent()
                != Some(canonical_folder.as_path())
        {
            return Err("Resource pack is outside its managed folder.".into());
        }
        fs::remove_file(target).map_err(|e| format!("Could not remove resource pack: {e}"))
    }

    pub fn open_profile_resource_packs(&self, profile_id: &str) -> Result<(), String> {
        let profile = checked_profile_root(&self.root, profile_id)?;
        let folder = profile.join("instance/resourcepacks");
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let canonical = folder.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(&profile) {
            return Err("Resource pack folder escaped its managed profile.".into());
        }
        opener::open(canonical).map_err(|e| format!("Could not open resource pack folder: {e}"))
    }

    pub fn profile_content_items(
        &self,
        profile_id: &str,
        section: &str,
        world: Option<&str>,
    ) -> Result<Vec<ProfileContentItem>, String> {
        let folder = self.profile_content_folder(profile_id, section, world, true)?;
        let mut items = Vec::new();
        for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            items.push(ProfileContentItem {
                name,
                directory: metadata.is_dir(),
                bytes: if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
            });
        }
        items.sort_by_key(|item| (!item.directory, item.name.to_lowercase()));
        Ok(items)
    }

    pub fn open_profile_content_folder(
        &self,
        profile_id: &str,
        section: &str,
        world: Option<&str>,
    ) -> Result<(), String> {
        let folder = self.profile_content_folder(profile_id, section, world, true)?;
        opener::open(folder).map_err(|e| format!("Could not open profile folder: {e}"))
    }

    pub fn open_profile_content_item(
        &self,
        profile_id: &str,
        section: &str,
        world: Option<&str>,
        name: &str,
    ) -> Result<(), String> {
        if !valid_profile_content_component(name, false) {
            return Err("Invalid profile content item".into());
        }
        let folder = self.profile_content_folder(profile_id, section, world, false)?;
        let target = folder.join(name);
        let metadata =
            fs::symlink_metadata(&target).map_err(|_| "Profile content item not found")?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Profile content item is not a regular file".into());
        }
        let canonical = target.canonicalize().map_err(|e| e.to_string())?;
        if !canonical
            .parent()
            .is_some_and(|parent| same_path(parent, &folder))
        {
            return Err("Profile content item escaped its managed folder".into());
        }
        opener::open(canonical).map_err(|e| format!("Could not open profile item: {e}"))
    }

    pub fn import_profile_content(
        &self,
        profile_id: &str,
        section: &str,
        world: Option<&str>,
        source_path: &str,
    ) -> Result<ProfileContentItem, String> {
        let source = Path::new(source_path);
        let source_meta = fs::symlink_metadata(source)
            .map_err(|e| format!("Could not read selected file: {e}"))?;
        if !source_meta.is_file()
            || source_meta.len() == 0
            || source_meta.len() > MAX_RESOURCE_PACK_BYTES as u64
        {
            return Err("Choose a regular file smaller than 128 MB.".into());
        }
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let name = source
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("Invalid filename")?;
        if !valid_profile_content_component(name, false) {
            return Err("Invalid filename".into());
        }
        let extension = Path::new(name)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let allowed = match section {
            "shaderpacks" => matches!(extension.as_str(), "zip" | "jar"),
            "datapacks" => extension == "zip",
            _ => false,
        };
        if !allowed {
            return Err("This file type is not supported for the selected section.".into());
        }
        validate_profile_content_archive(&source, section)?;
        let folder = self.profile_content_folder(profile_id, section, world, true)?;
        let target = folder.join(name);
        if target.exists() {
            return Err("A file with this name already exists.".into());
        }
        let staging = folder.join(format!(".{name}.snine-importing"));
        if let Err(error) = fs::copy(&source, &staging) {
            let _ = fs::remove_file(&staging);
            return Err(format!("Could not stage file: {error}"));
        }
        if fs::metadata(&staging).map_err(|e| e.to_string())?.len() != source_meta.len() {
            let _ = fs::remove_file(&staging);
            return Err("Selected file changed while being copied.".into());
        }
        if let Err(error) = fs::rename(&staging, &target) {
            let _ = fs::remove_file(&staging);
            return Err(format!("Could not add file: {error}"));
        }
        Ok(ProfileContentItem {
            name: name.into(),
            directory: false,
            bytes: source_meta.len(),
        })
    }

    pub fn remove_profile_content(
        &self,
        profile_id: &str,
        section: &str,
        world: Option<&str>,
        name: &str,
    ) -> Result<(), String> {
        if !valid_profile_content_component(name, false) {
            return Err("Invalid profile content item".into());
        }
        let folder = self.profile_content_folder(profile_id, section, world, false)?;
        let target = folder.join(name);
        let metadata =
            fs::symlink_metadata(&target).map_err(|_| "Profile content item not found")?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || !target
                .canonicalize()
                .map_err(|e| e.to_string())?
                .parent()
                .is_some_and(|parent| same_path(parent, &folder))
        {
            return Err("Only regular files inside the managed folder can be removed.".into());
        }
        fs::remove_file(target).map_err(|e| format!("Could not remove profile item: {e}"))
    }

    fn profile_content_folder(
        &self,
        profile_id: &str,
        section: &str,
        world: Option<&str>,
        create: bool,
    ) -> Result<PathBuf, String> {
        if !profile_content_section_supported(section) {
            return Err("Unsupported profile content section".into());
        }
        let profile = checked_profile_root(&self.root, profile_id)?;
        let instance = profile.join("instance");
        let canonical_instance = instance
            .canonicalize()
            .map_err(|_| "Profile instance is not available")?;
        let folder = match section {
            "shaderpacks" => instance.join("shaderpacks"),
            "worlds" => instance.join("saves"),
            "screenshots" => instance.join("screenshots"),
            "logs" => instance.join("logs"),
            "datapacks" => {
                let world = world.ok_or("Select a world before managing data packs")?;
                if !valid_profile_content_component(world, false) {
                    return Err("Invalid world name".into());
                }
                let world_dir = instance.join("saves").join(world);
                let world_meta =
                    fs::symlink_metadata(&world_dir).map_err(|_| "Selected world was not found")?;
                if !world_meta.is_dir() || world_meta.file_type().is_symlink() {
                    return Err("Selected world folder is not safe".into());
                }
                let canonical_world = world_dir.canonicalize().map_err(|e| e.to_string())?;
                if !path_is_within(&canonical_world, &canonical_instance)
                    || !canonical_world.join("level.dat").is_file()
                {
                    return Err("Selected world is not a valid managed world".into());
                }
                canonical_world.join("datapacks")
            }
            _ => return Err("Unsupported profile content section".into()),
        };
        if create {
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        }
        if !folder.is_dir() {
            return Ok(folder);
        }
        let canonical = folder.canonicalize().map_err(|e| e.to_string())?;
        if !path_is_within(&canonical, &canonical_instance) {
            return Err("Profile content folder escaped the managed instance".into());
        }
        Ok(canonical)
    }

    pub fn profile_symlinks(&self, profile_id: &str) -> Result<Vec<ProfileSymlink>, String> {
        let profile = checked_profile_root(&self.root, profile_id)?;
        let instance = profile.join("instance");
        fs::create_dir_all(&instance).map_err(|e| e.to_string())?;
        let canonical = instance.canonicalize().map_err(|e| e.to_string())?;
        let mut links = Vec::new();
        for entry in fs::read_dir(&canonical).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let target_meta = fs::read_link(&path).ok().and_then(|target| {
                let target = if target.is_absolute() {
                    target
                } else {
                    canonical.join(target)
                };
                fs::metadata(target).ok()
            });
            links.push(ProfileSymlink {
                name: entry.file_name().to_string_lossy().into_owned(),
                directory: target_meta.as_ref().is_some_and(|meta| meta.is_dir()),
                target_exists: target_meta.is_some(),
            });
        }
        links.sort_by_key(|link| link.name.to_lowercase());
        Ok(links)
    }

    pub fn create_profile_symlink(
        &self,
        profile_id: &str,
        source_path: &str,
        link_name: &str,
    ) -> Result<ProfileSymlink, String> {
        if !valid_profile_content_component(link_name, false) {
            return Err("Choose a simple link name without path components.".into());
        }
        let source = Path::new(source_path);
        let metadata = fs::symlink_metadata(source)
            .map_err(|e| format!("Could not read selected target: {e}"))?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err("Choose a regular file or folder, not a link.".into());
        }
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let profile = checked_profile_root(&self.root, profile_id)?;
        let instance = profile.join("instance");
        fs::create_dir_all(&instance).map_err(|e| e.to_string())?;
        let canonical = instance.canonicalize().map_err(|e| e.to_string())?;
        if source.starts_with(&canonical) {
            return Err("Choose a source outside this profile's own instance folder.".into());
        }
        let link = canonical.join(link_name);
        if link.exists() || fs::symlink_metadata(&link).is_ok() {
            return Err("An item with this name already exists in the profile.".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::{symlink_dir, symlink_file};
            let result = if metadata.is_dir() {
                symlink_dir(&source, &link)
            } else {
                symlink_file(&source, &link)
            };
            result.map_err(|error| format!("Windows could not create this link. Enable Developer Mode or run with link permission: {error}"))?;
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::symlink;
            symlink(&source, &link)
                .map_err(|error| format!("Could not create profile link: {error}"))?;
        }
        Ok(ProfileSymlink {
            name: link_name.to_owned(),
            directory: metadata.is_dir(),
            target_exists: true,
        })
    }

    pub fn remove_profile_symlink(&self, profile_id: &str, link_name: &str) -> Result<(), String> {
        if !valid_profile_content_component(link_name, false) {
            return Err("Invalid profile link name".into());
        }
        let profile = checked_profile_root(&self.root, profile_id)?;
        let instance = profile.join("instance");
        let canonical = instance
            .canonicalize()
            .map_err(|_| "Profile instance is not available")?;
        let link = canonical.join(link_name);
        let metadata = fs::symlink_metadata(&link).map_err(|_| "Profile link not found")?;
        if !metadata.file_type().is_symlink() {
            return Err("Only symbolic links inside this profile can be removed here.".into());
        }
        let target = fs::read_link(&link).ok().and_then(|path| {
            let path = if path.is_absolute() {
                path
            } else {
                canonical.join(path)
            };
            fs::metadata(path).ok()
        });
        if target.as_ref().is_some_and(|meta| meta.is_dir()) {
            fs::remove_dir(&link).map_err(|e| format!("Could not remove profile folder link: {e}"))
        } else {
            fs::remove_file(&link)
                .or_else(|_| fs::remove_dir(&link))
                .map_err(|e| format!("Could not remove profile link: {e}"))
        }
    }

    async fn fetch_cached_json(&self, path: &Path, url: &str) -> Result<Value, String> {
        if path.is_file() {
            if let Ok(value) = read_json(path) {
                return Ok(value);
            }
        }
        let bytes = self.fetch_bytes(url, MAX_METADATA_BYTES).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid official metadata JSON: {e}"))?;
        atomic_bytes(path, &bytes)?;
        Ok(value)
    }

    async fn fetch_verified_json(
        &self,
        path: &Path,
        url: &str,
        sha1: Option<&str>,
    ) -> Result<Value, String> {
        if path.is_file() && file_matches_sha1(path, None, sha1).unwrap_or(false) {
            return read_json(path);
        }
        let bytes = self.fetch_bytes(url, MAX_METADATA_BYTES).await?;
        if sha1.is_some_and(|expected| digest_sha1(&bytes) != expected) {
            return Err("Minecraft version metadata SHA-1 mismatch".into());
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid version metadata JSON: {e}"))?;
        atomic_bytes(path, &bytes)?;
        Ok(value)
    }

    async fn fetch_bytes(&self, url: &str, max: usize) -> Result<Vec<u8>, String> {
        let parsed = Url::parse(url).map_err(|e| e.to_string())?;
        if parsed.scheme() != "https" {
            return Err("Minecraft metadata must use HTTPS".into());
        }
        let response = self
            .client
            .get(parsed)
            .send()
            .await
            .map_err(|_| "Could not reach Minecraft metadata service".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Metadata server returned HTTP {}",
                response.status()
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > max as u64)
        {
            return Err("Minecraft metadata exceeds the safe size limit".into());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "Could not read Minecraft metadata".to_string())?;
        if bytes.len() > max {
            return Err("Minecraft metadata exceeds the safe size limit".into());
        }
        Ok(bytes.to_vec())
    }

    async fn ensure_mojang_file<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        profile: &str,
        metadata: &Value,
        target: &Path,
        label: &str,
    ) -> Result<u64, String> {
        let url = required_string(metadata, "url")?;
        let size = metadata
            .get("size")
            .and_then(Value::as_u64)
            .ok_or("Minecraft artifact metadata has no size")?;
        let sha1 = metadata
            .get("sha1")
            .and_then(Value::as_str)
            .ok_or("Minecraft artifact metadata has no SHA-1")?;
        self.ensure_external_file(app, profile, url, target, Some(size), Some(sha1), label)
            .await
    }

    // These values map directly to trusted artifact metadata and its destination.
    #[allow(clippy::too_many_arguments)]
    async fn ensure_external_file<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        profile: &str,
        url: &str,
        target: &Path,
        size: Option<u64>,
        sha1: Option<&str>,
        label: &str,
    ) -> Result<u64, String> {
        let url = Url::parse(url).map_err(|e| e.to_string())?;
        if url.scheme() != "https" {
            return Err("Managed artifacts must use HTTPS".into());
        }
        let discovered_sha1 = if sha1.is_none() {
            let checksum_url = format!("{}.sha1", url.as_str());
            let response = self
                .client
                .get(&checksum_url)
                .send()
                .await
                .map_err(|_| "Could not retrieve artifact integrity metadata".to_string())?;
            if !response.status().is_success() {
                return Err(format!(
                    "Artifact has no SHA-1 in its launcher metadata and checksum endpoint returned HTTP {}",
                    response.status()
                ));
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|_| "Could not read artifact integrity metadata".to_string())?;
            if bytes.len() > 256 {
                return Err("Artifact checksum response exceeded its size limit".into());
            }
            let checksum = String::from_utf8_lossy(&bytes)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
            if checksum.len() != 40 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Artifact checksum response was invalid".into());
            }
            Some(checksum)
        } else {
            None
        };
        let expected_sha1 = sha1.or(discovered_sha1.as_deref());
        if file_matches_sha1(target, size, expected_sha1).unwrap_or(false) {
            return Ok(0);
        }
        let file_name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid artifact file name")?
            .to_owned();
        let request = DownloadRequest {
            url: url.to_string(),
            file_name,
            expected_sha256: None,
            profile_id: Some(profile.to_owned()),
            max_bytes: Some(size.unwrap_or(MAX_ARTIFACT_BYTES).min(MAX_ARTIFACT_BYTES)),
        };
        let receipt = self.downloads.download_to(app, &request, target).await?;
        if size.is_some_and(|expected| expected != receipt.bytes)
            || !file_matches_sha1(target, size, expected_sha1)?
        {
            let _ = fs::remove_file(target);
            return Err(format!("Artifact integrity verification failed: {label}"));
        }
        Ok(receipt.bytes)
    }

    async fn resolve_fabric_loader(&self, mc: &str, minimum: &str) -> Result<String, String> {
        let url = format!("{FABRIC_META}/versions/loader/{mc}");
        let bytes = self.fetch_bytes(&url, MAX_METADATA_BYTES).await?;
        let versions: Vec<Value> = serde_json::from_slice(&bytes)
            .map_err(|_| "Fabric returned invalid loader metadata")?;
        select_fabric_loader_version(&versions, mc, minimum)
    }

    async fn resolve_quilt_loader(&self, mc: &str, requested: &str) -> Result<String, String> {
        let versions = self.quilt_loader_versions(mc).await?;
        if requested.is_empty() || requested == ">=0.0.0" {
            return versions
                .into_iter()
                .next()
                .ok_or_else(|| format!("No Quilt Loader version is available for Minecraft {mc}"));
        }
        versions
            .into_iter()
            .find(|version| version == requested)
            .ok_or_else(|| format!("Quilt Loader {requested} is not available for Minecraft {mc}"))
    }

    async fn resolve_modrinth_mod(
        &self,
        slug: &str,
        mc: &str,
        minimum: Option<&str>,
    ) -> Result<ModFile, String> {
        if !valid_modrinth_slug(slug) && !valid_modrinth_id(slug) {
            return Err("Modrinth returned an invalid project slug".into());
        }
        let url = Url::parse(&format!("{MODRINTH_API}/project/{slug}/version"))
            .map_err(|e| e.to_string())?;
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(12))
            .query(&[
                ("game_versions", format!("[\"{mc}\"]")),
                ("loaders", "[\"fabric\"]".to_owned()),
            ])
            .send()
            .await
            .map_err(|_| "Could not resolve a required Fabric mod dependency".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "Could not resolve required Fabric mod {slug} (HTTP {})",
                response.status()
            ));
        }
        let versions: Vec<Value> = response
            .json()
            .await
            .map_err(|_| "Mod dependency metadata is invalid")?;
        let selected = versions
            .into_iter()
            .find(|version| {
                let release =
                    version.get("version_type").and_then(Value::as_str) == Some("release");
                let version_number = version
                    .get("version_number")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                release
                    && minimum
                        .is_none_or(|requirement| semver_at_least(version_number, requirement))
            })
            .ok_or_else(|| {
                format!(
                    "No compatible release of required mod {slug} is available for Minecraft {mc}"
                )
            })?;
        let files = selected
            .get("files")
            .and_then(Value::as_array)
            .ok_or("Mod dependency has no files")?;
        let file = files
            .iter()
            .find(|file| file.get("primary").and_then(Value::as_bool) == Some(true))
            .or_else(|| files.first())
            .ok_or("Mod dependency has no downloadable file")?;
        let url = required_string(file, "url")?.to_owned();
        let parsed = Url::parse(&url).map_err(|e| e.to_string())?;
        if parsed.scheme() != "https"
            || parsed
                .host_str()
                .is_none_or(|host| host != "cdn.modrinth.com")
        {
            return Err("Mod dependency URL is not on the official Modrinth CDN".into());
        }
        let filename = required_string(file, "filename")?.to_owned();
        let size = file
            .get("size")
            .and_then(Value::as_u64)
            .ok_or("Mod dependency has no size")?;
        let hashes = file
            .get("hashes")
            .ok_or("Mod dependency has no integrity metadata")?;
        let sha512 = hashes
            .get("sha512")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let sha1 = hashes
            .get("sha1")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let (hash, algorithm) = if let Some(hash) = sha512 {
            (Some(hash), Some("sha512"))
        } else {
            (sha1, Some("sha1"))
        };
        if hash
            .as_deref()
            .is_none_or(|value| value.len() != if algorithm == Some("sha512") { 128 } else { 40 })
        {
            return Err("Mod dependency integrity metadata is invalid".into());
        }
        let required_projects = selected
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|dependency| {
                dependency.get("dependency_type").and_then(Value::as_str) == Some("required")
            })
            .filter_map(|dependency| {
                dependency
                    .get("project_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        let incompatible_projects = selected
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|dependency| {
                dependency.get("dependency_type").and_then(Value::as_str) == Some("incompatible")
            })
            .filter_map(|dependency| dependency.get("project_id").and_then(Value::as_str))
            .filter(|project| valid_modrinth_id(project))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let project_id = required_string(&selected, "project_id")?.to_owned();
        let version_id = required_string(&selected, "id")?.to_owned();
        if !valid_modrinth_id(&project_id) || !valid_modrinth_id(&version_id) {
            return Err("Modrinth returned invalid project version identifiers".into());
        }
        Ok(ModFile {
            project_id,
            version_id,
            slug: slug.to_owned(),
            url,
            filename,
            size,
            hash,
            hash_algorithm: algorithm.map(str::to_owned),
            required_projects,
            incompatible_projects,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedInstall {
    schema: u32,
    profile_id: String,
    minecraft_version: String,
    loader_version: String,
    #[serde(default)]
    snineclient_enabled: bool,
    main_class: String,
    client_jar: String,
    classpath: Vec<String>,
    classpath_string: String,
    native_directory: String,
    assets_directory: String,
    asset_index: String,
    version_json: Value,
    fabric_profile: Value,
    logging_argument: Option<String>,
    library_count: usize,
    asset_object_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledManifest {
    schema: u32,
    profile_id: String,
    minecraft_version: String,
    fabric_loader_version: String,
    snine_client_version: String,
    java_major: u32,
    java_path: Option<String>,
    installed_at_unix: u64,
    last_verified_at_unix: u64,
    mojang_version_metadata: String,
    fabric_profile_id: String,
    asset_index: String,
    managed_artifacts: Vec<ManagedArtifact>,
    library_count: usize,
    asset_object_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedArtifact {
    logical_id: String,
    relative_path: String,
    expected_hash: Option<String>,
    expected_size: Option<u64>,
    source: String,
    integrity: String,
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone)]
struct ModFile {
    project_id: String,
    version_id: String,
    slug: String,
    url: String,
    filename: String,
    size: u64,
    hash: Option<String>,
    hash_algorithm: Option<String>,
    required_projects: Vec<String>,
    incompatible_projects: Vec<String>,
}

fn mod_plan_has_conflicts(plan: &[ModFile], installed_projects: &HashSet<String>) -> bool {
    let planned_projects = plan
        .iter()
        .map(|file| file.project_id.as_str())
        .collect::<HashSet<_>>();
    plan.iter().any(|file| {
        file.incompatible_projects.iter().any(|project| {
            let is_planned_peer =
                planned_projects.contains(project.as_str()) && project != &file.project_id;
            let is_existing_not_replaced = installed_projects.contains(project)
                && !planned_projects.contains(project.as_str());
            is_planned_peer || is_existing_not_replaced
        })
    })
}

struct InspectedMod {
    dependencies: BTreeMap<String, String>,
}

fn inspect_snine_mod(path: &Path, minecraft_version: &str) -> Result<InspectedMod, String> {
    let file = fs::File::open(path)
        .map_err(|_| "Install the SNineClient mod before completing Minecraft setup")?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "SNineClient is not a valid JAR")?;
    let mut entry = archive
        .by_name("fabric.mod.json")
        .map_err(|_| "SNineClient JAR is missing Fabric metadata")?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|_| "Could not read SNineClient metadata")?;
    let manifest: Value =
        serde_json::from_str(&text).map_err(|_| "SNineClient metadata is invalid")?;
    if manifest.get("id").and_then(Value::as_str) != Some("snineclient") {
        return Err("SNineClient mod id is invalid".into());
    }
    let dependencies = manifest
        .get("depends")
        .and_then(Value::as_object)
        .ok_or("SNineClient dependency requirements are missing")?
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_owned())))
        .collect::<BTreeMap<_, _>>();
    if dependencies
        .get("minecraft")
        .is_none_or(|required| !required.contains(minecraft_version))
    {
        return Err(
            "SNineClient Minecraft requirement does not match the installed profile".into(),
        );
    }
    Ok(InspectedMod { dependencies })
}

fn read_fabric_mod_identity(path: &Path) -> Option<(String, String, Option<String>)> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut bytes = {
        let entry = archive.by_name("fabric.mod.json").ok()?;
        if entry.size() > 1024 * 1024 {
            return None;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        bytes
    };
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let metadata: Value = serde_json::from_slice(&bytes).ok()?;
    let mod_id = metadata.get("id")?.as_str()?.to_owned();
    if mod_id.is_empty()
        || mod_id.len() > 64
        || !mod_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
    {
        return None;
    }
    let version = metadata
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let icon_path = metadata.get("icon").and_then(|icon| {
        icon.as_str()
            .or_else(|| icon.as_object()?.values().find_map(Value::as_str))
    });
    let icon_data_url = icon_path.and_then(|icon_path| {
        if icon_path.is_empty()
            || icon_path.contains(['\\', ':'])
            || Path::new(icon_path)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return None;
        }
        let entry = archive.by_name(icon_path).ok()?;
        if entry.size() > 128 * 1024 {
            return None;
        }
        bytes.clear();
        entry.take(128 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 128 * 1024 {
            return None;
        }
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            "image/gif"
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return None;
        };
        Some(format!(
            "data:{mime};base64,{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        ))
    });
    Some((mod_id, version, icon_data_url))
}

fn mod_slugs(deps: &BTreeMap<String, String>) -> Vec<&'static str> {
    ["fabric-api", "geckolib", "owo-lib"]
        .into_iter()
        .filter(|name| deps.contains_key(*name))
        .collect()
}

fn valid_profile_content_component(value: &str, allow_hidden: bool) -> bool {
    !value.is_empty()
        && value.len() <= 180
        && (allow_hidden || !value.starts_with('.'))
        && !value.contains(['/', '\\', ':'])
        && !value.contains("..")
        && !value.chars().any(char::is_control)
        && Path::new(value).components().count() == 1
        && Path::new(value).file_name().and_then(|part| part.to_str()) == Some(value)
}

fn profile_content_section_supported(section: &str) -> bool {
    matches!(
        section,
        "shaderpacks" | "worlds" | "screenshots" | "logs" | "datapacks"
    )
}

fn validate_profile_content_archive(path: &Path, section: &str) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| format!("Could not read selected ZIP: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| "Selected file is not a valid ZIP archive")?;
    let mut has_pack_metadata = false;
    let mut has_content = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "Selected ZIP contains invalid entries")?;
        let name = entry.name();
        if name == "pack.mcmeta" {
            has_pack_metadata = true;
        }
        match section {
            "datapacks" if name.starts_with("data/") => has_content = true,
            "shaderpacks" if name.starts_with("shaders/") || name == "shaders.properties" => {
                has_content = true
            }
            _ => {}
        }
    }
    match section {
        "datapacks" if !has_pack_metadata || !has_content => Err(
            "This ZIP does not contain a Minecraft data pack (pack.mcmeta and data/ are required)."
                .into(),
        ),
        "shaderpacks" if !has_content => Err(
            "This ZIP does not contain a recognizable shader pack (shaders/ is required).".into(),
        ),
        "datapacks" | "shaderpacks" => Ok(()),
        _ => Err("Unsupported profile content archive type".into()),
    }
}

fn safe_resource_pack_filename(value: &str) -> Result<String, String> {
    let device_stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    let windows_device = matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device_stem
            .strip_prefix("COM")
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        || device_stem
            .strip_prefix("LPT")
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'));
    if value.len() > 180
        || !value.to_ascii_lowercase().ends_with(".zip")
        || value.contains("..")
        || value.starts_with('.')
        || value.ends_with('.')
        || value.ends_with(' ')
        || value
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
        || windows_device
    {
        return Err("Choose a ZIP resource pack with a safe filename.".into());
    }
    Ok(value.to_string())
}

fn read_resource_pack_metadata(path: &Path) -> Result<(String, String, Option<u32>), String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "Resource pack ZIP is invalid")?;
    let metadata = archive
        .by_name("pack.mcmeta")
        .map_err(|_| "Resource pack is missing pack.mcmeta")?;
    if metadata.size() > 64 * 1024 {
        return Err("Resource pack metadata is too large".into());
    }
    let mut bytes = Vec::new();
    metadata
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let json: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Resource pack metadata is invalid")?;
    Ok(resource_pack_summary(&json))
}

#[cfg(test)]
fn read_resource_pack_metadata_bytes(
    bytes: &[u8],
) -> Result<(String, String, Option<u32>), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| "Selected file is not a valid ZIP resource pack")?;
    let metadata = archive
        .by_name("pack.mcmeta")
        .map_err(|_| "Selected ZIP is missing pack.mcmeta and is not a Minecraft resource pack")?;
    if metadata.size() > 64 * 1024 {
        return Err("Resource pack metadata is too large".into());
    }
    let mut contents = Vec::new();
    metadata
        .take(64 * 1024 + 1)
        .read_to_end(&mut contents)
        .map_err(|e| e.to_string())?;
    let json: Value =
        serde_json::from_slice(&contents).map_err(|_| "Resource pack metadata is invalid")?;
    Ok(resource_pack_summary(&json))
}

fn resource_pack_summary(json: &Value) -> (String, String, Option<u32>) {
    let pack = json.get("pack");
    let text = |value: Option<&Value>| -> String {
        match value {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Object(object)) => object
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<String>(),
            _ => String::new(),
        }
    };
    let description = text(pack.and_then(|value| value.get("description")));
    let pack_format = pack
        .and_then(|value| value.get("pack_format"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let name = if description.is_empty() {
        "Resource Pack".into()
    } else {
        description.chars().take(64).collect()
    };
    let description = if description.is_empty() {
        "Minecraft resource pack".into()
    } else {
        description
    };
    (name, description, pack_format)
}

fn validate_profile_id(id: &str) -> Result<(), String> {
    let path = Path::new(id);
    if id.is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || id.contains(['/', '\\', ':'])
    {
        Err("Invalid managed profile ID".into())
    } else {
        Ok(())
    }
}

fn validate_version_id(version: &str) -> Result<(), String> {
    if version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
    {
        Err("Invalid Minecraft version id".into())
    } else {
        Ok(())
    }
}

fn valid_modrinth_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn valid_modrinth_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

fn rollback_mod_publish(published: &[PathBuf], backups: &[(PathBuf, PathBuf)]) {
    for path in published.iter().rev() {
        let _ = fs::remove_file(path);
    }
    for (saved, original) in backups.iter().rev() {
        let _ = fs::rename(saved, original);
    }
}
fn validate_launch_install(root: &Path, profile_id: &str, install: &ResolvedInstall) -> bool {
    let version = install.minecraft_version.as_str();
    let valid_main = if install.fabric_profile.is_null() {
        install
            .version_json
            .get("mainClass")
            .and_then(Value::as_str)
            == Some(install.main_class.as_str())
            && install.main_class.starts_with("net.minecraft.")
    } else {
        install
            .fabric_profile
            .get("mainClass")
            .and_then(Value::as_str)
            == Some(install.main_class.as_str())
            && (install.main_class.starts_with("net.fabricmc.loader.")
                || install.main_class.starts_with("org.quiltmc.loader."))
    };
    if version.is_empty()
        || version.len() > 64
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        || version == "."
        || version == ".."
        || install.version_json.get("id").and_then(Value::as_str) != Some(version)
        || !valid_main
    {
        return false;
    }
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(roots) = managed_data_roots(&root) else {
        return false;
    };
    let expected_client = root
        .join("minecraft/versions")
        .join(version)
        .join(format!("{version}.jar"));
    let expected_assets = root.join("minecraft/assets");
    let expected_natives = root
        .join("minecraft/natives")
        .join(profile_id)
        .join(version);
    let Ok(client) = expected_client.canonicalize() else {
        return false;
    };
    let Ok(assets) = expected_assets.canonicalize() else {
        return false;
    };
    let Ok(natives) = expected_natives.canonicalize() else {
        return false;
    };
    let library_roots = roots
        .iter()
        .filter_map(|base| base.join("minecraft/libraries").canonicalize().ok())
        .collect::<Vec<_>>();
    if library_roots.is_empty() {
        return false;
    }
    let client_matches = Path::new(&install.client_jar)
        .canonicalize()
        .is_ok_and(|path| same_path(&path, &client));
    let assets_matches = Path::new(&install.assets_directory)
        .canonicalize()
        .is_ok_and(|path| same_path(&path, &assets));
    let natives_matches = Path::new(&install.native_directory)
        .canonicalize()
        .is_ok_and(|path| same_path(&path, &natives));
    let managed_paths = [&client, &assets, &natives]
        .iter()
        .all(|path| path_is_within_roots(path, &roots));
    if !client_matches || !assets_matches || !natives_matches || !managed_paths {
        return false;
    }
    if !install.classpath.iter().any(|path| {
        Path::new(path)
            .canonicalize()
            .is_ok_and(|item| same_path(&item, &client))
    }) {
        return false;
    }
    for entry in &install.classpath {
        let Ok(path) = Path::new(entry).canonicalize() else {
            return false;
        };
        if !same_path(&path, &client)
            && !library_roots
                .iter()
                .any(|libraries| path_is_within(&path, libraries))
        {
            return false;
        }
    }
    let classpath = std::env::join_paths(install.classpath.iter().map(PathBuf::from));
    let classpath_matches =
        classpath.is_ok_and(|value| value.to_string_lossy() == install.classpath_string);
    let asset_index_exists = assets
        .join("indexes")
        .join(format!("{}.json", install.asset_index))
        .is_file();
    classpath_matches && asset_index_exists
}
fn valid_block_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

fn block_id_from_state_path(value: &str) -> Option<String> {
    let name = value
        .strip_prefix("assets/minecraft/blockstates/")?
        .strip_suffix(".json")?;
    valid_block_path(name).then(|| format!("minecraft:{name}"))
}

fn item_id_from_definition_path(value: &str) -> Option<String> {
    let name = value
        .strip_prefix("assets/minecraft/items/")?
        .strip_suffix(".json")?;
    valid_block_path(name).then(|| format!("minecraft:{name}"))
}

fn item_id_from_model_path(value: &str) -> Option<String> {
    let name = value
        .strip_prefix("assets/minecraft/models/item/")?
        .strip_suffix(".json")?;
    if matches!(name, "generated" | "handheld" | "handheld_rod") || name.starts_with("template_") {
        return None;
    }
    valid_block_path(name).then(|| format!("minecraft:{name}"))
}

fn block_icon_cache_root(root: &Path, version: &str, fingerprint: &str) -> PathBuf {
    root.join("cache/block-icons")
        .join(version)
        .join(fingerprint)
}

fn block_icon_catalog_from_jar(
    root: &Path,
    minecraft_version: &str,
    client_jar: &Path,
    query: &str,
    offset: usize,
    limit: usize,
) -> Result<Vec<BlockIconEntry>, String> {
    let metadata = fs::metadata(client_jar)
        .map_err(|_| "Latest Minecraft client archive is not cached yet")?;
    if !metadata.is_file() {
        return Err("Minecraft client archive is not a file".into());
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs());
    let fingerprint = format!("v3-{}-{modified}", metadata.len());
    let cache_root = block_icon_cache_root(root, minecraft_version, &fingerprint);
    fs::create_dir_all(&cache_root).map_err(|error| error.to_string())?;
    let index_path = cache_root.join("catalog.json");
    let icons: Vec<BlockIconIndexEntry> = if index_path.is_file() {
        serde_json::from_slice(&fs::read(&index_path).map_err(|error| error.to_string())?)
            .map_err(|_| "Cached Minecraft icon catalog is invalid")?
    } else {
        let file = fs::File::open(client_jar).map_err(|error| error.to_string())?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|_| "Minecraft client archive is invalid")?;
        let mut by_id = BTreeMap::<String, String>::new();
        let mut has_item_definitions = false;
        let mut model_items = Vec::new();
        for index in 0..zip.len() {
            let entry_name = zip
                .by_index(index)
                .map_err(|_| "Could not index Minecraft resources")?
                .name()
                .to_owned();
            if let Some(id) = block_id_from_state_path(&entry_name) {
                insert_icon_kind(&mut by_id, id, "block");
            }
            if let Some(id) = item_id_from_definition_path(&entry_name) {
                has_item_definitions = true;
                insert_icon_kind(&mut by_id, id, "item");
            }
            if let Some(id) = item_id_from_model_path(&entry_name) {
                model_items.push(id);
            }
        }
        if !has_item_definitions {
            for id in model_items {
                insert_icon_kind(&mut by_id, id, "item");
            }
        }
        let icons = by_id
            .into_iter()
            .map(|(id, kind)| BlockIconIndexEntry { id, kind })
            .collect::<Vec<_>>();
        atomic_json(&index_path, &icons)?;
        icons
    };

    let file = fs::File::open(client_jar).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "Minecraft client archive is invalid")?;
    let needle = normalize_block_search(query);
    let filtered = icons
        .into_iter()
        .filter(|icon| {
            needle.is_empty()
                || normalize_block_search(&format!(
                    "{} {} {}",
                    icon.id,
                    icon.id
                        .trim_start_matches("minecraft:")
                        .replace(['_', '/'], " "),
                    icon.kind.replace('_', " ")
                ))
                .contains(&needle)
        })
        .collect::<Vec<_>>();
    let start = offset.min(filtered.len());
    let end = start.saturating_add(limit.clamp(1, 80)).min(filtered.len());
    let mut result = Vec::with_capacity(end - start);
    for icon in &filtered[start..end] {
        let path = icon.id.trim_start_matches("minecraft:");
        let cache_name = format!("{}.png", icon.id.replace(':', "__").replace('/', "__"));
        let cached = cache_root.join(cache_name);
        if !cached.is_file() {
            let texture = if icon.kind == "item" {
                resolve_item_texture(&mut zip, path)
            } else {
                resolve_block_texture(&mut zip, path)
                    .or_else(|| resolve_item_texture(&mut zip, path))
            };
            if let Some(texture) = texture {
                // Show the original flat Minecraft sprite; never synthesize a 3D cube.
                fs::write(&cached, texture).map_err(|error| error.to_string())?;
            }
        }
        let preview_data_url = fs::read(&cached).ok().map(|bytes| {
            format!(
                "data:image/png;base64,{}",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
            )
        });
        result.push(BlockIconEntry {
            id: icon.id.clone(),
            label: path.replace(['_', '/'], " "),
            kind: icon.kind.clone(),
            source_version: minecraft_version.to_owned(),
            preview_data_url,
        });
    }
    Ok(result)
}

fn insert_icon_kind(by_id: &mut BTreeMap<String, String>, id: String, kind: &str) {
    by_id
        .entry(id)
        .and_modify(|existing| {
            if existing != kind {
                *existing = "block_and_item".into();
            }
        })
        .or_insert_with(|| kind.to_owned());
}

fn normalize_block_search(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace("minecraft:", "")
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn zip_bytes(zip: &mut zip::ZipArchive<fs::File>, path: &str) -> Option<Vec<u8>> {
    if safe_relative(path).is_err() {
        return None;
    }
    let mut entry = zip.by_name(path).ok()?;
    if entry.size() > 4 * 1024 * 1024 {
        return None;
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn first_blockstate_model(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            if let Some(model) = object.get("model").and_then(Value::as_str) {
                return Some(model.to_owned());
            }
            object.values().find_map(first_blockstate_model)
        }
        Value::Array(values) => values.iter().find_map(first_blockstate_model),
        _ => None,
    }
}

fn resolve_block_texture(zip: &mut zip::ZipArchive<fs::File>, block: &str) -> Option<Vec<u8>> {
    let direct = format!("assets/minecraft/textures/block/{block}.png");
    if let Some(bytes) = zip_bytes(zip, &direct) {
        return Some(bytes);
    }
    let state_path = format!("assets/minecraft/blockstates/{block}.json");
    let state: Value = serde_json::from_slice(&zip_bytes(zip, &state_path)?).ok()?;
    let model = first_blockstate_model(&state)?;
    resolve_model_texture(zip, &model, &["all", "side", "top", "front", "particle"])
}

fn resolve_item_texture(zip: &mut zip::ZipArchive<fs::File>, item: &str) -> Option<Vec<u8>> {
    let definition_path = format!("assets/minecraft/items/{item}.json");
    let model = zip_bytes(zip, &definition_path)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|definition| first_blockstate_model(&definition))
        .unwrap_or_else(|| format!("minecraft:item/{item}"));
    resolve_model_texture(
        zip,
        &model,
        &[
            "layer0", "layer1", "all", "side", "top", "front", "particle",
        ],
    )
}

fn resolve_model_texture(
    zip: &mut zip::ZipArchive<fs::File>,
    model: &str,
    preferred_keys: &[&str],
) -> Option<Vec<u8>> {
    let model_path = |id: &str| -> Option<String> {
        let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
        if namespace != "minecraft" || !valid_block_path(path) {
            return None;
        }
        Some(format!("assets/minecraft/models/{path}.json"))
    };
    let mut next = model.to_owned();
    let mut textures = BTreeMap::<String, String>::new();
    let mut seen = HashSet::new();
    for _ in 0..16 {
        if !seen.insert(next.clone()) {
            break;
        }
        let bytes = zip_bytes(zip, &model_path(&next)?)?;
        let json: Value = serde_json::from_slice(&bytes).ok()?;
        if let Some(map) = json.get("textures").and_then(Value::as_object) {
            for (key, value) in map {
                if let Some(value) = value.as_str() {
                    textures
                        .entry(key.clone())
                        .or_insert_with(|| value.to_owned());
                }
            }
        }
        let Some(parent) = json.get("parent").and_then(Value::as_str) else {
            break;
        };
        next = parent.to_owned();
    }
    for &key in preferred_keys {
        let Some(mut value) = textures.get(key).cloned() else {
            continue;
        };
        let mut aliases = HashSet::new();
        while let Some(name) = value.strip_prefix('#') {
            if !aliases.insert(name.to_owned()) {
                value.clear();
                break;
            }
            let Some(alias) = textures.get(name).cloned() else {
                value.clear();
                break;
            };
            value = alias;
        }
        if value.is_empty() {
            continue;
        }
        let (namespace, path) = value
            .split_once(':')
            .unwrap_or(("minecraft", value.as_str()));
        if namespace != "minecraft" {
            continue;
        }
        let path = path.strip_prefix("textures/").unwrap_or(path);
        let path = if path.ends_with(".png") {
            path.to_owned()
        } else {
            format!("{path}.png")
        };
        if !valid_block_path(path.trim_end_matches(".png")) {
            continue;
        }
        if let Some(bytes) = zip_bytes(zip, &format!("assets/minecraft/textures/{path}")) {
            return Some(bytes);
        }
    }
    None
}

fn validate_version_json(value: &Value, expected: &str) -> Result<(), String> {
    if value.get("id").and_then(Value::as_str) != Some(expected) {
        return Err("Mojang returned mismatched Minecraft version metadata".into());
    }
    if value.get("mainClass").and_then(Value::as_str).is_none() {
        return Err("Minecraft version metadata has no main class".into());
    }
    Ok(())
}

fn parse_minecraft_versions(
    manifest: &Value,
    include_snapshots: bool,
) -> Result<Vec<MinecraftVersionInfo>, String> {
    let versions = manifest
        .get("versions")
        .and_then(Value::as_array)
        .ok_or("Mojang version manifest has no versions")?;
    let mut result = versions
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?;
            let version_type = item.get("type")?.as_str()?;
            if version_type != "release" && !(include_snapshots && version_type == "snapshot") {
                return None;
            }
            Some(MinecraftVersionInfo {
                id: id.to_owned(),
                version_type: version_type.to_owned(),
                release_time: item.get("releaseTime")?.as_str()?.to_owned(),
                url: item.get("url")?.as_str()?.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| b.release_time.cmp(&a.release_time));
    Ok(result)
}

fn select_fabric_loader_version(
    versions: &[Value],
    minecraft_version: &str,
    requirement: &str,
) -> Result<String, String> {
    let mut choices = versions
        .iter()
        .filter_map(|value| {
            let loader = value.get("loader")?;
            let version = loader.get("version")?.as_str()?;
            let stable = loader
                .get("stable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            (stable
                && (version == requirement
                    || (requirement.starts_with(">=") && semver_at_least(version, requirement))))
            .then(|| version.to_owned())
        })
        .collect::<Vec<_>>();
    choices.sort_by(|a, b| compare_versions(b, a));
    choices.into_iter().next().ok_or_else(|| {
        format!("No stable Fabric Loader version for Minecraft {minecraft_version} satisfies {requirement}")
    })
}

fn parse_quilt_loader_versions(data: &Value) -> Result<Vec<String>, String> {
    let rows = data
        .as_array()
        .ok_or("Quilt loader metadata has no versions")?;
    let mut versions = rows
        .iter()
        .filter_map(|row| row.pointer("/loader/version")?.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    versions.sort_by(|a, b| compare_versions(b, a));
    versions.dedup();
    Ok(versions)
}
fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Minecraft metadata is missing {key}"))
}
fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("Could not read metadata cache: {e}"))?,
    )
    .map_err(|_| "Cached Minecraft metadata is invalid".into())
}
fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid metadata path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    atomic_replace_file(&temp, path)
}

#[cfg(windows)]
fn atomic_replace_file(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            windows::core::PCWSTR(source.as_ptr()),
            windows::core::PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|e| {
        format!(
            "Atomic metadata replacement failed (HRESULT 0x{:08X})",
            e.code().0 as u32
        )
    })
}

#[cfg(not(windows))]
fn atomic_replace_file(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|e| e.to_string())
}
fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    atomic_bytes(path, &bytes)
}
fn digest_sha1(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}

fn java_gui_executable(java_path: &Path) -> PathBuf {
    #[cfg(windows)]
    if java_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("java.exe"))
    {
        let javaw = java_path.with_file_name("javaw.exe");
        if javaw.is_file() {
            return javaw;
        }
    }
    java_path.to_path_buf()
}
fn file_matches_sha256(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<bool, String> {
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(false);
    }
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(false);
    };
    if metadata.len() != expected_size || !metadata.is_file() {
        return Ok(false);
    }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected_sha256))
}
pub(crate) fn ensure_profile_root(root: &Path, profile_id: &str) -> Result<PathBuf, String> {
    validate_profile_id(profile_id)?;
    let roots = managed_data_roots(root)?;
    fs::create_dir_all(root.join("profiles")).map_err(|e| e.to_string())?;
    let profiles_path = root.join("profiles");
    let profiles = profiles_path
        .canonicalize()
        .map_err(|_| "Managed profile directory is unavailable")?;
    if !path_is_within_roots(&profiles, &roots) {
        return Err("Managed profile directory escaped the launcher data directory".into());
    }
    let profile_path = profiles.join(profile_id);
    match fs::create_dir(&profile_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&profile_path).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("Managed profile path is not a safe directory".into());
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    let profile = profile_path
        .canonicalize()
        .map_err(|_| "Managed profile directory is unavailable")?;
    let leaf_mismatch = !profile
        .file_name()
        .is_some_and(|leaf| leaf.to_string_lossy().eq_ignore_ascii_case(profile_id));
    let parent_mismatch = !profile.parent().is_some_and(|parent| {
        roots.iter().any(|base| {
            let candidate = base.join("profiles");
            same_path(parent, &candidate)
                || candidate
                    .canonicalize()
                    .is_ok_and(|canonical| same_path(parent, &canonical))
        })
    });
    let outside_root = !path_is_within_roots(&profile, &roots);
    if leaf_mismatch || parent_mismatch || outside_root {
        return Err("Managed profile path escaped the launcher data directory".into());
    }
    Ok(profile)
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        normalize_windows_path(left) == normalize_windows_path(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = normalize_windows_path(path);
        let root = normalize_windows_path(root);
        path == root || path.starts_with(&(root + "\\"))
    }
    #[cfg(not(windows))]
    {
        path.starts_with(root)
    }
}

pub(crate) fn managed_data_roots(root: &Path) -> Result<Vec<PathBuf>, String> {
    let canonical = root
        .canonicalize()
        .map_err(|_| "Launcher data directory is unavailable")?;
    let mut roots = vec![canonical];
    if let Ok(effective) = effective_data_root(root) {
        if !roots.iter().any(|item| same_path(item, &effective)) {
            roots.push(effective);
        }
    }
    Ok(roots)
}

fn path_is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path_is_within(path, root))
}

#[cfg(windows)]
fn normalize_windows_path(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(without_prefix) = value.strip_prefix("\\\\?\\") {
        value = without_prefix.to_owned();
    }
    value.trim_end_matches('\\').to_ascii_lowercase()
}

fn write_profile_backup(
    instance: &Path,
    profile_id: &str,
    destination: &Path,
) -> Result<u64, String> {
    const MAX_BACKUP_BYTES: u64 = 50 * 1024 * 1024 * 1024;
    const MAX_BACKUP_FILES: usize = 250_000;
    let temporary =
        destination.with_file_name(format!(".snine-backup-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<u64, String> {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("Could not create backup file: {error}"))?;
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let metadata = serde_json::json!({ "format": "snine-profile-backup", "schemaVersion": 1, "profileId": profile_id, "createdAt": created_at });
        archive
            .start_file("snine-backup.json", options)
            .map_err(|error| error.to_string())?;
        archive
            .write_all(&serde_json::to_vec_pretty(&metadata).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;

        let mut pending = vec![instance.to_path_buf()];
        let mut file_count = 0usize;
        let mut total_bytes = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        while let Some(directory) = pending.pop() {
            let mut entries = fs::read_dir(&directory)
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
                if metadata.file_type().is_symlink() {
                    return Err("Profile contains a symbolic link; backup stopped to protect files outside the profile.".into());
                }
                if metadata.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !metadata.is_file() {
                    continue;
                }
                file_count += 1;
                total_bytes = total_bytes.saturating_add(metadata.len());
                if file_count > MAX_BACKUP_FILES || total_bytes > MAX_BACKUP_BYTES {
                    return Err("Profile backup exceeds the supported size or file count.".into());
                }
                let relative = path
                    .strip_prefix(instance)
                    .map_err(|_| "Profile file escaped the instance directory")?;
                let archive_name = Path::new("instance")
                    .join(relative)
                    .to_string_lossy()
                    .replace('\\', "/");
                archive
                    .start_file(archive_name, options)
                    .map_err(|error| error.to_string())?;
                let mut input = fs::File::open(path).map_err(|error| error.to_string())?;
                loop {
                    let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
                    if read == 0 {
                        break;
                    }
                    archive
                        .write_all(&buffer[..read])
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        let file = archive.finish().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, destination)
            .map_err(|error| format!("Could not finish profile backup: {error}"))?;
        Ok(total_bytes)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn restore_profile_backup_files(
    profile: &Path,
    profile_id: &str,
    source: &Path,
) -> Result<u64, String> {
    const MAX_BYTES: u64 = 50 * 1024 * 1024 * 1024;
    const MAX_FILES: usize = 250_000;
    let source_meta = fs::symlink_metadata(source).map_err(|_| "Backup archive is unavailable")?;
    if source_meta.file_type().is_symlink() || !source_meta.is_file() {
        return Err("Backup archive must be a regular file.".into());
    }
    let stage = profile.join(format!(".restore-staging-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage).map_err(|error| error.to_string())?;
    let result = (|| -> Result<u64, String> {
        let mut archive =
            zip::ZipArchive::new(fs::File::open(source).map_err(|error| error.to_string())?)
                .map_err(|_| "Selected file is not a SNine profile backup")?;
        if archive.len() > MAX_FILES + 1 {
            return Err("Backup contains too many entries.".into());
        }
        let metadata_bytes = {
            let mut entry = archive
                .by_name("snine-backup.json")
                .map_err(|_| "Backup metadata is missing")?;
            if entry.size() > 8192 {
                return Err("Backup metadata is invalid.".into());
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            bytes
        };
        let metadata: Value =
            serde_json::from_slice(&metadata_bytes).map_err(|_| "Backup metadata is invalid")?;
        if metadata.get("format").and_then(Value::as_str) != Some("snine-profile-backup")
            || metadata.get("schemaVersion").and_then(Value::as_u64) != Some(1)
            || metadata.get("profileId").and_then(Value::as_str) != Some(profile_id)
        {
            return Err("This backup is invalid or belongs to a different profile.".into());
        }
        let mut planned_bytes = 0u64;
        let mut planned_files = 0usize;
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|_| "Backup archive is damaged")?;
            if entry.name() == "snine-backup.json" {
                continue;
            }
            let path = entry
                .enclosed_name()
                .ok_or("Backup contains an unsafe path")?;
            let parts = path.components().collect::<Vec<_>>();
            if parts
                .first()
                .is_none_or(|part| part.as_os_str() != "instance")
                || parts
                    .iter()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err("Backup contains a file outside the profile instance.".into());
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("Backup contains a symbolic link.".into());
            }
            if !entry.is_dir() {
                planned_files += 1;
                planned_bytes = planned_bytes.saturating_add(entry.size());
            }
        }
        if planned_files > MAX_FILES || planned_bytes > MAX_BYTES {
            return Err("Backup exceeds the supported size or file count.".into());
        }
        let available = fs2::available_space(profile)
            .map_err(|_| "Could not check free disk space for restore")?;
        if available < planned_bytes.saturating_add(16 * 1024 * 1024) {
            return Err("DiskFull: not enough space to restore this profile backup.".into());
        }
        let mut total = 0u64;
        let mut count = 0usize;
        let mut buffer = vec![0u8; 1024 * 1024];
        let mut found = false;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|_| "Backup archive is damaged")?;
            if entry.name() == "snine-backup.json" {
                continue;
            }
            let relative = entry
                .enclosed_name()
                .ok_or("Backup contains an unsafe path")?
                .to_path_buf();
            let parts = relative.components().collect::<Vec<_>>();
            if parts
                .first()
                .is_none_or(|part| part.as_os_str() != "instance")
                || parts
                    .iter()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err("Backup contains a file outside the profile instance.".into());
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("Backup contains a symbolic link.".into());
            }
            let target = stage.join(&relative);
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|error| error.to_string())?;
                found = true;
                continue;
            }
            count += 1;
            total = total.saturating_add(entry.size());
            if count > MAX_FILES || total > MAX_BYTES {
                return Err("Backup exceeds the supported size or file count.".into());
            }
            found = true;
            fs::create_dir_all(target.parent().ok_or("Invalid backup path")?)
                .map_err(|error| error.to_string())?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|error| error.to_string())?;
            loop {
                let read = entry
                    .read(&mut buffer)
                    .map_err(|_| "Backup data failed integrity validation")?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| error.to_string())?;
            }
            output.sync_all().map_err(|error| error.to_string())?;
        }
        if !found {
            return Err("Backup contains no profile instance data.".into());
        }
        let instance = profile.join("instance");
        let restored = stage.join("instance");
        let previous = profile.join(format!(".instance-previous-{}", uuid::Uuid::new_v4()));
        let had_previous = match fs::symlink_metadata(&instance) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => true,
            Ok(_) => return Err("Profile instance path is not a safe directory.".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.to_string()),
        };
        if had_previous {
            fs::rename(&instance, &previous)
                .map_err(|error| format!("Could not preserve current profile data: {error}"))?;
        }
        if let Err(error) = fs::rename(&restored, &instance) {
            if had_previous {
                let _ = fs::rename(&previous, &instance);
            }
            return Err(format!("Could not activate restored profile data: {error}"));
        }
        if had_previous {
            fs::remove_dir_all(previous).map_err(|error| {
                format!("Profile restored, but old data cleanup failed: {error}")
            })?;
        }
        Ok(total)
    })();
    let _ = fs::remove_dir_all(&stage);
    result
}

fn checked_profile_root(root: &Path, profile_id: &str) -> Result<PathBuf, String> {
    validate_profile_id(profile_id)?;
    let roots = managed_data_roots(root)?;
    for base in &roots {
        let expected_profiles = base.join("profiles");
        let expected = expected_profiles.join(profile_id);
        let Ok(profile) = expected.canonicalize() else {
            continue;
        };
        let parent_ok = profile.parent().is_some_and(|parent| {
            expected_profiles
                .canonicalize()
                .is_ok_and(|canonical| same_path(parent, &canonical))
                || same_path(parent, &expected_profiles)
        });
        if parent_ok
            && profile
                .file_name()
                .is_some_and(|leaf| leaf.to_string_lossy().eq_ignore_ascii_case(profile_id))
            && path_is_within_roots(&profile, &roots)
        {
            return Ok(profile);
        }
    }
    Err("Profile lookup rejected a path outside the managed profile directory".into())
}

/// Resolve Windows package-local app-data virtualization through the existing launcher DB.
/// The DB is a regular launcher-owned file; its canonical parent is the effective root used by
/// Windows when the process is running inside an AppContainer. Outside that environment this is
/// the same canonical data directory passed by Tauri.
fn effective_data_root(root: &Path) -> Result<PathBuf, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "Launcher data directory is unavailable")?;
    let database_path = root.join("launcher.db");
    if let Ok(metadata) = fs::symlink_metadata(&database_path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Launcher database path is not a safe regular file".into());
        }
        if let Ok(database) = database_path.canonicalize() {
            if let Some(parent) = database.parent() {
                // Package virtualization can insert package/cache directories above the exact
                // same application-data leaf. Never accept a different app-data leaf.
                if same_path(
                    parent.file_name().map(Path::new).unwrap_or(parent),
                    canonical_root
                        .file_name()
                        .map(Path::new)
                        .unwrap_or(&canonical_root),
                ) {
                    return Ok(parent.to_path_buf());
                }
            }
        }
    }
    Ok(canonical_root)
}

fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().fold(0u64, |total, entry| {
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            return total;
        };
        if metadata.file_type().is_symlink() {
            return total;
        }
        if metadata.is_file() {
            total.saturating_add(metadata.len())
        } else if metadata.is_dir() {
            total.saturating_add(directory_size(&entry.path()))
        } else {
            total
        }
    })
}

fn derive_profile_state(
    installed: bool,
    running: bool,
    client_valid: bool,
    launch_valid: bool,
) -> &'static str {
    if running {
        "Running"
    } else if !installed {
        "NotInstalled"
    } else if !client_valid || !launch_valid {
        "RepairRequired"
    } else {
        "Installed"
    }
}

fn classify_exit_code(code: Option<i32>) -> &'static str {
    if code == Some(0) {
        "exited"
    } else {
        "crashed"
    }
}

async fn wait_child_to_completion(
    mut child: tokio::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    child.wait().await
}

fn managed_uninstall_targets(profile: &Path) -> [PathBuf; 6] {
    [
        profile.join("instance/mods/snineclient.jar"),
        profile.join("instance/mods/snineclient.jar.previous"),
        profile.join("instance/snine-launch.json"),
        profile.join("instance/installed-manifest.json"),
        profile.join("client-install.json"),
        profile.join("client-install.json.previous"),
    ]
}

fn remove_managed_profile_files(profile: &Path) -> Result<(), String> {
    let canonical_profile = profile.canonicalize().map_err(|e| e.to_string())?;
    if canonical_profile.parent().is_none() || canonical_profile == Path::new("/") {
        return Err("Refusing to uninstall a filesystem root".into());
    }
    let mut removals = Vec::new();
    for path in managed_uninstall_targets(&canonical_profile) {
        if !path.exists() {
            continue;
        }
        let parent = path
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .ok_or("Managed uninstall parent is unavailable")?;
        if !parent.starts_with(&canonical_profile) {
            return Err("Uninstall target escaped the managed profile directory".into());
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Uninstall target is not a regular managed file".into());
        }
        removals.push(path);
    }
    for path in removals {
        fs::remove_file(path).map_err(|e| format!("Could not remove managed profile file: {e}"))?;
    }
    Ok(())
}

fn remove_generated_natives(root: &Path, profile_id: &str) -> Result<(), String> {
    validate_profile_id(profile_id)?;
    let roots = managed_data_roots(root)?;
    let native_roots = roots
        .iter()
        .filter_map(|base| base.join("minecraft/natives").canonicalize().ok())
        .collect::<Vec<_>>();
    for base in &roots {
        let native_root = base.join("minecraft/natives");
        let natives = native_root.join(profile_id);
        if !natives.exists() {
            continue;
        }
        let canonical_root = native_root.canonicalize().map_err(|e| e.to_string())?;
        let canonical_natives = natives.canonicalize().map_err(|e| e.to_string())?;
        if !canonical_natives
            .file_name()
            .is_some_and(|leaf| leaf.to_string_lossy().eq_ignore_ascii_case(profile_id))
            || !canonical_natives.parent().is_some_and(|parent| {
                same_path(parent, &canonical_root)
                    || native_roots
                        .iter()
                        .any(|expected| same_path(parent, expected))
            })
            || !path_is_within_roots(&canonical_natives, &roots)
        {
            return Err("Profile native directory escaped the shared cache root".into());
        }
        fs::remove_dir_all(canonical_natives)
            .map_err(|e| format!("Could not remove profile native files: {e}"))?;
    }
    Ok(())
}
fn create_managed_directory(anchor: &Path, target: &Path) -> Result<(), String> {
    let roots = managed_data_roots(anchor)?;
    if !target.is_absolute() {
        return Err("Managed directory escaped the launcher data root".into());
    }
    // Profile paths may resolve into Windows' package-local AppData mirror. Use the
    // canonical/effective roots together, rather than assuming the lexical Tauri path
    // and canonical profile path have the same prefix.
    let allowed_root = roots
        .iter()
        .find(|root| path_is_within(target, root))
        .ok_or("Managed directory escaped the launcher data root")?;
    let root_depth = allowed_root.components().count();
    let mut relative = PathBuf::new();
    for component in target.components().skip(root_depth) {
        let Component::Normal(name) = component else {
            return Err("Managed directory path is invalid".into());
        };
        relative.push(name);
    }
    let mut current = allowed_root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Managed directory path is invalid".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("Managed directory contains an unsafe filesystem link".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        }
        if !path_is_within_roots(&current.canonicalize().map_err(|e| e.to_string())?, &roots) {
            return Err("Managed directory resolved outside the launcher data root".into());
        }
    }
    Ok(())
}
fn file_matches_sha1(path: &Path, size: Option<u64>, sha1: Option<&str>) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if size.is_some_and(|size| size != metadata.len()) {
        return Ok(false);
    }
    let Some(expected) = sha1 else {
        return Ok(size.is_some());
    };
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
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
fn missing_artifact_bytes(artifact: &Value, target: &Path) -> Result<u64, String> {
    let size = artifact
        .get("size")
        .and_then(Value::as_u64)
        .ok_or("Mojang artifact size is missing")?;
    let sha1 = artifact
        .get("sha1")
        .and_then(Value::as_str)
        .ok_or("Mojang artifact SHA-1 is missing")?;
    Ok(if file_matches_sha1(target, Some(size), Some(sha1))? {
        0
    } else {
        size
    })
}
fn artifact_path(artifact: &Value) -> Result<PathBuf, String> {
    let raw = required_string(artifact, "path")?;
    safe_relative(raw)
}
fn safe_relative(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Artifact metadata contains an unsafe relative path".into());
    }
    Ok(path.to_owned())
}
fn library_allowed(
    library: &Value,
    os: &str,
    arch: &str,
    features: &BTreeMap<String, bool>,
) -> bool {
    let Some(rules) = library.get("rules").and_then(Value::as_array) else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        let Some(action) = rule.get("action").and_then(Value::as_str) else {
            continue;
        };
        let mut matches = true;
        if let Some(condition) = rule.get("os") {
            if condition
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name != os)
            {
                matches = false
            }
            if condition
                .get("arch")
                .and_then(Value::as_str)
                .is_some_and(|name| name != arch)
            {
                matches = false
            }
        }
        if let Some(conditions) = rule.get("features").and_then(Value::as_object) {
            for (key, wanted) in conditions {
                if features.get(key).copied().unwrap_or(false) != wanted.as_bool().unwrap_or(false)
                {
                    matches = false
                }
            }
        }
        if matches {
            allowed = action == "allow"
        }
    }
    allowed
}
fn native_classifier(library: &Value, os: &str, arch: &str) -> Option<String> {
    let template = library.get("natives")?.get(os)?.as_str()?;
    Some(template.replace("${arch}", arch))
}
fn fabric_library_artifact(
    library: &Value,
) -> Result<(PathBuf, String, Option<String>, Option<u64>), String> {
    let name = required_string(library, "name")?;
    let artifact = library.get("downloads").and_then(|d| d.get("artifact"));
    let path = match artifact
        .and_then(|item| item.get("path"))
        .and_then(Value::as_str)
    {
        Some(path) => safe_relative(path)?,
        None => maven_path(name)?,
    };
    let repository_path = path.to_string_lossy().replace('\\', "/");
    let mut url = if let Some(url) = library.get("url").and_then(Value::as_str) {
        format!("{}/{}", url.trim_end_matches('/'), repository_path)
    } else {
        format!("https://maven.fabricmc.net/{repository_path}")
    };
    if let Some(artifact_url) = artifact
        .and_then(|item| item.get("url"))
        .and_then(Value::as_str)
    {
        url = artifact_url.to_owned();
    }
    let sha1 = artifact
        .and_then(|a| a.get("sha1"))
        .or_else(|| library.get("sha1"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let size = artifact
        .and_then(|a| a.get("size"))
        .or_else(|| library.get("size"))
        .and_then(Value::as_u64);
    Ok((path, url, sha1, size))
}
fn maven_path(name: &str) -> Result<PathBuf, String> {
    let (coordinates, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let mut parts = coordinates.split(':');
    let group = parts.next().ok_or("Invalid Maven coordinate")?;
    let artifact = parts.next().ok_or("Invalid Maven coordinate")?;
    let version = parts.next().ok_or("Invalid Maven coordinate")?;
    let classifier = parts.next();
    if parts.next().is_some() || group.is_empty() || artifact.is_empty() || version.is_empty() {
        return Err("Invalid Maven coordinate".into());
    }
    let suffix = classifier
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    let file = format!("{artifact}-{version}{suffix}.{extension}");
    safe_relative(&format!(
        "{}/{}/{}/{}",
        group.replace('.', "/"),
        artifact,
        version,
        file
    ))
}
fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    fn nums(value: &str) -> Vec<u64> {
        value
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|p| p.parse().ok())
            .collect()
    }
    nums(left).cmp(&nums(right))
}
fn semver_at_least(value: &str, requirement: &str) -> bool {
    let min = requirement
        .trim()
        .strip_prefix(">=")
        .unwrap_or(requirement.trim())
        .split_whitespace()
        .next()
        .unwrap_or("");
    compare_versions(value, min) != std::cmp::Ordering::Less
}

fn extract_natives(archive: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "Invalid native archive".to_string())?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .enclosed_name()
            .ok_or("Native archive has an unsafe path")?
            .to_owned();
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Native archive contains a link".into());
        }
        if name
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("Native archive path is unsafe".into());
        }
        if name
            .components()
            .next()
            .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case("META-INF"))
        {
            continue;
        }
        let target = destination.join(name);
        fs::create_dir_all(target.parent().ok_or("Invalid native path")?)
            .map_err(|e| e.to_string())?;
        let mut output = fs::File::create(target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn apply_window_settings(
    args: &mut Vec<String>,
    settings: &crate::database::MinecraftWindowSettings,
) {
    if settings.fullscreen {
        args.push("--fullscreen".into());
    } else {
        args.extend([
            "--width".into(),
            settings.width.to_string(),
            "--height".into(),
            settings.height.to_string(),
        ]);
    }
}

fn build_launch_args(
    install: &ResolvedInstall,
    account: &SafeAccount,
    token: &str,
    xuid: &str,
    client_id: &str,
    game_dir: &Path,
    memory_mb: u32,
) -> Result<Vec<String>, String> {
    let classpath = install
        .classpath
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let _ = classpath;
    let mut args = vec![
        "-Xms512M".to_owned(),
        format!("-Xmx{}M", memory_mb.clamp(2048, 32768)),
        format!("-Djava.library.path={}", install.native_directory),
        "-Dminecraft.launcher.brand=SNineLauncher".into(),
        format!("-Dminecraft.launcher.version={}", env!("CARGO_PKG_VERSION")),
        "-cp".into(),
        install.classpath_string.clone(),
    ];
    if let Some(jvm) = install
        .version_json
        .pointer("/arguments/jvm")
        .and_then(Value::as_array)
    {
        let mut features = BTreeMap::new();
        features.insert("is_demo_user".into(), false);
        features.insert("has_custom_resolution".into(), false);
        for item in jvm {
            for value in expand_argument(item, "windows", "x86_64", &features) {
                if value == "-cp" || value == "${classpath}" {
                    continue;
                }
                args.push(substitute(
                    &value, install, account, token, xuid, client_id, game_dir,
                ));
            }
        }
    }
    if let Some(jvm) = install
        .fabric_profile
        .pointer("/arguments/jvm")
        .and_then(Value::as_array)
    {
        for item in jvm {
            for value in expand_argument(item, "windows", "x86_64", &BTreeMap::new()) {
                if value == "-cp" || value == "${classpath}" {
                    continue;
                }
                args.push(substitute(
                    &value, install, account, token, xuid, client_id, game_dir,
                ));
            }
        }
    }
    if let Some(logging) = &install.logging_argument {
        args.push(substitute(
            logging, install, account, token, xuid, client_id, game_dir,
        ));
    }
    args.push(install.main_class.clone());
    let mut game_values = Vec::new();
    if let Some(game) = install
        .version_json
        .pointer("/arguments/game")
        .and_then(Value::as_array)
    {
        for item in game {
            game_values.extend(expand_argument(item, "windows", "x86_64", &BTreeMap::new()));
        }
    } else if let Some(legacy) = install
        .version_json
        .get("minecraftArguments")
        .and_then(Value::as_str)
    {
        game_values.extend(legacy.split_whitespace().map(str::to_owned));
    }
    if let Some(game) = install
        .fabric_profile
        .pointer("/arguments/game")
        .and_then(Value::as_array)
    {
        for item in game {
            game_values.extend(expand_argument(item, "windows", "x86_64", &BTreeMap::new()));
        }
    }
    for value in game_values {
        args.push(substitute(
            &value, install, account, token, xuid, client_id, game_dir,
        ));
    }
    Ok(args)
}
fn expand_argument(
    value: &Value,
    os: &str,
    arch: &str,
    features: &BTreeMap<String, bool>,
) -> Vec<String> {
    if let Some(text) = value.as_str() {
        return vec![text.into()];
    }
    if let Some(object) = value.as_object() {
        if !object
            .get("rules")
            .and_then(Value::as_array)
            .is_some_and(|rules| rules_allow(rules, os, arch, features))
        {
            return Vec::new();
        }
        let Some(value) = object.get("value") else {
            return Vec::new();
        };
        return if let Some(text) = value.as_str() {
            vec![text.into()]
        } else {
            value
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
    }
    Vec::new()
}
fn rule_conditions_match(
    rule: &Value,
    os: &str,
    arch: &str,
    features: &BTreeMap<String, bool>,
) -> bool {
    let mut matches = true;
    if let Some(condition) = rule.get("os") {
        if condition
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name != os)
        {
            matches = false
        }
        if condition
            .get("arch")
            .and_then(Value::as_str)
            .is_some_and(|name| name != arch)
        {
            matches = false
        }
    }
    if let Some(conditions) = rule.get("features").and_then(Value::as_object) {
        for (key, wanted) in conditions {
            if features.get(key).copied().unwrap_or(false) != wanted.as_bool().unwrap_or(false) {
                matches = false
            }
        }
    }
    matches
}
fn rules_allow(rules: &[Value], os: &str, arch: &str, features: &BTreeMap<String, bool>) -> bool {
    let mut allowed = false;
    for rule in rules {
        if rule_conditions_match(rule, os, arch, features) {
            allowed = rule.get("action").and_then(Value::as_str) == Some("allow");
        }
    }
    allowed
}
fn substitute(
    input: &str,
    install: &ResolvedInstall,
    account: &SafeAccount,
    token: &str,
    xuid: &str,
    client_id: &str,
    game_dir: &Path,
) -> String {
    input
        .replace("${natives_directory}", &install.native_directory)
        .replace("${launcher_name}", "SNineLauncher")
        .replace("${launcher_version}", env!("CARGO_PKG_VERSION"))
        .replace("${classpath}", &install.classpath_string)
        .replace("${classpath_separator}", ";")
        .replace(
            "${library_directory}",
            &PathBuf::from(&install.assets_directory)
                .parent()
                .unwrap_or(Path::new(""))
                .join("libraries")
                .to_string_lossy(),
        )
        .replace("${auth_player_name}", &account.username)
        .replace(
            "${version_name}",
            install
                .fabric_profile
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(&install.minecraft_version),
        )
        .replace("${game_directory}", &game_dir.to_string_lossy())
        .replace("${assets_root}", &install.assets_directory)
        .replace("${assets_index_name}", &install.asset_index)
        .replace("${auth_uuid}", &account.uuid)
        .replace("${auth_access_token}", token)
        .replace("${user_type}", "msa")
        .replace("${version_type}", "release")
        .replace("${clientid}", client_id)
        .replace("${auth_xuid}", xuid)
}

fn file_matches_modrinth(path: &Path, file: &ModFile) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    if fs::metadata(path).map_err(|e| e.to_string())?.len() != file.size {
        return Ok(false);
    }
    let Some(expected) = file.hash.as_deref() else {
        return Ok(false);
    };
    let mut source = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let actual = if file.hash_algorithm.as_deref() == Some("sha512") {
        let mut hasher = Sha512::new();
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        format!("{:x}", hasher.finalize())
    } else {
        let mut hasher = Sha1::new();
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        format!("{:x}", hasher.finalize())
    };
    Ok(actual.eq_ignore_ascii_case(expected))
}

async fn ensure_hash_file<R: Runtime>(
    downloads: &DownloadService,
    app: &AppHandle<R>,
    profile: &str,
    url: &str,
    target: &Path,
    size: u64,
    sha1: &str,
) -> Result<u64, String> {
    if file_matches_sha1(target, Some(size), Some(sha1))? {
        return Ok(0);
    }
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid asset name")?
        .to_owned();
    let request = DownloadRequest {
        url: url.into(),
        file_name,
        expected_sha256: None,
        profile_id: Some(profile.into()),
        max_bytes: Some(size),
    };
    let receipt = downloads.download_to_quiet(app, &request, target).await?;
    if receipt.bytes != size || !file_matches_sha1(target, Some(size), Some(sha1))? {
        let _ = fs::remove_file(target);
        return Err("Minecraft asset failed SHA-1/size verification".into());
    }
    Ok(receipt.bytes)
}
// One event snapshot intentionally carries all progress fields together.
#[allow(clippy::too_many_arguments)]
fn emit_download_task<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    profile_id: &str,
    file_name: &str,
    state: &str,
    bytes: u64,
    total: u64,
    speed: u64,
    error: Option<&str>,
) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            id: id.into(),
            file_name: file_name.into(),
            state: state.into(),
            bytes_received: bytes,
            total_bytes: Some(total),
            bytes_per_second: speed,
            eta_seconds: (speed > 0).then(|| total.saturating_sub(bytes) / speed),
            error: error.map(str::to_owned),
            profile_id: Some(profile_id.into()),
        },
    );
}
fn emit_stage<R: Runtime>(app: &AppHandle<R>, profile: &str, state: &str, detail: &str) {
    #[cfg(debug_assertions)]
    eprintln!("[install] profile={profile} state={state} detail={detail}");
    let _ = app.emit(
        "install-progress",
        json!({"profileId":profile,"state":state,"detail":detail}),
    );
}
fn emit_launch<R: Runtime>(
    app: &AppHandle<R>,
    profile: &str,
    state: &str,
    pid: Option<u32>,
    message: Option<&str>,
) {
    let _ = app.emit(
        "launch-state",
        LaunchState {
            profile_id: profile.into(),
            state: state.into(),
            pid,
            message: message.map(str::to_owned),
        },
    );
}

fn play_timing(stage: &str, profile: &str, started: std::time::Instant) {
    #[cfg(debug_assertions)]
    eprintln!(
        "[play-timing] stage={stage} profile={profile} elapsed_ms={:.1}",
        started.elapsed().as_secs_f64() * 1000.0
    );
    #[cfg(not(debug_assertions))]
    let _ = (stage, profile, started);
}

async fn inspect_launch_log(path: &Path, expected_loader: &str) -> (bool, bool) {
    use tokio::io::AsyncBufReadExt;

    let Ok(file) = tokio::fs::File::open(path).await else {
        return (false, false);
    };
    let mut lines = tokio::io::BufReader::new(file).lines();
    let mut evidence = (false, false);
    while let Ok(Some(line)) = lines.next_line().await {
        update_launch_evidence(&mut evidence, &line, expected_loader);
        if evidence.0 && evidence.1 {
            break;
        }
    }
    evidence
}

#[cfg(test)]
fn parse_launch_evidence(lines: &[String], expected_loader: &str) -> (bool, bool) {
    let mut evidence = (false, false);
    for line in lines {
        update_launch_evidence(&mut evidence, line, expected_loader);
    }
    evidence
}

fn update_launch_evidence(evidence: &mut (bool, bool), line: &str, expected_loader: &str) {
    let lower = line.to_ascii_lowercase();
    if (lower.contains("with fabric loader") || lower.contains("with quilt loader"))
        && line.contains(expected_loader)
    {
        evidence.0 = true;
    }
    let trimmed = lower.trim_start_matches(['-', ' ', '\t']);
    if trimmed.starts_with("snineclient ") || trimmed.starts_with("snineclient(") {
        evidence.1 = true;
    }
}

async fn append_child_stream<R: tokio::io::AsyncRead + Unpin>(
    stream: Option<R>,
    path: PathBuf,
    stream_name: &'static str,
) {
    let Some(mut stream) = stream else { return };
    let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    else {
        return;
    };
    let header = format!("\n--- {stream_name} ---\n");
    let _ = tokio::io::AsyncWriteExt::write_all(&mut file, header.as_bytes()).await;
    let _ = tokio::io::copy(&mut stream, &mut file).await;
    let _ = tokio::io::AsyncWriteExt::flush(&mut file).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resource_pack_metadata_is_read_without_extracting_archive() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut bytes);
            zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(br#"{"pack":{"pack_format":34,"description":{"text":"Forest Pack"}}}"#)
                .unwrap();
            zip.finish().unwrap();
        }
        let parsed = read_resource_pack_metadata_bytes(bytes.get_ref()).unwrap();
        assert_eq!(parsed.0, "Forest Pack");
        assert_eq!(parsed.1, "Forest Pack");
        assert_eq!(parsed.2, Some(34));
    }

    #[test]
    fn resource_pack_filenames_are_confined_zip_names() {
        assert_eq!(
            safe_resource_pack_filename("My Pack.zip").unwrap(),
            "My Pack.zip"
        );
        assert_eq!(
            safe_resource_pack_filename("Ästhetik.zip").unwrap(),
            "Ästhetik.zip"
        );
        for unsafe_name in [
            "../outside.zip",
            "C:\\temp.zip",
            "nested/pack.zip",
            "..pack.zip",
            ".hidden.zip",
            "pack.jar",
            "CON.zip",
            "trailing-space .zip ",
        ] {
            assert!(
                safe_resource_pack_filename(unsafe_name).is_err(),
                "accepted {unsafe_name}"
            );
        }
    }

    #[test]
    fn profile_content_names_reject_path_traversal_and_accept_world_names() {
        assert!(valid_profile_content_component("My Survival World", false));
        assert!(valid_profile_content_component(
            "screenshot 2026.png",
            false
        ));
        for unsafe_name in [
            "",
            "..",
            "../outside",
            "..\\outside",
            "C:\\Windows",
            "/tmp/file",
            ".hidden",
            "nested/name",
        ] {
            assert!(
                !valid_profile_content_component(unsafe_name, false),
                "accepted {unsafe_name}"
            );
        }
    }

    #[test]
    fn profile_content_section_allowlist_is_explicit() {
        for supported in ["shaderpacks", "worlds", "screenshots", "logs", "datapacks"] {
            assert!(profile_content_section_supported(supported));
        }
        assert!(!profile_content_section_supported("arbitrary-path"));
    }

    #[test]
    fn profile_content_archive_requires_section_specific_files() {
        let directory =
            std::env::temp_dir().join(format!("snine-content-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let datapack = directory.join("valid-datapack.zip");
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut bytes);
            zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(br#"{"pack":{"pack_format":48,"description":"QA"}}"#)
                .unwrap();
            zip.start_file(
                "data/example/functions/load.mcfunction",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"say qa").unwrap();
            zip.finish().unwrap();
        }
        fs::write(&datapack, bytes.into_inner()).unwrap();
        assert!(validate_profile_content_archive(&datapack, "datapacks").is_ok());
        assert!(validate_profile_content_archive(&datapack, "shaderpacks").is_err());
        let unrelated = directory.join("unrelated.zip");
        fs::write(&unrelated, b"not a zip").unwrap();
        assert!(validate_profile_content_archive(&unrelated, "datapacks").is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn profile_symlink_management_is_confined_and_never_removes_the_source() {
        let root = std::env::temp_dir().join(format!("snine-symlink-{}", uuid::Uuid::new_v4()));
        let profile = root.join("profiles/profile-a");
        fs::create_dir_all(profile.join("instance")).unwrap();
        let source = root.join("shared-config");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("options.txt"), b"keep").unwrap();
        let downloads = Arc::new(DownloadService::new(root.clone()).unwrap());
        let runtime = Arc::new(RuntimeService::new(root.clone(), Arc::clone(&downloads)).unwrap());
        let service = MinecraftService {
            root: root.clone(),
            downloads,
            runtime,
            database: Arc::new(Database::open(root.join("launcher.sqlite")).unwrap()),
            client: Client::new(),
            install_lock: AsyncMutex::new(()),
            mod_install_lock: AsyncMutex::new(()),
            launch_lock: AsyncMutex::new(()),
            running: Mutex::new(BTreeMap::new()),
        };
        assert!(service
            .create_profile_symlink("profile-a", source.to_str().unwrap(), "../outside")
            .is_err());
        assert!(service
            .remove_profile_symlink("profile-a", "ordinary-file")
            .is_err());
        match service.create_profile_symlink("profile-a", source.to_str().unwrap(), "shared-config")
        {
            Ok(link) => {
                assert!(link.directory);
                assert_eq!(service.profile_symlinks("profile-a").unwrap().len(), 1);
                service
                    .remove_profile_symlink("profile-a", "shared-config")
                    .unwrap();
                assert!(!profile.join("instance/shared-config").exists());
            }
            Err(_) => assert!(!profile.join("instance/shared-config").exists()),
        }
        assert_eq!(fs::read(source.join("options.txt")).unwrap(), b"keep");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_manifest_parser_filters_snapshots_and_preserves_metadata() {
        let manifest = json!({"versions":[
            {"id":"24w01a","type":"snapshot","releaseTime":"2024-01-01T00:00:00Z","url":"https://example.test/snapshot.json"},
            {"id":"1.21.1","type":"release","releaseTime":"2024-07-01T00:00:00Z","url":"https://example.test/release.json"}
        ]});
        let releases = parse_minecraft_versions(&manifest, false).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].id, "1.21.1");
        assert_eq!(releases[0].url, "https://example.test/release.json");
        assert_eq!(parse_minecraft_versions(&manifest, true).unwrap().len(), 2);
    }

    #[test]
    fn cached_version_manifest_remains_usable_without_network() {
        let root =
            std::env::temp_dir().join(format!("snine-version-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("version_manifest_v2.json");
        fs::write(&path, br#"{"versions":[{"id":"1.21.11","type":"release","releaseTime":"2026-01-01T00:00:00Z","url":"https://example.test/version.json"}]}"#).unwrap();
        let cached = read_json(&path).unwrap();
        let versions = parse_minecraft_versions(&cached, false).unwrap();
        assert_eq!(versions[0].id, "1.21.11");
        assert_eq!(versions[0].url, "https://example.test/version.json");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fabric_loader_picker_uses_stable_versions_and_honors_exact_or_minimum_request() {
        let versions = [
            json!({"loader":{"version":"0.19.5","stable":true}}),
            json!({"loader":{"version":"0.19.6","stable":false}}),
            json!({"loader":{"version":"0.19.4","stable":true}}),
        ];
        assert_eq!(
            select_fabric_loader_version(&versions, "1.21.11", ">=0.19.0").unwrap(),
            "0.19.5"
        );
        assert_eq!(
            select_fabric_loader_version(&versions, "1.21.11", "0.19.4").unwrap(),
            "0.19.4"
        );
        assert!(select_fabric_loader_version(&versions, "1.20.1", "0.20.0").is_err());
    }

    #[test]
    fn quilt_loader_versions_are_parsed_and_provider_logs_are_recognized() {
        let metadata = json!([
            {"loader":{"version":"0.20.0-beta.9"}},
            {"loader":{"version":"0.19.2"}},
            {"loader":{"version":"0.20.0-beta.8"}}
        ]);
        let versions = parse_quilt_loader_versions(&metadata).unwrap();
        assert_eq!(versions[0], "0.20.0-beta.9");
        assert!(versions.contains(&"0.19.2".to_owned()));
        let log = vec![
            "[main/INFO]: Loading Minecraft 1.21.11 with Quilt Loader 0.20.0-beta.9".to_owned(),
        ];
        assert_eq!(parse_launch_evidence(&log, "0.20.0-beta.9"), (true, false));
    }

    #[test]
    fn block_catalog_discovers_only_safe_minecraft_blockstate_paths() {
        assert_eq!(
            block_id_from_state_path("assets/minecraft/blockstates/diamond_block.json").as_deref(),
            Some("minecraft:diamond_block")
        );
        assert_eq!(
            block_id_from_state_path("assets/minecraft/blockstates/copper/door.json").as_deref(),
            Some("minecraft:copper/door")
        );
        assert_eq!(
            block_id_from_state_path("assets/minecraft/blockstates/../escape.json"),
            None
        );
        assert!(normalize_block_search("Minecraft:Diamond Block").contains("diamondblock"));
        assert!(normalize_block_search("oak_planks").contains("oakplanks"));
        let root = Path::new("C:/managed");
        let first = block_icon_cache_root(root, "1.21.11", "123-456");
        let second = block_icon_cache_root(root, "1.20.1", "123-456");
        assert_ne!(first, second);
        assert!(valid_block_path("minecraft/stone"));
        assert!(!valid_block_path("../escape"));
    }

    #[test]
    fn minecraft_window_settings_map_to_vanilla_arguments() {
        let mut args = vec!["net.minecraft.client.main.Main".into()];
        apply_window_settings(
            &mut args,
            &crate::database::MinecraftWindowSettings {
                width: 1920,
                height: 1080,
                fullscreen: false,
            },
        );
        assert!(args.windows(2).any(|pair| pair == ["--width", "1920"]));
        assert!(args.windows(2).any(|pair| pair == ["--height", "1080"]));
        let mut fullscreen = Vec::new();
        apply_window_settings(
            &mut fullscreen,
            &crate::database::MinecraftWindowSettings {
                width: 1280,
                height: 720,
                fullscreen: true,
            },
        );
        assert_eq!(fullscreen, ["--fullscreen"]);
    }

    #[test]
    fn vanilla_launch_config_uses_mojang_main_class_without_fabric() {
        let install = ResolvedInstall {
            schema: 1,
            profile_id: "fixture".into(),
            minecraft_version: "1.21.11".into(),
            loader_version: "vanilla".into(),
            snineclient_enabled: false,
            main_class: "net.minecraft.client.main.Main".into(),
            client_jar: "client.jar".into(),
            classpath: vec!["client.jar".into()],
            classpath_string: "client.jar".into(),
            native_directory: "natives".into(),
            assets_directory: "assets".into(),
            asset_index: "1.21".into(),
            version_json: json!({"id":"1.21.11","mainClass":"net.minecraft.client.main.Main","arguments":{"jvm":[],"game":[]}}),
            fabric_profile: Value::Null,
            logging_argument: None,
            library_count: 0,
            asset_object_count: 0,
        };
        let account = SafeAccount {
            uuid: "0123456789abcdef0123456789abcdef".into(),
            username: "Tester".into(),
            last_authenticated_at: 1,
        };
        let args = build_launch_args(
            &install,
            &account,
            "opaque",
            "",
            "client",
            Path::new("instance"),
            4096,
        )
        .unwrap();
        assert!(args
            .iter()
            .any(|arg| arg == "net.minecraft.client.main.Main"));
        assert!(!args.iter().any(|arg| arg.contains("fabric")));
    }

    #[test]
    fn model_parent_and_texture_alias_resolve_to_builtin_texture() {
        let root =
            std::env::temp_dir().join(format!("snine-block-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.jar");
        let file = fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("assets/minecraft/blockstates/blue_stone.json", br#"{"variants":{"":{"model":"minecraft:block/blue_stone"}}}"#.as_slice()),
            ("assets/minecraft/models/block/blue_stone.json", br##"{"parent":"minecraft:block/cube_all","textures":{"all":"#base","base":"minecraft:block/stone"}}"##.as_slice()),
            ("assets/minecraft/models/block/cube_all.json", br#"{"textures":{"all":"minecraft:block/stone"}}"#.as_slice()),
            ("assets/minecraft/textures/block/stone.png", b"fixture-png".as_slice()),
        ] { writer.start_file(name, options).unwrap(); writer.write_all(bytes).unwrap(); }
        writer.finish().unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(
            resolve_block_texture(&mut archive, "blue_stone").unwrap(),
            b"fixture-png"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn block_catalog_indexes_version_jar_searches_and_reuses_cached_previews() {
        let root =
            std::env::temp_dir().join(format!("snine-block-catalog-{}", uuid::Uuid::new_v4()));
        let version_dir = root.join("minecraft/versions/1.21.11");
        fs::create_dir_all(&version_dir).unwrap();
        let jar_path = version_dir.join("1.21.11.jar");
        let mut texture_png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            16,
            16,
            image::Rgba([72, 122, 52, 255]),
        ))
        .write_to(&mut texture_png, image::ImageFormat::Png)
        .unwrap();
        let archive_file = fs::File::create(&jar_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        let options = zip::write::SimpleFileOptions::default();
        for name in ["stone", "grass_block", "diamond_block"] {
            archive
                .start_file(format!("assets/minecraft/blockstates/{name}.json"), options)
                .unwrap();
            archive
                .write_all(
                    format!("{{\"variants\":{{\"\":{{\"model\":\"minecraft:block/{name}\"}}}}}}")
                        .as_bytes(),
                )
                .unwrap();
            archive
                .start_file(
                    format!("assets/minecraft/models/block/{name}.json"),
                    options,
                )
                .unwrap();
            archive
                .write_all(
                    br#"{"parent":"minecraft:block/cube_all","textures":{"all":"minecraft:block/stone"}}"#,
                )
                .unwrap();
        }
        archive
            .start_file("assets/minecraft/models/block/cube_all.json", options)
            .unwrap();
        archive
            .write_all(br#"{"textures":{"all":"minecraft:block/stone"}}"#)
            .unwrap();
        archive
            .start_file("assets/minecraft/textures/block/stone.png", options)
            .unwrap();
        archive.write_all(texture_png.get_ref()).unwrap();
        archive.finish().unwrap();

        let downloads = Arc::new(DownloadService::new(root.join("downloads")).unwrap());
        let runtime = Arc::new(RuntimeService::new(root.clone(), Arc::clone(&downloads)).unwrap());
        let database = Arc::new(Database::open(root.join("launcher.db")).unwrap());
        let service = MinecraftService::new(root.clone(), downloads, runtime, database).unwrap();
        let first = service.block_icon_catalog("1.21.11", "", 0, 64).unwrap();
        assert_eq!(first.len(), 3);
        assert!(first.iter().all(|entry| entry
            .preview_data_url
            .as_deref()
            .is_some_and(|data| data.starts_with("data:image/png;base64,"))));
        assert_eq!(
            first
                .iter()
                .find(|entry| entry.id == "minecraft:diamond_block")
                .unwrap()
                .label,
            "diamond block"
        );
        let filtered = service
            .block_icon_catalog("1.21.11", "minecraft:diamond block", 0, 64)
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "minecraft:diamond_block");
        assert!(root.join("cache/block-icons/1.21.11").is_dir());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn mod_update_plan_checks_installed_and_planned_conflicts() {
        let mut existing = HashSet::new();
        existing.insert("other-mod".to_owned());
        let file = |project_id: &str, incompatible_projects: &[&str]| ModFile {
            project_id: project_id.into(),
            version_id: "version".into(),
            slug: project_id.into(),
            url: "https://cdn.modrinth.com/data/file.jar".into(),
            filename: format!("{project_id}.jar"),
            size: 1,
            hash: Some("0".repeat(128)),
            hash_algorithm: Some("sha512".into()),
            required_projects: Vec::new(),
            incompatible_projects: incompatible_projects
                .iter()
                .map(|value| (*value).into())
                .collect(),
        };

        assert!(mod_plan_has_conflicts(
            &[file("new-mod", &["other-mod"])],
            &existing
        ));
        assert!(mod_plan_has_conflicts(
            &[
                file("new-mod", &["required-dependency"]),
                file("required-dependency", &[])
            ],
            &HashSet::new()
        ));
        assert!(!mod_plan_has_conflicts(&[file("new-mod", &[])], &existing));
    }

    #[test]
    fn mod_publish_rollback_restores_previous_managed_files() {
        let root =
            std::env::temp_dir().join(format!("snine-mod-rollback-{}", uuid::Uuid::new_v4()));
        let mods = root.join("mods");
        let backup = mods.join(".backup");
        fs::create_dir_all(&backup).unwrap();
        let old = mods.join("example.jar");
        let saved = backup.join("example.jar");
        let fresh = mods.join("new.jar");
        fs::write(&saved, b"old mod").unwrap();
        fs::write(&fresh, b"new mod").unwrap();
        rollback_mod_publish(
            std::slice::from_ref(&fresh),
            &[(saved.clone(), old.clone())],
        );
        assert!(!fresh.exists());
        assert_eq!(fs::read(old).unwrap(), b"old mod");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_state_resolution_covers_install_repair_and_process_lifecycle() {
        assert_eq!(
            derive_profile_state(false, false, false, false),
            "NotInstalled"
        );
        assert_eq!(derive_profile_state(true, false, true, true), "Installed");
        assert_eq!(
            derive_profile_state(true, false, false, true),
            "RepairRequired"
        );
        assert_eq!(derive_profile_state(true, true, true, true), "Running");
    }

    #[test]
    fn process_exit_classification_requires_nonzero_exit_code_for_crash() {
        assert_eq!(classify_exit_code(Some(0)), "exited");
        assert_eq!(classify_exit_code(Some(1)), "crashed");
        assert_eq!(classify_exit_code(None), "crashed");
    }

    #[test]
    fn explicit_artifact_verification_finds_only_missing_or_corrupt_files() {
        let root =
            std::env::temp_dir().join(format!("snine-repair-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let valid = root.join("library.jar");
        let missing = root.join("missing-asset");
        let corrupt = root.join("fabric-loader.jar");
        fs::write(&valid, b"fixture-library").unwrap();
        fs::write(&corrupt, b"damaged-artifact").unwrap();
        let expected = "fixture-library";
        let expected_sha1 = digest_sha1(expected.as_bytes());
        let corrupt_sha1 = digest_sha1(b"expected-fabric-artifact");
        assert!(
            file_matches_sha1(&valid, Some(expected.len() as u64), Some(&expected_sha1)).unwrap()
        );
        assert!(!file_matches_sha1(
            &missing,
            Some(10),
            Some("0000000000000000000000000000000000000000")
        )
        .unwrap());
        assert!(!file_matches_sha1(
            &corrupt,
            Some(b"expected-fabric-artifact".len() as u64),
            Some(&corrupt_sha1)
        )
        .unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn harmless_child_process_exit_lifecycle_clears_running_for_both_exit_codes() {
        let normal = test_exit_process(0);
        let normal_status = wait_child_to_completion(normal).await.unwrap();
        let crash = test_exit_process(23);
        let crash_status = wait_child_to_completion(crash).await.unwrap();
        let normal_state = if normal_status.success() {
            "Ready"
        } else {
            "ReadyWithCrash"
        };
        let crash_state = if crash_status.success() {
            "Ready"
        } else {
            "ReadyWithCrash"
        };
        assert_eq!(normal_state, "Ready");
        assert_eq!(crash_state, "ReadyWithCrash");
        assert_eq!(classify_exit_code(normal_status.code()), "exited");
        assert_eq!(classify_exit_code(crash_status.code()), "crashed");
        let stale_running = false; // startup has no managed child after restart
        assert_eq!(
            derive_profile_state(true, stale_running, true, true),
            "Installed"
        );
    }

    #[cfg(windows)]
    fn test_exit_process(code: i32) -> tokio::process::Child {
        tokio::process::Command::new("cmd")
            .args(["/C", &format!("exit {code}")])
            .spawn()
            .unwrap()
    }

    #[cfg(not(windows))]
    fn test_exit_process(code: i32) -> tokio::process::Child {
        tokio::process::Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .spawn()
            .unwrap()
    }

    #[test]
    fn uninstall_targets_only_launcher_owned_profile_files() {
        let root = Path::new("C:/managed/profiles/test-profile");
        let targets = managed_uninstall_targets(root);
        assert_eq!(targets.len(), 6);
        assert!(targets.iter().all(|target| target.starts_with(root)));
        let names = targets
            .iter()
            .map(|target| target.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name.ends_with("snineclient.jar")));
        assert!(!names.iter().any(|name| name.contains("saves")
            || name.contains("resourcepacks")
            || name.contains("minecraft/assets")));
    }

    #[test]
    fn folder_and_logs_resolve_from_profile_id_inside_managed_root() {
        let root = std::env::temp_dir().join(format!("snine-open-paths-{}", uuid::Uuid::new_v4()));
        let profile_root = root.join("profiles/test-profile");
        fs::create_dir_all(profile_root.join("instance/logs")).unwrap();
        let resolved = checked_profile_root(&root, "test-profile").unwrap();
        let folder = resolved.join("instance");
        let logs = resolved.join("instance/logs");
        assert_eq!(
            folder.canonicalize().unwrap(),
            profile_root.join("instance").canonicalize().unwrap()
        );
        assert_eq!(
            logs.canonicalize().unwrap(),
            profile_root.join("instance/logs").canonicalize().unwrap()
        );
        assert!(checked_profile_root(&root, "../outside").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_removes_managed_install_but_preserves_user_data_and_siblings() {
        let base = std::env::temp_dir().join(format!("snine-uninstall-{}", uuid::Uuid::new_v4()));
        let profile = base.join("profiles/p1");
        let sibling = base.join("profiles/p2");
        for path in [
            profile.join("instance/mods"),
            profile.join("instance/saves/world"),
            profile.join("instance/screenshots"),
            profile.join("instance/resourcepacks"),
            profile.join("instance/shaderpacks"),
            profile.join("instance/config"),
            sibling.join("instance/mods"),
            base.join("minecraft/natives/p1"),
            base.join("minecraft/libraries/shared"),
            base.join("minecraft/assets/shared"),
            base.join("runtimes/java-21"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(profile.join("instance/mods/snineclient.jar"), b"managed").unwrap();
        fs::write(profile.join("client-install.json"), b"metadata").unwrap();
        fs::write(
            profile.join("instance/saves/world/level.dat"),
            b"personal save",
        )
        .unwrap();
        fs::write(sibling.join("instance/mods/snineclient.jar"), b"sibling").unwrap();
        fs::write(
            profile.join("instance/screenshots/image.png"),
            b"personal screenshot",
        )
        .unwrap();
        fs::write(
            profile.join("instance/resourcepacks/custom.zip"),
            b"personal pack",
        )
        .unwrap();
        fs::write(
            profile.join("instance/shaderpacks/custom.zip"),
            b"personal shader",
        )
        .unwrap();
        fs::write(
            profile.join("instance/config/options.txt"),
            b"personal config",
        )
        .unwrap();
        fs::write(
            base.join("minecraft/natives/p1/native.dll"),
            b"generated native",
        )
        .unwrap();
        fs::write(
            base.join("minecraft/libraries/shared/library.jar"),
            b"shared library",
        )
        .unwrap();
        fs::write(base.join("minecraft/assets/shared/object"), b"shared asset").unwrap();
        fs::write(base.join("runtimes/java-21/java.exe"), b"managed runtime").unwrap();
        remove_managed_profile_files(&profile).unwrap();
        remove_generated_natives(&base, "p1").unwrap();
        assert!(!profile.join("instance/mods/snineclient.jar").exists());
        assert!(!profile.join("client-install.json").exists());
        assert_eq!(
            fs::read(profile.join("instance/saves/world/level.dat")).unwrap(),
            b"personal save"
        );
        assert_eq!(
            fs::read(sibling.join("instance/mods/snineclient.jar")).unwrap(),
            b"sibling"
        );
        assert!(!base.join("minecraft/natives/p1").exists());
        for preserved in [
            profile.join("instance/screenshots/image.png"),
            profile.join("instance/resourcepacks/custom.zip"),
            profile.join("instance/shaderpacks/custom.zip"),
            profile.join("instance/config/options.txt"),
            base.join("minecraft/libraries/shared/library.jar"),
            base.join("minecraft/assets/shared/object"),
            base.join("runtimes/java-21/java.exe"),
        ] {
            assert!(
                preserved.exists(),
                "preserved file missing: {}",
                preserved.display()
            );
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn official_version_metadata_fixture_is_valid() {
        let value:Value=serde_json::from_str(r#"{"id":"1.21.11","mainClass":"net.minecraft.client.main.Main","javaVersion":{"majorVersion":21},"downloads":{"client":{"url":"https://piston-data.mojang.com/client.jar","sha1":"0000000000000000000000000000000000000000","size":10}},"arguments":{"game":["--username","${auth_player_name}"]}}"#).unwrap();
        assert!(validate_version_json(&value, "1.21.11").is_ok());
        assert!(validate_version_json(&value, "1.21.10").is_err());
    }
    #[test]
    fn library_rules_obey_allow_and_disallow_and_windows_arch() {
        let lib:Value=serde_json::from_str(r#"{"rules":[{"action":"allow","os":{"name":"windows"}},{"action":"disallow","os":{"arch":"x86"}}]}"#).unwrap();
        assert!(library_allowed(&lib, "windows", "x86_64", &BTreeMap::new()));
        assert!(!library_allowed(&lib, "windows", "x86", &BTreeMap::new()));
        assert!(!library_allowed(&lib, "linux", "x86_64", &BTreeMap::new()));
    }
    #[test]
    fn resolves_native_classifier_from_metadata() {
        let lib: Value =
            serde_json::from_str(r#"{"natives":{"windows":"natives-windows-${arch}"}}"#).unwrap();
        assert_eq!(
            native_classifier(&lib, "windows", "64").as_deref(),
            Some("natives-windows-64")
        );
    }

    #[test]
    fn parses_fabric_meta_library_checksum_and_maven_path() {
        let library: Value = serde_json::from_str(
            r#"{"name":"net.fabricmc:intermediary:1.21.11","url":"https://maven.fabricmc.net/","sha1":"f1e2033afc8b637150c223b2bfd352d37544bc96","size":797685}"#,
        )
        .unwrap();
        let (path, url, sha1, size) = fabric_library_artifact(&library).unwrap();
        assert_eq!(
            path,
            PathBuf::from("net/fabricmc/intermediary/1.21.11/intermediary-1.21.11.jar")
        );
        assert_eq!(
            url,
            "https://maven.fabricmc.net/net/fabricmc/intermediary/1.21.11/intermediary-1.21.11.jar"
        );
        assert_eq!(
            sha1.as_deref(),
            Some("f1e2033afc8b637150c223b2bfd352d37544bc96")
        );
        assert_eq!(size, Some(797685));
    }
    #[test]
    fn argument_placeholders_are_replaced_without_logging() {
        let install = ResolvedInstall {
            schema: 1,
            profile_id: "p".into(),
            minecraft_version: "1.21.11".into(),
            loader_version: "0.19.3".into(),
            snineclient_enabled: false,
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            client_jar: "client.jar".into(),
            classpath: vec![],
            classpath_string: "libs;client.jar".into(),
            native_directory: "natives".into(),
            assets_directory: "assets".into(),
            asset_index: "29".into(),
            version_json: json!({}),
            fabric_profile: json!({}),
            logging_argument: None,
            library_count: 0,
            asset_object_count: 0,
        };
        let account = SafeAccount {
            uuid: "uuid".into(),
            username: "player".into(),
            last_authenticated_at: 0,
        };
        let value = substitute(
            "${auth_access_token}",
            &install,
            &account,
            "secret",
            "xuid",
            "client-id",
            Path::new("game"),
        );
        assert_eq!(value, "secret");
        let redacted = redact(&["--accessToken".into(), value, "player".into()]);
        assert_eq!(redacted, vec!["--accessToken", "<redacted>", "player"]);
    }

    #[test]
    fn launch_config_uses_mojang_and_fabric_arguments_without_shell_joining() {
        let install = ResolvedInstall {
            schema: 1,
            profile_id: "p".into(),
            minecraft_version: "1.21.11".into(),
            loader_version: "0.19.5".into(),
            snineclient_enabled: false,
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            client_jar: "client.jar".into(),
            classpath: vec!["library.jar".into(), "client.jar".into()],
            classpath_string: "library.jar;client.jar".into(),
            native_directory: "natives".into(),
            assets_directory: "assets".into(),
            asset_index: "29".into(),
            version_json: json!({"arguments":{"jvm":["-Dlauncher=${launcher_name}"],"game":["--username","${auth_player_name}","--accessToken","${auth_access_token}"]}}),
            fabric_profile: json!({"id":"fabric-loader-0.19.5-1.21.11","arguments":{"jvm":["-DFabricMcEmu= net.minecraft.client.main.Main "]}}),
            logging_argument: None,
            library_count: 1,
            asset_object_count: 1,
        };
        let account = SafeAccount {
            uuid: "uuid".into(),
            username: "player".into(),
            last_authenticated_at: 0,
        };
        let args = build_launch_args(
            &install,
            &account,
            "secret-token",
            "xuid",
            "client-id",
            Path::new("game dir"),
            6144,
        )
        .unwrap();
        assert!(args.iter().any(|arg| arg == "-cp"));
        assert!(args.iter().any(|arg| arg == "library.jar;client.jar"));
        assert!(args.iter().any(|arg| arg == "-Dlauncher=SNineLauncher"));
        assert!(args.iter().any(|arg| arg == "--username"));
        assert!(args.iter().any(|arg| arg == "-Xmx6144M"));
        assert!(args.iter().any(|arg| arg == "player"));
        assert!(args.iter().any(|arg| arg == &install.main_class));
        let safe = redact(&args);
        assert!(!safe.iter().any(|arg| arg.contains("secret-token")));
    }
    #[test]
    fn rejects_unsafe_artifact_and_archive_paths() {
        for p in ["../x.jar", "C:/x.jar", "/x.jar", "a/../../b.jar"] {
            assert!(safe_relative(p).is_err());
        }
        let path = std::env::temp_dir().join(format!("snine-natives-{}.zip", uuid::Uuid::new_v4()));
        use std::io::Write;
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("../escape.dll", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        let dest = path.with_extension("out");
        assert!(extract_natives(&path, &dest).is_err());
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(dest);
    }
    #[test]
    fn loader_minimum_comparison_is_numeric() {
        assert!(semver_at_least("0.19.3", ">=0.19.3"));
        assert!(semver_at_least("0.20.0", ">=0.19.3"));
        assert!(!semver_at_least("0.9.9", ">=0.19.3"));
    }

    #[test]
    fn ensure_profile_root_creates_only_canonical_profile_directory() {
        let root =
            std::env::temp_dir().join(format!("snine-profile-root-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let result = ensure_profile_root(&root, &id).unwrap();
        assert_eq!(result.file_name().unwrap(), id.as_str());
        assert!(result.is_dir());
        assert!(ensure_profile_root(&root, "../escape").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn launch_evidence_requires_fabric_and_snineclient_log_markers() {
        let only_fabric =
            vec!["[main/INFO]: Loading Minecraft 1.21.11 with Fabric Loader 0.19.5".into()];
        assert_eq!(parse_launch_evidence(&only_fabric, "0.19.5"), (true, false));
        let loaded = vec![
            "[main/INFO]: Loading Minecraft 1.21.11 with Fabric Loader 0.19.5".into(),
            "- snineclient 1.1.5".into(),
        ];
        assert_eq!(parse_launch_evidence(&loaded, "0.19.5"), (true, true));
        assert_eq!(parse_launch_evidence(&loaded, "0.19.4"), (false, true));
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
fn redact(args: &[String]) -> Vec<String> {
    let mut result = args.to_vec();
    let mut redact_next = false;
    for arg in &mut result {
        if redact_next {
            *arg = "<redacted>".into();
            redact_next = false;
            continue;
        }
        if arg == "--accessToken" || arg == "--clientToken" {
            redact_next = true
        } else if arg.starts_with("--accessToken=") {
            *arg = "--accessToken=<redacted>".into();
        }
    }
    result
}
