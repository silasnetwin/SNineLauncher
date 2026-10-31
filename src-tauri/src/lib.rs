mod auth;
mod commands;
mod database;
mod downloads;
mod install;
mod minecraft;
mod modrinth;
#[cfg(debug_assertions)]
mod qa;
mod runtime;
mod social;

use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    let process_started = std::time::Instant::now();
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let root = app.path().app_data_dir()?;
            std::fs::create_dir_all(&root)?;
            std::fs::create_dir_all(root.join("cache"))?;
            std::fs::create_dir_all(root.join("logs"))?;
            std::fs::create_dir_all(root.join("downloads"))?;
            std::fs::create_dir_all(root.join("runtimes"))?;
            let db = Arc::new(database::Database::open(root.join("launcher.db"))?);
            db.ensure_default_snine_profiles()?;
            db.ensure_default_port_profiles()?;
            app.manage(Arc::clone(&db));
            let downloads = Arc::new(downloads::DownloadService::new(root.join("downloads"))?);
            downloads
                .set_concurrency(db.download_concurrency()? as usize)
                .map_err(std::io::Error::other)?;
            let runtime = Arc::new(runtime::RuntimeService::new(
                root.clone(),
                Arc::clone(&downloads),
            )?);
            let installer =
                install::Installer::new(root.clone(), Arc::clone(&downloads), Arc::clone(&db))?;
            let minecraft = Arc::new(minecraft::MinecraftService::new(
                root.clone(),
                Arc::clone(&downloads),
                Arc::clone(&runtime),
                Arc::clone(&db),
            )?);
            let auth = Arc::new(auth::AuthService::new(Arc::clone(&db), root.join("auth"))?);
            let social = Arc::new(social::SocialService::new()?);
            let modrinth = Arc::new(modrinth::ModrinthService::new()?);
            #[cfg(debug_assertions)]
            if std::env::var("SNINE_REAL_PROVIDER_QA").as_deref() == Ok("1") {
                let qa_app = app.handle().clone();
                let qa_root = root.clone();
                let qa_downloads = Arc::clone(&downloads);
                let qa_runtime = Arc::clone(&runtime);
                let qa_auth = Arc::clone(&auth);
                tauri::async_runtime::spawn(async move {
                    if let Err(error) =
                        qa::run(qa_app, qa_root, qa_downloads, qa_runtime, qa_auth).await
                    {
                        eprintln!("[real-provider-qa] failed: {error}");
                    }
                });
            }
            app.manage(auth);
            app.manage(social);
            app.manage(modrinth);
            app.manage(downloads);
            app.manage(runtime.clone());
            app.manage(installer.clone());
            app.manage(minecraft.clone());
            let pending_default_profiles = db
                .bootstrap()?
                .profiles
                .into_iter()
                .filter(|profile| {
                    matches!(profile.id.as_str(), "snineclient-default" | "snineclient-26-1-2" | "snineclient-26-3")
                        && !profile.installed
                })
                .collect::<Vec<_>>();
            if !pending_default_profiles.is_empty() {
                let app_handle = app.handle().clone();
                let installer = installer.clone();
                let minecraft = Arc::clone(&minecraft);
                let runtime = Arc::clone(&runtime);
                tauri::async_runtime::spawn(async move {
                    for profile in pending_default_profiles {
                        let java_major = if matches!(profile.minecraft_version.as_str(), "26.1.2" | "26.3") { 25 } else { 21 };
                        if runtime.detect(java_major).compatible.is_none() {
                            if matches!(java_major, 21 | 25) {
                                if let Err(error) = runtime.install_managed_java(&app_handle, java_major).await {
                                    eprintln!("[default-profile] Java {java_major} setup failed: {error}");
                                    continue;
                                }
                            } else {
                                eprintln!("[default-profile] Java {java_major} is required but managed installation is unsupported.");
                                continue;
                            }
                        }
                        if profile.snineclient_enabled && !profile.client_ready {
                            if let Err(error) = installer.install_client(&app_handle, profile.id.clone()).await {
                                eprintln!("[default-profile] SNineClient setup failed for {}: {error}", profile.minecraft_version);
                                continue;
                            }
                        }
                        if let Err(error) = minecraft.install_profile(&app_handle, &profile.id).await {
                            eprintln!("[default-profile] Minecraft setup failed for {}: {error}", profile.minecraft_version);
                        }
                    }
                });
            }
            if cfg!(debug_assertions) {
                eprintln!(
                    "[startup] native core ready: {:.1} ms",
                    process_started.elapsed().as_secs_f64() * 1000.0
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_minecraft_memory_setting,
            commands::set_minecraft_memory_setting,
            commands::get_minecraft_window_settings,
            commands::set_minecraft_window_settings,
            commands::get_download_concurrency_setting,
            commands::set_download_concurrency_setting,
            commands::get_minimize_on_game_start_setting,
            commands::set_minimize_on_game_start_setting,
            commands::get_storage_info,
            commands::create_profile,
            commands::get_minecraft_versions,
            commands::get_fabric_loader_versions,
            commands::get_quilt_loader_versions,
            commands::get_supported_loader_types,
            commands::get_block_icon_catalog,
            commands::rename_profile,
            commands::duplicate_profile,
            commands::delete_empty_profile,
            commands::set_profile_icon,
            commands::get_profile_memory,
            commands::set_profile_memory,
            commands::set_profile_group,
            commands::search_modrinth_fabric_mods,
            commands::get_modrinth_project_icons,
            commands::search_modrinth_resource_packs,
            commands::get_profile_mods,
            commands::install_modrinth_mod,
            commands::check_mod_updates,
            commands::set_profile_mod_enabled,
            commands::remove_profile_mod,
            commands::get_profile_resource_packs,
            commands::import_profile_resource_pack,
            commands::install_modrinth_resource_pack,
            commands::remove_profile_resource_pack,
            commands::open_profile_resource_packs,
            commands::get_profile_content,
            commands::open_profile_content_folder,
            commands::open_profile_content_item,
            commands::import_profile_content,
            commands::remove_profile_content,
            commands::get_profile_symlinks,
            commands::create_profile_symlink,
            commands::remove_profile_symlink,
            commands::set_selected_profile,
            commands::set_profile_favorite,
            commands::set_profile_hidden,
            commands::enqueue_download,
            commands::cancel_download,
            commands::retry_download,
            commands::install_profile_client,
            commands::detect_runtime,
            commands::install_managed_java,
            commands::auth_get_state,
            commands::auth_get_skin_texture,
            commands::auth_select_account,
            commands::auth_get_account_cosmetics,
            commands::auth_activate_skin,
            commands::auth_upload_skin,
            commands::auth_activate_cape,
            commands::auth_hide_cape,
            commands::auth_begin_sign_in,
            commands::auth_cancel_sign_in,
            commands::auth_restore,
            commands::auth_sign_out,
            commands::social_get_friends,
            commands::social_get_client_cosmetics,
            commands::wait_for_game_window,
            commands::social_get_messages,
            commands::social_send_message,
            commands::social_add_friend,
            commands::social_respond_to_request,
            commands::social_cancel_friend_request,
            commands::social_remove_friend,
            commands::auth_require_valid_minecraft_session,
            commands::auth_retry_persist_session,
            commands::install_minecraft_profile,
            commands::launch_minecraft_profile,
            commands::get_profile_launch_state,
            commands::resolve_profile_state,
            commands::get_profile_manage_info,
            commands::uninstall_minecraft_profile,
            commands::create_profile_backup,
            commands::restore_profile_backup,
            commands::open_minecraft_profile_folder,
            commands::open_minecraft_profile_logs,
            commands::read_minecraft_profile_log,
            commands::repair_minecraft_profile,
            commands::check_profile_update,
            commands::update_minecraft_profile
        ])
        .run(tauri::generate_context!())
        .expect("SNine Launcher failed to start");
}
