use crate::auth::{AccountCosmetics, AuthService, AuthState, DeviceLoginPrompt};
use crate::database::{
    Bootstrap, Database, MinecraftWindowSettings, NewProfileConfig, Profile, ProfileFlag,
};
use crate::downloads::{DownloadRequest, DownloadService};
use crate::install::{ClientUpdateCheck, InstallResult, Installer};
use crate::minecraft::{
    LaunchState, MinecraftInstallResult, MinecraftService, ProfileLogTail, ProfileManageInfo,
    ProfileState, StorageInfo,
};
use crate::modrinth::{ModrinthProject, ModrinthService};
use crate::runtime::{ManagedRuntimeResult, RuntimeService, RuntimeStatus};
use crate::social::{FriendMessage, FriendsSnapshot, SocialService};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tauri::{AppHandle, Emitter, Manager};

#[tauri::command]
pub async fn bootstrap(db: State<'_, Arc<Database>>) -> Result<Bootstrap, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.bootstrap())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_minecraft_memory_setting(db: State<'_, Arc<Database>>) -> Result<u32, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.minecraft_memory_mb())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_minecraft_memory_setting(
    megabytes: u32,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_minecraft_memory_mb(megabytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_minecraft_window_settings(
    db: State<'_, Arc<Database>>,
) -> Result<MinecraftWindowSettings, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.minecraft_window_settings())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_minecraft_window_settings(
    settings: MinecraftWindowSettings,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_minecraft_window_settings(settings))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_download_concurrency_setting(db: State<'_, Arc<Database>>) -> Result<u32, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.download_concurrency())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_download_concurrency_setting(
    value: u32,
    db: State<'_, Arc<Database>>,
    downloads: State<'_, Arc<DownloadService>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_download_concurrency(value))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    downloads.set_concurrency(value as usize)
}

#[tauri::command]
pub async fn get_minimize_on_game_start_setting(
    db: State<'_, Arc<Database>>,
) -> Result<bool, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.minimize_on_game_start())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_minimize_on_game_start_setting(
    minimize: bool,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_minimize_on_game_start(minimize))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_storage_info(
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<StorageInfo, String> {
    let service = Arc::clone(&minecraft);
    tokio::task::spawn_blocking(move || service.storage_info())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_profile(
    app: AppHandle,
    config: NewProfileConfig,
    db: State<'_, Arc<Database>>,
) -> Result<Profile, String> {
    let database = Arc::clone(&db);
    let created = tokio::task::spawn_blocking(move || database.create_profile_config(config))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not resolve launcher data folder: {e}"))?;
    if let Err(error) = crate::minecraft::ensure_profile_root(&root, &created.id) {
        let _ = db.delete_empty_profile(&created.id);
        return Err(error);
    }
    Ok(created)
}

#[tauri::command]
pub async fn get_minecraft_versions(
    include_snapshots: bool,
    refresh: bool,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::minecraft::MinecraftVersionInfo>, String> {
    minecraft
        .minecraft_versions(include_snapshots, refresh)
        .await
}

#[tauri::command]
pub async fn get_fabric_loader_versions(
    minecraft_version: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<String>, String> {
    minecraft.fabric_loader_versions(&minecraft_version).await
}

#[tauri::command]
pub async fn get_quilt_loader_versions(
    minecraft_version: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<String>, String> {
    minecraft.quilt_loader_versions(&minecraft_version).await
}

#[tauri::command]
pub async fn get_supported_loader_types(
    minecraft_version: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::database::LoaderType>, String> {
    minecraft.supported_loader_types(&minecraft_version).await
}

#[tauri::command]
pub async fn get_block_icon_catalog(
    minecraft_version: String,
    query: String,
    offset: usize,
    limit: usize,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::minecraft::BlockIconEntry>, String> {
    if minecraft_version != "latest" {
        return Err("Minecraft profile icons always use the latest release resource pack".into());
    }
    minecraft
        .latest_block_icon_catalog(&query, offset, limit)
        .await
}

#[tauri::command]
pub async fn rename_profile(
    id: String,
    name: String,
    db: State<'_, Arc<Database>>,
) -> Result<Profile, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.rename_profile(&id, name))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn duplicate_profile(
    id: String,
    name: String,
    db: State<'_, Arc<Database>>,
) -> Result<Profile, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.duplicate_profile(&id, name))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_empty_profile(id: String, db: State<'_, Arc<Database>>) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.delete_empty_profile(&id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_profile_icon(
    id: String,
    icon_id: Option<String>,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_profile_icon(&id, icon_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_profile_memory(
    id: String,
    db: State<'_, Arc<Database>>,
) -> Result<Option<u32>, String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.profile_memory_mb(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_profile_memory(
    id: String,
    megabytes: Option<u32>,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_profile_memory_mb(&id, megabytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_profile_group(
    id: String,
    group_name: String,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_profile_group(&id, group_name))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_modrinth_fabric_mods(
    query: String,
    minecraft_version: String,
    modrinth: State<'_, Arc<ModrinthService>>,
) -> Result<Vec<ModrinthProject>, String> {
    modrinth
        .search_fabric_mods(&query, &minecraft_version)
        .await
}

#[tauri::command]
pub async fn get_modrinth_project_icons(
    project_ids: Vec<String>,
    modrinth: State<'_, Arc<ModrinthService>>,
) -> Result<Vec<crate::modrinth::ModrinthProjectIcon>, String> {
    modrinth.project_icons(&project_ids).await
}

#[tauri::command]
pub async fn search_modrinth_resource_packs(
    query: String,
    minecraft_version: String,
    modrinth: State<'_, Arc<ModrinthService>>,
) -> Result<Vec<ModrinthProject>, String> {
    modrinth
        .search_resource_packs(&query, &minecraft_version)
        .await
}

#[tauri::command]
pub fn get_profile_mods(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::database::ManagedMod>, String> {
    minecraft.profile_mods(&profile_id)
}

#[tauri::command]
pub async fn install_modrinth_mod(
    app: AppHandle,
    profile_id: String,
    project_id: String,
    project_slug: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::database::ManagedMod>, String> {
    minecraft
        .install_modrinth_project(&app, &profile_id, &project_id, &project_slug)
        .await
}

#[tauri::command]
pub async fn check_mod_updates(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::minecraft::ModUpdateStatus>, String> {
    minecraft.mod_update_status(&profile_id).await
}

#[tauri::command]
pub fn set_profile_mod_enabled(
    profile_id: String,
    filename: String,
    enabled: bool,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.set_managed_mod_enabled(&profile_id, &filename, enabled)
}

#[tauri::command]
pub fn remove_profile_mod(
    profile_id: String,
    filename: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.remove_managed_mod(&profile_id, &filename)
}

#[tauri::command]
pub fn get_profile_resource_packs(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::minecraft::ResourcePackInfo>, String> {
    minecraft.profile_resource_packs(&profile_id)
}

#[tauri::command]
pub fn import_profile_resource_pack(
    profile_id: String,
    source_path: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<crate::minecraft::ResourcePackInfo, String> {
    minecraft.import_profile_resource_pack(&profile_id, &source_path)
}

#[tauri::command]
pub async fn install_modrinth_resource_pack(
    app: AppHandle,
    profile_id: String,
    project_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<crate::minecraft::ResourcePackInfo, String> {
    minecraft
        .install_modrinth_resource_pack(&app, &profile_id, &project_id)
        .await
}

#[tauri::command]
pub fn remove_profile_resource_pack(
    profile_id: String,
    filename: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.remove_profile_resource_pack(&profile_id, &filename)
}

#[tauri::command]
pub fn open_profile_resource_packs(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.open_profile_resource_packs(&profile_id)
}

#[tauri::command]
pub fn get_profile_content(
    profile_id: String,
    section: String,
    world: Option<String>,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::minecraft::ProfileContentItem>, String> {
    minecraft.profile_content_items(&profile_id, &section, world.as_deref())
}

#[tauri::command]
pub fn open_profile_content_folder(
    profile_id: String,
    section: String,
    world: Option<String>,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.open_profile_content_folder(&profile_id, &section, world.as_deref())
}

#[tauri::command]
pub fn open_profile_content_item(
    profile_id: String,
    section: String,
    world: Option<String>,
    name: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.open_profile_content_item(&profile_id, &section, world.as_deref(), &name)
}

#[tauri::command]
pub fn import_profile_content(
    profile_id: String,
    section: String,
    world: Option<String>,
    source_path: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<crate::minecraft::ProfileContentItem, String> {
    minecraft.import_profile_content(&profile_id, &section, world.as_deref(), &source_path)
}

#[tauri::command]
pub fn remove_profile_content(
    profile_id: String,
    section: String,
    world: Option<String>,
    name: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.remove_profile_content(&profile_id, &section, world.as_deref(), &name)
}

#[tauri::command]
pub fn get_profile_symlinks(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<Vec<crate::minecraft::ProfileSymlink>, String> {
    minecraft.profile_symlinks(&profile_id)
}

#[tauri::command]
pub fn create_profile_symlink(
    profile_id: String,
    source_path: String,
    link_name: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<crate::minecraft::ProfileSymlink, String> {
    minecraft.create_profile_symlink(&profile_id, &source_path, &link_name)
}

#[tauri::command]
pub fn remove_profile_symlink(
    profile_id: String,
    link_name: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.remove_profile_symlink(&profile_id, &link_name)
}

#[tauri::command]
pub async fn set_selected_profile(id: String, db: State<'_, Arc<Database>>) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.select_profile(id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_profile_favorite(
    id: String,
    favorite: bool,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || {
        database.set_profile_flag(id, ProfileFlag::Favorite, favorite)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_profile_hidden(
    id: String,
    hidden: bool,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let database = Arc::clone(&db);
    tokio::task::spawn_blocking(move || database.set_profile_flag(id, ProfileFlag::Hidden, hidden))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn enqueue_download(
    app: AppHandle,
    downloads: State<'_, Arc<DownloadService>>,
    url: String,
    file_name: String,
    expected_sha256: Option<String>,
) -> Result<String, String> {
    Arc::clone(&downloads).enqueue(
        app,
        DownloadRequest {
            url,
            file_name,
            expected_sha256,
            profile_id: None,
            max_bytes: None,
        },
    )
}

#[tauri::command]
pub async fn cancel_download(
    id: String,
    downloads: State<'_, Arc<DownloadService>>,
) -> Result<bool, String> {
    downloads.cancel(&id)
}

#[tauri::command]
pub fn retry_download(
    app: AppHandle,
    id: String,
    downloads: State<'_, Arc<DownloadService>>,
) -> Result<String, String> {
    Arc::clone(&downloads).retry(app, &id)
}

#[tauri::command]
pub async fn install_profile_client(
    app: AppHandle,
    profile_id: String,
    installer: State<'_, Installer>,
) -> Result<InstallResult, String> {
    installer.install_client(&app, profile_id).await
}

#[tauri::command]
pub async fn detect_runtime(
    required_major: u32,
    runtime: State<'_, Arc<RuntimeService>>,
) -> Result<RuntimeStatus, String> {
    let runtime = Arc::clone(&runtime);
    tokio::task::spawn_blocking(move || runtime.detect(required_major))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_managed_java(
    app: AppHandle,
    required_major: u32,
    runtime: State<'_, Arc<RuntimeService>>,
) -> Result<ManagedRuntimeResult, String> {
    Arc::clone(&runtime)
        .install_managed_java(&app, required_major)
        .await
}

#[tauri::command]
pub async fn auth_get_state(auth: State<'_, Arc<AuthService>>) -> Result<AuthState, String> {
    auth.state()
}

#[tauri::command]
pub async fn auth_get_skin_texture(
    uuid: String,
    auth: State<'_, Arc<AuthService>>,
) -> Result<Option<String>, String> {
    Ok(auth.skin_texture_for_uuid(&uuid).await)
}

#[tauri::command]
pub async fn auth_select_account(
    app: AppHandle,
    account_uuid: String,
    auth: State<'_, Arc<AuthService>>,
) -> Result<AuthState, String> {
    auth.select_account(&app, &account_uuid).await
}

#[tauri::command]
pub async fn auth_get_account_cosmetics(
    auth: State<'_, Arc<AuthService>>,
) -> Result<AccountCosmetics, String> {
    auth.account_cosmetics().await
}

#[tauri::command]
pub async fn auth_activate_skin(
    skin_id: String,
    auth: State<'_, Arc<AuthService>>,
) -> Result<(), String> {
    auth.activate_skin(&skin_id).await
}

#[tauri::command]
pub async fn auth_upload_skin(
    path: String,
    variant: String,
    auth: State<'_, Arc<AuthService>>,
) -> Result<(), String> {
    auth.upload_skin(PathBuf::from(path), &variant).await
}

#[tauri::command]
pub async fn auth_activate_cape(
    cape_id: String,
    auth: State<'_, Arc<AuthService>>,
) -> Result<(), String> {
    auth.activate_cape(&cape_id).await
}

#[tauri::command]
pub async fn auth_hide_cape(auth: State<'_, Arc<AuthService>>) -> Result<(), String> {
    auth.hide_cape().await
}

#[tauri::command]
pub async fn auth_begin_sign_in(
    app: AppHandle,
    auth: State<'_, Arc<AuthService>>,
) -> Result<DeviceLoginPrompt, String> {
    Arc::clone(&auth).begin_sign_in(app).await
}

#[tauri::command]
pub async fn auth_cancel_sign_in(auth: State<'_, Arc<AuthService>>) -> Result<(), String> {
    auth.cancel_sign_in()
}

#[tauri::command]
pub async fn auth_restore(
    app: AppHandle,
    auth: State<'_, Arc<AuthService>>,
) -> Result<AuthState, String> {
    auth.restore(&app).await
}

#[tauri::command]
pub async fn auth_sign_out(
    app: AppHandle,
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<AuthState, String> {
    let result = auth.sign_out(&app).await?;
    social.clear_session().await;
    Ok(result)
}

#[tauri::command]
pub async fn social_get_friends(
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<FriendsSnapshot, String> {
    social.friends(&auth).await
}

#[tauri::command]
pub async fn social_get_messages(
    friend_uuid: String,
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<Vec<FriendMessage>, String> {
    social.messages(&auth, &friend_uuid).await
}

#[tauri::command]
pub async fn social_send_message(
    friend_uuid: String,
    message: String,
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<FriendMessage, String> {
    social.send_message(&auth, &friend_uuid, message).await
}

#[tauri::command]
pub async fn social_add_friend(
    target_name: String,
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<FriendsSnapshot, String> {
    social.add_friend(&auth, target_name).await
}

#[tauri::command]
pub async fn social_respond_to_request(
    requester_uuid: String,
    accept: bool,
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<FriendsSnapshot, String> {
    social
        .respond_to_request(&auth, &requester_uuid, accept)
        .await
}

#[tauri::command]
pub async fn social_remove_friend(
    friend_uuid: String,
    auth: State<'_, Arc<AuthService>>,
    social: State<'_, Arc<SocialService>>,
) -> Result<FriendsSnapshot, String> {
    social.remove_friend(&auth, &friend_uuid).await
}

#[tauri::command]
pub async fn auth_require_valid_minecraft_session(
    auth: State<'_, Arc<AuthService>>,
) -> Result<crate::database::SafeAccount, String> {
    let (account, mut secret_session_token, mut secret_xuid) =
        auth.require_valid_minecraft_session().await?;
    zeroize::Zeroize::zeroize(&mut secret_session_token);
    zeroize::Zeroize::zeroize(&mut secret_xuid);
    Ok(account)
}

#[tauri::command]
pub async fn auth_retry_persist_session(
    auth: State<'_, Arc<AuthService>>,
) -> Result<AuthState, String> {
    auth.retry_persist_session()
}

#[tauri::command]
pub async fn install_minecraft_profile(
    app: AppHandle,
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<MinecraftInstallResult, String> {
    minecraft.install_profile(&app, &profile_id).await
}

#[tauri::command]
pub async fn launch_minecraft_profile(
    app: AppHandle,
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
    auth: State<'_, Arc<AuthService>>,
) -> Result<LaunchState, String> {
    Arc::clone(&minecraft)
        .launch_profile(app, Arc::clone(&auth), profile_id)
        .await
}

#[tauri::command]
pub async fn get_profile_launch_state(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<LaunchState, String> {
    minecraft.launch_state(&profile_id)
}

#[tauri::command]
pub fn resolve_profile_state(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<ProfileState, String> {
    minecraft.resolve_profile_state(&profile_id)
}

#[tauri::command]
pub async fn get_profile_manage_info(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<ProfileManageInfo, String> {
    let service = Arc::clone(&minecraft);
    tokio::task::spawn_blocking(move || service.manage_info(&profile_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn repair_minecraft_profile(
    app: AppHandle,
    profile_id: String,
    installer: State<'_, Installer>,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<MinecraftInstallResult, String> {
    let _maintenance_guard = minecraft.lock_profile_maintenance(&profile_id).await?;
    let _ = app.emit("install-progress", serde_json::json!({"profileId":profile_id,"state":"repairing","detail":"Scanning managed profile files"}));
    let client_repair = installer.client_needs_repair(&profile_id).await?;
    let mut replaced_client = false;
    if client_repair {
        if let Err(error) = installer.install_client(&app, profile_id.clone()).await {
            let _ = installer.restore_previous_client(&profile_id).await;
            return Err(error);
        }
        replaced_client = true;
    }
    match minecraft.install_profile(&app, &profile_id).await {
        Ok(result) => {
            if replaced_client {
                installer.discard_previous_client(&profile_id).await?;
            }
            Ok(result)
        }
        Err(error) => {
            if replaced_client {
                let _ = installer.restore_previous_client(&profile_id).await;
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn check_profile_update(
    profile_id: String,
    installer: State<'_, Installer>,
) -> Result<ClientUpdateCheck, String> {
    installer.check_client_update(&profile_id).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub update_available: bool,
    pub check: ClientUpdateCheck,
    pub install: Option<MinecraftInstallResult>,
}

#[tauri::command]
pub async fn update_minecraft_profile(
    app: AppHandle,
    profile_id: String,
    installer: State<'_, Installer>,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<UpdateResult, String> {
    let _maintenance_guard = minecraft.lock_profile_maintenance(&profile_id).await?;
    let _ = app.emit("install-progress", serde_json::json!({"profileId":profile_id,"state":"update-checking","detail":"Checking SNine release metadata"}));
    let update = installer.check_client_update(&profile_id).await?;
    if !update.available {
        let _ = app.emit("install-progress", serde_json::json!({"profileId":profile_id,"state":"ready","detail":"No newer comparable SNine release found"}));
        return Ok(UpdateResult {
            update_available: false,
            check: update,
            install: None,
        });
    }
    let _ = app.emit("install-progress", serde_json::json!({"profileId":profile_id,"state":"updating","detail":"Updating managed SNine profile"}));
    // Existing SNine release endpoint provides the authoritative current client JAR, but
    // may not provide a semantic version header. Stage/validate through the existing installer;
    // it keeps the old JAR and metadata available until the full Minecraft/Fabric install succeeds.
    let install = installer.install_client(&app, profile_id.clone()).await;
    if let Err(error) = install {
        let _ = installer.restore_previous_client(&profile_id).await;
        return Err(error);
    }
    match minecraft.install_profile(&app, &profile_id).await {
        Ok(result) => {
            installer.discard_previous_client(&profile_id).await?;
            Ok(UpdateResult {
                update_available: true,
                check: update,
                install: Some(result),
            })
        }
        Err(error) => {
            let _ = installer.restore_previous_client(&profile_id).await;
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn uninstall_minecraft_profile(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.uninstall_profile(&profile_id).await
}

#[tauri::command]
pub async fn create_profile_backup(
    profile_id: String,
    destination: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<u64, String> {
    minecraft
        .create_profile_backup(&profile_id, std::path::PathBuf::from(destination))
        .await
}

#[tauri::command]
pub async fn restore_profile_backup(
    profile_id: String,
    source: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<u64, String> {
    minecraft
        .restore_profile_backup(&profile_id, std::path::PathBuf::from(source))
        .await
}

#[tauri::command]
pub fn open_minecraft_profile_folder(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.open_profile_folder(&profile_id)
}

#[tauri::command]
pub fn open_minecraft_profile_logs(
    profile_id: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<(), String> {
    minecraft.open_profile_logs(&profile_id)
}

#[tauri::command]
pub fn read_minecraft_profile_log(
    profile_id: String,
    file_name: String,
    minecraft: State<'_, Arc<MinecraftService>>,
) -> Result<ProfileLogTail, String> {
    minecraft.read_profile_log_tail(&profile_id, &file_name)
}
