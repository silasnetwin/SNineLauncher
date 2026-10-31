import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export type LoaderType = "vanilla" | "fabric" | "neoforge" | "forge" | "quilt";
export type Profile = { id: string; name: string; groupName?: string; clientVersion: string; minecraftVersion: string; installed: boolean; clientReady: boolean; selected: boolean; favorite: boolean; hidden: boolean; loaderType: LoaderType; loaderVersion: string | null; snineclientEnabled: boolean; snineclientVersion?: string | null; iconId: string | null; memoryMb: number | null };
export type MinecraftVersionInfo = { id: string; versionType: string; releaseTime: string; url: string };
export type BlockIconEntry = { id: string; label: string; kind: "block" | "item" | "block_and_item"; sourceVersion: string; previewDataUrl: string | null };
export type ActivityEntry = { id: string; kind: string; title: string; detail: string; profileId: string | null; occurredAt: number };
export type Bootstrap = { profiles: Profile[]; selectedProfileId: string | null; recentProfileIds: string[]; activity: ActivityEntry[]; schemaVersion: number };
export type DownloadProgress = { id: string; fileName: string; state: string; bytesReceived: number; totalBytes: number | null; bytesPerSecond: number; etaSeconds: number | null; error: string | null; profileId: string | null };
export type RuntimeStatus = { requiredMajor: number; compatible: { executable: string; major: number | null; architecture: string | null; valid: boolean; compatible: boolean; detail: string } | null; candidates: Array<{ executable: string; major: number | null; architecture: string | null; valid: boolean; compatible: boolean; detail: string }>; managed: boolean };
export type SafeAccount = { uuid: string; username: string; lastAuthenticatedAt: number };
export type AccountCosmetics = { username: string; skins: Array<{ id: string; state: string; url: string; variant: string }>; capes: Array<{ id: string; state: string; url: string; alias: string }> };
export type ClientCosmeticsSnapshot = { ownedCosmetics: string[]; equippedCosmetics: Record<string, string>; cosmetics: Array<{ id: string; kind: string; name: string; description: string; rarity: string; previewAsset: string }> };
export type Friend = { uuid: string; name: string; customNickname: string; note: string; online: boolean; lastSeen: number; status: string; favorite: boolean; unreadMessages: number };
export type FriendRequest = { uuid: string; name: string; createdAt: number };
export type FriendsSnapshot = { ok: boolean; friends: Friend[]; incomingRequests: FriendRequest[]; outgoingRequests: FriendRequest[] };
export type FriendMessage = { messageId: number; senderUuid: string; receiverUuid: string; senderName: string; message: string; sentAt: number; read: boolean };
export type AuthState = { status: "loggedOut" | "authorizing" | "refreshing" | "loggedIn" | "reloginRequired" | string; account: SafeAccount | null; accounts: SafeAccount[]; persistenceWarning?: string | null };
export type DeviceLoginPrompt = { userCode: string; verificationUri: string; expiresAtUnix: number; intervalSeconds: number };
export type MinecraftInstallResult = { profileId: string; minecraftVersion: string; loaderVersion: string; mainClass: string; libraryCount: number; assetCount: number; bytesDownloaded: number; ready: boolean };
export type LaunchState = { profileId: string; state: string; pid: number | null; message: string | null };
export type LaunchVerification = { profileId: string; fabricLoaded: boolean; loaderType?: LoaderType; snineClientLoaded: boolean; snineClientExpected: boolean; loaderVersion: string };
export type ProfileState = { profileId: string; state: "NotInstalled" | "Installing" | "Installed" | "UpdateAvailable" | "RepairRequired" | "Launching" | "Running" | "Failed" | string; runtimeReady: boolean; minecraftVersion: string | null; loaderVersion: string | null; message: string | null };
export type ProfileManageInfo = { profileId: string; installed: boolean; minecraftVersion: string | null; loaderVersion: string | null; clientVersion: string | null; runtime: string | null; directory: string; storageBytes: number };
export type ProfileUpdateResult = { updateAvailable: boolean; check: { available: boolean; comparable: boolean; currentVersion: string | null; latestVersion: string }; install: MinecraftInstallResult | null };
export type StorageInfo = { profileInstancesBytes: number; librariesBytes: number; assetsBytes: number; runtimesBytes: number; downloadCacheBytes: number };
export type MinecraftWindowSettings = { width: number; height: number; fullscreen: boolean };
export type ModrinthProject = { projectId: string; slug: string; title: string; description: string; author: string; iconUrl: string; downloads: number; categories: string[]; gameVersions: string[] };
export type ModrinthProjectIcon = { projectId: string; slug: string; iconUrl: string };
export type ManagedMod = { profileId: string; modId: string; projectId: string; slug: string; versionId: string; filename: string; hash: string; enabled: boolean; managed: boolean; iconDataUrl?: string | null };
export type ModUpdateStatus = { projectId: string; slug: string; currentVersionId: string; latestVersionId: string; updateAvailable: boolean };
export type ResourcePackInfo = { filename: string; name: string; description: string; packFormat: number | null; bytes: number };
export type ProfileContentItem = { name: string; directory: boolean; bytes: number };
export type ProfileLogTail = { fileName: string; content: string; bytes: number; truncated: boolean };
export type ProfileContentSection = "shaderpacks" | "worlds" | "screenshots" | "logs" | "datapacks";
export type ProfileSymlink = { name: string; directory: boolean; targetExists: boolean };
const isTauri = () => "__TAURI_INTERNALS__" in window;

export const ipc = {
  bootstrap: (): Promise<Bootstrap> => isTauri()
    ? invoke("bootstrap")
    : import.meta.env.DEV
      ? Promise.resolve({ profiles: [{ id: "demo", name: "SNineClient", groupName: "", clientVersion: "1.1.5", minecraftVersion: "1.21.11", installed: false, clientReady: false, selected: true, favorite: false, hidden: false, loaderType: "fabric", loaderVersion: null, snineclientEnabled: true, snineclientVersion: "1.1.5", iconId: null, memoryMb: null }], selectedProfileId: "demo", recentProfileIds: ["demo"], activity: [], schemaVersion: 9 })
      : Promise.resolve({ profiles: [], selectedProfileId: null, recentProfileIds: [], activity: [], schemaVersion: 5 }),
  getMinecraftMemory: (): Promise<number> => invoke("get_minecraft_memory_setting"),
  setMinecraftMemory: (megabytes: number): Promise<void> => invoke("set_minecraft_memory_setting", { megabytes }),
  getMinecraftWindowSettings: (): Promise<MinecraftWindowSettings> => invoke("get_minecraft_window_settings"),
  setMinecraftWindowSettings: (settings: MinecraftWindowSettings): Promise<void> => invoke("set_minecraft_window_settings", { settings }),
  getDownloadConcurrency: (): Promise<number> => invoke("get_download_concurrency_setting"),
  setDownloadConcurrency: (value: number): Promise<void> => invoke("set_download_concurrency_setting", { value }),
  getMinimizeOnGameStart: (): Promise<boolean> => invoke("get_minimize_on_game_start_setting"),
  setMinimizeOnGameStart: (minimize: boolean): Promise<void> => invoke("set_minimize_on_game_start_setting", { minimize }),
  getStorageInfo: (): Promise<StorageInfo> => invoke("get_storage_info"),
  createProfile: (config: { name: string; minecraftVersion: string; loaderType: LoaderType; loaderVersion?: string | null; snineclientEnabled: boolean; iconId?: string | null; memoryMb?: number | null }): Promise<Profile> => invoke("create_profile", { config }),
  getMinecraftVersions: (includeSnapshots = false, refresh = false): Promise<MinecraftVersionInfo[]> => invoke("get_minecraft_versions", { includeSnapshots, refresh }),
  getFabricLoaderVersions: (minecraftVersion: string): Promise<string[]> => invoke("get_fabric_loader_versions", { minecraftVersion }),
  getQuiltLoaderVersions: (minecraftVersion: string): Promise<string[]> => invoke("get_quilt_loader_versions", { minecraftVersion }),
  getSupportedLoaderTypes: (minecraftVersion: string): Promise<LoaderType[]> => invoke("get_supported_loader_types", { minecraftVersion }),
  getBlockIconCatalog: (minecraftVersion: string, query = "", offset = 0, limit = 64): Promise<BlockIconEntry[]> => invoke("get_block_icon_catalog", { minecraftVersion, query, offset, limit }),
  renameProfile: (id: string, name: string): Promise<Profile> => invoke("rename_profile", { id, name }),
  duplicateProfile: (id: string, name: string): Promise<Profile> => invoke("duplicate_profile", { id, name }),
  deleteEmptyProfile: (id: string): Promise<void> => invoke("delete_empty_profile", { id }),
  setProfileIcon: (id: string, iconId: string | null): Promise<void> => invoke("set_profile_icon", { id, iconId }),
  getProfileMemory: (id: string): Promise<number | null> => invoke("get_profile_memory", { id }),
  setProfileMemory: (id: string, megabytes: number | null): Promise<void> => invoke("set_profile_memory", { id, megabytes }),
  setProfileGroup: (id: string, groupName: string): Promise<void> => invoke("set_profile_group", { id, groupName }),
  searchModrinthFabricMods: (query: string, minecraftVersion: string): Promise<ModrinthProject[]> => invoke("search_modrinth_fabric_mods", { query, minecraftVersion }),
  getModrinthFabricRecommendations: (minecraftVersion: string): Promise<ModrinthProject[]> => invoke("search_modrinth_fabric_mods", { query: "", minecraftVersion }),
  getModrinthProjectIcons: (projectIds: string[]): Promise<ModrinthProjectIcon[]> => invoke("get_modrinth_project_icons", { projectIds }),
  searchModrinthResourcePacks: (query: string, minecraftVersion: string): Promise<ModrinthProject[]> => invoke("search_modrinth_resource_packs", { query, minecraftVersion }),
  getManagedMods: (profileId: string): Promise<ManagedMod[]> => invoke("get_profile_mods", { profileId }),
  installModrinthMod: (profileId: string, projectId: string, projectSlug = projectId): Promise<ManagedMod[]> => invoke("install_modrinth_mod", { profileId, projectId, projectSlug }),
  checkModUpdates: (profileId: string): Promise<ModUpdateStatus[]> => invoke("check_mod_updates", { profileId }),
  setModEnabled: (profileId: string, filename: string, enabled: boolean): Promise<void> => invoke("set_profile_mod_enabled", { profileId, filename, enabled }),
  removeManagedMod: (profileId: string, filename: string): Promise<void> => invoke("remove_profile_mod", { profileId, filename }),
  getProfileResourcePacks: (profileId: string): Promise<ResourcePackInfo[]> => invoke("get_profile_resource_packs", { profileId }),
  importProfileResourcePack: (profileId: string, sourcePath: string): Promise<ResourcePackInfo> => invoke("import_profile_resource_pack", { profileId, sourcePath }),
  selectResourcePackFile: async (): Promise<string | null> => {
    const selected = await open({ multiple: false, filters: [{ name: "Minecraft resource pack", extensions: ["zip"] }] });
    return typeof selected === "string" ? selected : null;
  },
  installModrinthResourcePack: (profileId: string, projectId: string): Promise<ResourcePackInfo> => invoke("install_modrinth_resource_pack", { profileId, projectId }),
  removeProfileResourcePack: (profileId: string, filename: string): Promise<void> => invoke("remove_profile_resource_pack", { profileId, filename }),
  openProfileResourcePacks: (profileId: string): Promise<void> => invoke("open_profile_resource_packs", { profileId }),
  getProfileContent: (profileId: string, section: ProfileContentSection, world?: string): Promise<ProfileContentItem[]> => invoke("get_profile_content", { profileId, section, world: world ?? null }),
  openProfileContentFolder: (profileId: string, section: ProfileContentSection, world?: string): Promise<void> => invoke("open_profile_content_folder", { profileId, section, world: world ?? null }),
  openProfileContentItem: (profileId: string, section: ProfileContentSection, name: string, world?: string): Promise<void> => invoke("open_profile_content_item", { profileId, section, name, world: world ?? null }),
  importProfileContent: (profileId: string, section: ProfileContentSection, sourcePath: string, world?: string): Promise<ProfileContentItem> => invoke("import_profile_content", { profileId, section, sourcePath, world: world ?? null }),
  removeProfileContent: (profileId: string, section: ProfileContentSection, name: string, world?: string): Promise<void> => invoke("remove_profile_content", { profileId, section, name, world: world ?? null }),
  selectProfileContentFile: async (section: "shaderpacks" | "datapacks"): Promise<string | null> => {
    const selected = await open({ multiple: false, filters: [{ name: section === "shaderpacks" ? "Minecraft shader pack" : "Minecraft data pack", extensions: section === "shaderpacks" ? ["zip", "jar"] : ["zip"] }] });
    return typeof selected === "string" ? selected : null;
  },
  getProfileSymlinks: (profileId: string): Promise<ProfileSymlink[]> => invoke("get_profile_symlinks", { profileId }),
  createProfileSymlink: (profileId: string, sourcePath: string, linkName: string): Promise<ProfileSymlink> => invoke("create_profile_symlink", { profileId, sourcePath, linkName }),
  removeProfileSymlink: (profileId: string, linkName: string): Promise<void> => invoke("remove_profile_symlink", { profileId, linkName }),
  selectSymlinkSource: async (directory: boolean): Promise<string | null> => {
    const selected = await open({ multiple: false, directory });
    return typeof selected === "string" ? selected : null;
  },
  selectProfile: (id: string): Promise<void> => invoke("set_selected_profile", { id }),
  setFavorite: (id: string, favorite: boolean): Promise<void> => invoke("set_profile_favorite", { id, favorite }),
  setHidden: (id: string, hidden: boolean): Promise<void> => invoke("set_profile_hidden", { id, hidden }),
  enqueueDownload: (url: string, fileName: string, expectedSha256?: string): Promise<string> => invoke("enqueue_download", { url, fileName, expectedSha256 }),
  cancelDownload: (id: string): Promise<boolean> => invoke("cancel_download", { id }),
  retryDownload: (id: string): Promise<string> => invoke("retry_download", { id }),
  installClient: (profileId: string): Promise<{ profileId: string; clientVersion: string; minecraftVersion: string; bytes: number; sha256: string }> => invoke("install_profile_client", { profileId }),
  detectRuntime: (requiredMajor: number): Promise<RuntimeStatus> => invoke("detect_runtime", { requiredMajor }),
  installManagedJava: (requiredMajor: number): Promise<{ executable: string; major: number; version: string; bytes: number; sha256: string }> => invoke("install_managed_java", { requiredMajor }),
  installMinecraftProfile: (profileId: string): Promise<MinecraftInstallResult> => invoke("install_minecraft_profile", { profileId }),
  launchMinecraftProfile: (profileId: string): Promise<LaunchState> => invoke("launch_minecraft_profile", { profileId }),
  waitForGameWindow: (pid: number): Promise<boolean> => invoke("wait_for_game_window", { pid }),
  getProfileLaunchState: (profileId: string): Promise<LaunchState> => invoke("get_profile_launch_state", { profileId }),
  resolveProfileState: (profileId: string): Promise<ProfileState> => invoke("resolve_profile_state", { profileId }),
  getProfileManageInfo: (profileId: string): Promise<ProfileManageInfo> => invoke("get_profile_manage_info", { profileId }),
  repairMinecraftProfile: (profileId: string): Promise<MinecraftInstallResult> => invoke("repair_minecraft_profile", { profileId }),
  checkProfileUpdate: (profileId: string): Promise<{ available: boolean; comparable: boolean; currentVersion: string | null; latestVersion: string }> => invoke("check_profile_update", { profileId }),
  updateMinecraftProfile: (profileId: string): Promise<ProfileUpdateResult> => invoke("update_minecraft_profile", { profileId }),
  uninstallMinecraftProfile: (profileId: string): Promise<void> => invoke("uninstall_minecraft_profile", { profileId }),
  openMinecraftProfileFolder: (profileId: string): Promise<void> => invoke("open_minecraft_profile_folder", { profileId }),
  chooseProfileBackupDestination: (fileName: string): Promise<string | null> => save({ defaultPath: fileName, filters: [{ name: "SNine Profile Backup", extensions: ["zip"] }] }),
  chooseProfileBackupSource: async (): Promise<string | null> => {
    const selected = await open({ multiple: false, directory: false, filters: [{ name: "SNine Profile Backup", extensions: ["zip"] }] });
    return typeof selected === "string" ? selected : null;
  },
  createProfileBackup: (profileId: string, destination: string): Promise<number> => invoke("create_profile_backup", { profileId, destination }),
  restoreProfileBackup: (profileId: string, source: string): Promise<number> => invoke("restore_profile_backup", { profileId, source }),
  openMinecraftProfileLogs: (profileId: string): Promise<void> => invoke("open_minecraft_profile_logs", { profileId }),
  readMinecraftProfileLog: (profileId: string, fileName: string): Promise<ProfileLogTail> => invoke("read_minecraft_profile_log", { profileId, fileName }),
  authState: (): Promise<AuthState> => invoke("auth_get_state"),
  authGetSkinTexture: (uuid: string): Promise<string | null> => invoke("auth_get_skin_texture", { uuid }),
  authSelectAccount: (accountUuid: string): Promise<AuthState> => invoke("auth_select_account", { accountUuid }),
  authGetAccountCosmetics: (): Promise<AccountCosmetics> => invoke("auth_get_account_cosmetics"),
  authActivateSkin: (skinId: string): Promise<void> => invoke("auth_activate_skin", { skinId }),
  authUploadSkin: (path: string, variant: "classic" | "slim"): Promise<void> => invoke("auth_upload_skin", { path, variant }),
  chooseSkinFile: async (): Promise<string | null> => {
    const selected = await open({ multiple: false, filters: [{ name: "Minecraft skin PNG", extensions: ["png"] }] });
    return typeof selected === "string" ? selected : null;
  },
  authActivateCape: (capeId: string): Promise<void> => invoke("auth_activate_cape", { capeId }),
  authHideCape: (): Promise<void> => invoke("auth_hide_cape"),
  authBeginSignIn: (): Promise<DeviceLoginPrompt> => invoke("auth_begin_sign_in"),
  authCancelSignIn: (): Promise<void> => invoke("auth_cancel_sign_in"),
  authRestore: (): Promise<AuthState> => invoke("auth_restore"),
  authSignOut: (): Promise<AuthState> => invoke("auth_sign_out"),
  authRequireValidMinecraftSession: (): Promise<SafeAccount> => invoke("auth_require_valid_minecraft_session"),
  getFriends: (): Promise<FriendsSnapshot> => invoke("social_get_friends"),
  getClientCosmetics: (): Promise<ClientCosmeticsSnapshot> => invoke("social_get_client_cosmetics"),
  getFriendMessages: (friendUuid: string): Promise<FriendMessage[]> => invoke("social_get_messages", { friendUuid }),
  sendFriendMessage: (friendUuid: string, message: string): Promise<FriendMessage> => invoke("social_send_message", { friendUuid, message }),
  addFriend: (targetName: string): Promise<FriendsSnapshot> => invoke("social_add_friend", { targetName }),
  respondToFriendRequest: (requesterUuid: string, accept: boolean): Promise<FriendsSnapshot> => invoke("social_respond_to_request", { requesterUuid, accept }),
  cancelFriendRequest: (friendUuid: string): Promise<FriendsSnapshot> => invoke("social_cancel_friend_request", { friendUuid }),
  removeFriend: (friendUuid: string): Promise<FriendsSnapshot> => invoke("social_remove_friend", { friendUuid }),
  authRetryPersistSession: (): Promise<AuthState> => invoke("auth_retry_persist_session"),
};
