//! Opt-in native provider smoke test. This is compiled into debug builds only and runs only
//! when SNINE_REAL_PROVIDER_QA=1 is present in the environment.
use crate::{
    auth::AuthService,
    database::{Database, LoaderType, NewProfileConfig},
    downloads::DownloadService,
    minecraft::MinecraftService,
    runtime::RuntimeService,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tauri::{AppHandle, Runtime};

pub async fn run<R: Runtime>(
    app: AppHandle<R>,
    root: PathBuf,
    downloads: Arc<DownloadService>,
    runtime: Arc<RuntimeService>,
    auth: Arc<AuthService>,
) -> Result<(), String> {
    let db = Arc::new(Database::open(":memory:").map_err(|e| e.to_string())?);
    let service = Arc::new(
        MinecraftService::new(root.clone(), downloads, runtime, Arc::clone(&db))
            .map_err(|e| e.to_string())?,
    );
    let vanilla = db
        .create_profile_config(NewProfileConfig {
            name: "Vanilla QA".into(),
            minecraft_version: "1.21.11".into(),
            loader_type: LoaderType::Vanilla,
            loader_version: None,
            snineclient_enabled: false,
            memory_mb: Some(4096),
            icon_id: None,
        })
        .map_err(|e| e.to_string())?;
    let fabric_versions = service.fabric_loader_versions("1.21.11").await?;
    let fabric_version = fabric_versions
        .first()
        .ok_or("No stable Fabric loader is available for Minecraft 1.21.11")?
        .clone();
    let fabric = db
        .create_profile_config(NewProfileConfig {
            name: "Fabric QA".into(),
            minecraft_version: "1.21.11".into(),
            loader_type: LoaderType::Fabric,
            loader_version: Some(fabric_version.clone()),
            snineclient_enabled: false,
            memory_mb: Some(4096),
            icon_id: None,
        })
        .map_err(|e| e.to_string())?;
    let quilt_versions = service.quilt_loader_versions("1.21.11").await?;
    let quilt_version = quilt_versions
        .first()
        .ok_or("No Quilt Loader is available for Minecraft 1.21.11")?
        .clone();
    let quilt = db
        .create_profile_config(NewProfileConfig {
            name: "Quilt QA".into(),
            minecraft_version: "1.21.11".into(),
            loader_type: LoaderType::Quilt,
            loader_version: Some(quilt_version.clone()),
            snineclient_enabled: false,
            memory_mb: Some(4096),
            icon_id: None,
        })
        .map_err(|e| e.to_string())?;
    crate::minecraft::ensure_profile_root(&root, &vanilla.id)?;
    crate::minecraft::ensure_profile_root(&root, &fabric.id)?;
    crate::minecraft::ensure_profile_root(&root, &quilt.id)?;
    let mut running = Vec::new();

    let result = async {
        let shared_client = root.join("minecraft/versions/1.21.11/1.21.11.jar");
        if !shared_client.is_file() {
            return Err("The existing shared Minecraft 1.21.11 cache is unavailable".into());
        }
        let vanilla_install = service.install_profile(&app, &vanilla.id).await?;
        if vanilla_install.main_class != "net.minecraft.client.main.Main" {
            return Err("Vanilla resolved a non-Mojang main class".into());
        }
        let vanilla_root = root.join("profiles").join(&vanilla.id).join("instance");
        let vanilla_launch: serde_json::Value = serde_json::from_slice(
            &fs::read(vanilla_root.join("snine-launch.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let vanilla_classpath = vanilla_launch["classpath"]
            .as_array()
            .ok_or("Vanilla launch classpath is missing")?;
        let vanilla_mods = vanilla_root.join("mods");
        if vanilla_launch["mainClass"] != "net.minecraft.client.main.Main"
            || !vanilla_launch["fabricProfile"].is_null()
            || vanilla_launch["snineclientEnabled"] != false
            || !Path::new(vanilla_launch["clientJar"].as_str().unwrap_or_default()).is_file()
            || vanilla_classpath.iter().any(|entry| {
                entry.as_str().is_some_and(|path| {
                    let path = path.to_ascii_lowercase();
                    path.contains("fabric") || path.contains("snineclient")
                })
            })
            || fs::read_dir(&vanilla_mods)
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains("snineclient"))
        {
            return Err("Vanilla install contains Fabric or SNineClient state".into());
        }
        let vanilla_state = service.resolve_profile_state(&vanilla.id)?;
        if vanilla_state.state != "Installed" {
            return Err(format!(
                "Vanilla profile state resolved to {} (runtime_ready={})",
                vanilla_state.state, vanilla_state.runtime_ready
            ));
        }

        let fabric_install = service.install_profile(&app, &fabric.id).await?;
        if fabric_install.main_class != "net.fabricmc.loader.impl.launch.knot.KnotClient" {
            return Err("Fabric did not resolve its KnotClient main class".into());
        }
        let fabric_root = root.join("profiles").join(&fabric.id).join("instance");
        let fabric_launch: serde_json::Value = serde_json::from_slice(
            &fs::read(fabric_root.join("snine-launch.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let fabric_classpath = fabric_launch["classpath"]
            .as_array()
            .ok_or("Fabric launch classpath is missing")?;
        if fabric_launch["loaderVersion"] != fabric_version
            || fabric_launch["mainClass"] != "net.fabricmc.loader.impl.launch.knot.KnotClient"
            || fabric_launch["snineclientEnabled"] != false
            || !fabric_classpath.iter().any(|entry| {
                entry
                    .as_str()
                    .is_some_and(|path| path.contains("fabric-loader"))
            })
            || !fabric_root.join("mods").is_dir()
            || fabric_root.join("mods/snineclient.jar").exists()
        {
            return Err("Generic Fabric installation is incomplete or includes SNineClient".into());
        }
        let fabric_state = service.resolve_profile_state(&fabric.id)?;
        if fabric_state.state != "Installed" {
            return Err(format!(
                "Fabric-without-SNine profile state resolved to {} (runtime_ready={})",
                fabric_state.state, fabric_state.runtime_ready
            ));
        }

        let quilt_install = service.install_profile(&app, &quilt.id).await?;
        let quilt_root = root.join("profiles").join(&quilt.id).join("instance");
        let quilt_launch: serde_json::Value = serde_json::from_slice(
            &fs::read(quilt_root.join("snine-launch.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if quilt_install.main_class != "org.quiltmc.loader.impl.launch.knot.KnotClient"
            || quilt_launch["mainClass"] != "org.quiltmc.loader.impl.launch.knot.KnotClient"
            || quilt_launch["snineclientEnabled"] != false
            || !quilt_root.join("mods").is_dir()
            || quilt_root.join("mods/snineclient.jar").exists()
            || !quilt_launch["classpath"].as_array().is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry
                        .as_str()
                        .is_some_and(|path| path.contains("quilt-loader"))
                })
            })
        {
            return Err("Quilt install did not produce a generic Quilt profile".into());
        }
        let quilt_state = service.resolve_profile_state(&quilt.id)?;
        if quilt_state.state != "Installed" {
            return Err(format!(
                "Quilt profile state resolved to {}",
                quilt_state.state
            ));
        }

        let mods = service
            .install_modrinth_project(&app, &fabric.id, "AANobbMI", "sodium")
            .await?;
        if mods.is_empty()
            || mods
                .iter()
                .any(|item| !item.managed || item.hash.is_empty() || item.profile_id != fabric.id)
        {
            return Err("Modrinth Sodium QA artifacts failed managed hash verification".into());
        }
        for item in &mods {
            if !fabric_root.join("mods").join(&item.filename).is_file() {
                return Err(format!(
                    "Modrinth artifact {} is missing after install",
                    item.filename
                ));
            }
        }

        let auth_ready = auth.state()?.status == "loggedIn";
        let mut vanilla_started = None;
        let mut fabric_started = None;
        let mut quilt_started = None;
        let mut sodium_loaded = None;
        if auth_ready {
            let vanilla_log = vanilla_root.join("logs/latest.log");
            vanilla_started = Some(
                launch_and_stop(
                    &service,
                    app.clone(),
                    Arc::clone(&auth),
                    &vanilla.id,
                    &vanilla_log,
                    "Setting user:",
                    &mut running,
                )
                .await?,
            );
            let fabric_log = fabric_root.join("logs/latest.log");
            fabric_started = Some(
                launch_and_stop(
                    &service,
                    app.clone(),
                    Arc::clone(&auth),
                    &fabric.id,
                    &fabric_log,
                    &format!("Loading Minecraft 1.21.11 with Fabric Loader {fabric_version}"),
                    &mut running,
                )
                .await?,
            );
            let quilt_log = quilt_root.join("logs/latest.log");
            quilt_started = Some(
                launch_and_stop(
                    &service,
                    app.clone(),
                    Arc::clone(&auth),
                    &quilt.id,
                    &quilt_log,
                    &format!("Loading Minecraft 1.21.11 with Quilt Loader {quilt_version}"),
                    &mut running,
                )
                .await?,
            );
            let log = fs::read_to_string(&fabric_log)
                .unwrap_or_default()
                .to_ascii_lowercase();
            sodium_loaded = Some(log.contains("sodium"));
            if vanilla_started != Some(true)
                || fabric_started != Some(true)
                || quilt_started != Some(true)
            {
                return Err(
                    "A Java process spawned but the expected game-start marker was not observed"
                        .into(),
                );
            }
        }

        for item in mods.iter().rev() {
            service.remove_managed_mod(&fabric.id, &item.filename)?;
        }
        if service
            .profile_mods(&fabric.id)?
            .iter()
            .any(|item| mods.iter().any(|removed| removed.filename == item.filename))
        {
            return Err("Modrinth QA remove left managed Sodium artifacts behind".into());
        }

        Ok::<_, String>((
            vanilla_install,
            fabric_install,
            quilt_install,
            mods,
            auth_ready,
            vanilla_started,
            fabric_started,
            quilt_started,
            sodium_loaded,
        ))
    }
    .await;

    for pid in running {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .await;
    }
    for profile in [&vanilla, &fabric, &quilt] {
        let profile_root = root.join("profiles").join(&profile.id);
        if profile_root.exists() {
            if let Err(error) = service.uninstall_profile(&profile.id).await {
                eprintln!(
                    "[real-provider-qa] cleanup uninstall {} failed: {error}",
                    profile.id
                );
            }
            let _ = db.delete_empty_profile(&profile.id);
            fs::remove_dir_all(&profile_root)
                .map_err(|e| format!("Could not clean QA profile {}: {e}", profile.id))?;
            if roots_have_natives(&root, &profile.id) {
                eprintln!(
                    "[real-provider-qa] cleanup left empty/generated natives for {}",
                    profile.id
                );
            }
        }
    }

    let (
        vanilla,
        fabric,
        quilt,
        mods,
        auth_ready,
        vanilla_started,
        fabric_started,
        quilt_started,
        sodium_loaded,
    ) = result?;
    let modrinth_versions = mods
        .iter()
        .map(|item| format!("{}:{}:{}", item.project_id, item.version_id, item.filename))
        .collect::<Vec<_>>()
        .join(";");
    let modrinth_hashes_present = mods
        .iter()
        .all(|item| item.managed && !item.hash.is_empty());
    println!(
        "[real-provider-qa] vanilla=PASS mc=1.21.11 main={} downloaded_bytes={} fabric=PASS version={} main={} downloaded_bytes={} quilt=PASS version={} main={} downloaded_bytes={} modrinth_sodium_artifacts={} modrinth_versions={} hashes_present={} auth_available={} vanilla_game_marker={:?} fabric_game_marker={:?} quilt_game_marker={:?} sodium_log_marker={:?}",
        vanilla.main_class,
        vanilla.bytes_downloaded,
        fabric.loader_version,
        fabric.main_class,
        fabric.bytes_downloaded,
        quilt.loader_version,
        quilt.main_class,
        quilt.bytes_downloaded,
        mods.len(),
        modrinth_versions,
        modrinth_hashes_present,
        auth_ready,
        vanilla_started,
        fabric_started,
        quilt_started,
        sodium_loaded,
    );
    Ok(())
}

fn roots_have_natives(root: &Path, profile_id: &str) -> bool {
    crate::minecraft::managed_data_roots(root)
        .unwrap_or_default()
        .into_iter()
        .any(|base| base.join("minecraft/natives").join(profile_id).exists())
}

async fn launch_and_stop<R: Runtime>(
    service: &Arc<MinecraftService>,
    app: AppHandle<R>,
    auth: Arc<AuthService>,
    profile_id: &str,
    log_path: &Path,
    expected_marker: &str,
    running: &mut Vec<u32>,
) -> Result<bool, String> {
    let state = service
        .launch_profile(app, auth, profile_id.to_owned())
        .await?;
    let pid = state.pid.ok_or("Java process did not return its PID")?;
    running.push(pid);
    let mut marker_seen = false;
    for _ in 0..150 {
        if fs::read_to_string(log_path).is_ok_and(|contents| contents.contains(expected_marker)) {
            marker_seen = true;
            break;
        }
        if service.launch_state(profile_id)?.state != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let _ = tokio::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .await;
    for _ in 0..20 {
        if service.launch_state(profile_id)?.state != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    running.retain(|running_pid| *running_pid != pid);
    Ok(marker_seen)
}
