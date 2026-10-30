import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, ArrowRight, Check, ChevronDown, ChevronRight, CircleUserRound, Copy, Download, ExternalLink, FileText, Gamepad2, Grid2X2, History, LibraryBig, List, MessageSquareText, Minus, Pencil, Plus, RotateCcw, Search, Settings2, Sparkles, Square, Users, X, UserPlus, LoaderCircle, Trash2 } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ipc, type ActivityEntry, type AuthState, type BlockIconEntry, type DeviceLoginPrompt, type DownloadProgress, type LaunchVerification, type MinecraftVersionInfo, type Profile, type ProfileManageInfo, type RuntimeStatus, type SafeAccount, type StorageInfo, type LoaderType, type MinecraftWindowSettings } from "../lib/ipc";
import { findSelectedProfile } from "../lib/profile-utils";
import { markStartup } from "../lib/startup";
import snineLauncherIcon from "../assets/brand/snine-launcher-icon.png";
import { ProfileCard, ProfileCover } from "./ProfileCard";
import { BlockIconPicker, insertCreatedProfile, ProfileCreationForm } from "./ProfileManagementForms";
import { ProfileModsPage } from "./ProfileModsPage";
import { ProfileResourcePacksPage } from "./ProfileResourcePacksPage";
import { ProfileContentPage } from "./ProfileContentPage";
import { SocialPanel, type SocialTab } from "./SocialPanel";
import { CosmeticsPage } from "./CosmeticsPage";
import { MinecraftHead } from "./MinecraftHead";
import { SVGRepoIcon } from "./SVGRepoIcon";
import settingsGearSvg from "../assets/icons/settings-gear.svg";
import gamepadSvg from "../assets/icons/gamepad.svg";
import downloadArrowSvg from "../assets/icons/download-arrow.svg";
import databaseStorageSvg from "../assets/icons/database-storage.svg";

type AppPage = "Library" | "Cosmetics";
type LibraryTab = "Clients" | "Profiles" | "Favorites";
type SideTab = SocialTab | "Downloads";
type CornerStyle = "square" | "balanced" | "rounded";
type SettingsTab = "Appearance" | "Minecraft" | "Downloads" | "Storage";
type LibraryPreferences = { tab: LibraryTab; view: "grid" | "list"; sort: string; installedOnly: boolean; groupFilter: string; loaderFilter: string; statusFilter: string; javaFilter: string; clientFilter: string };
type LauncherCommand = { id: string; label: string; description: string; keywords: string; icon: ReactNode; run: () => void };
const RECENTS_KEY = "snine.launcher.recent-profiles.v1";
const CORNER_STYLE_KEY = "snine.launcher.corner-style.v1";
const LIBRARY_PREFS_KEY = "snine.launcher.library-preferences.v1";
const LAUNCHER_VERSION = "1.0.12";
const isTauri = () => "__TAURI_INTERNALS__" in window;
const requiredJavaMajor = (profile: Profile) => ["26.1.2", "26.2", "26.3"].includes(profile.minecraftVersion) ? 25 : 21;

export function App() {
  const [page, setPage] = useState<AppPage>("Library");
  const [history, setHistory] = useState<AppPage[]>(["Library"]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [profileViewId, setProfileViewId] = useState<string | null>(null);
  const [recentIds, setRecentIds] = useState<string[]>(() => isTauri() ? [] : readIds(RECENTS_KEY));
  const [recentMenuOpen, setRecentMenuOpen] = useState(false);
  const [libraryTab, setLibraryTab] = useState<LibraryTab>(() => readLibraryPreferences().tab);
  const [sideTab, setSideTab] = useState<SideTab>("Friends");
  const [search, setSearch] = useState("");
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [commandQuery, setCommandQuery] = useState("");
  const [commandSelection, setCommandSelection] = useState(0);
  const [view, setView] = useState<"grid" | "list">(() => readLibraryPreferences().view);
  const [viewMenuOpen, setViewMenuOpen] = useState(false);
  const [sortOpen, setSortOpen] = useState(false);
  const [sortLabel, setSortLabel] = useState(() => readLibraryPreferences().sort);
  const [installedOnly, setInstalledOnly] = useState(() => readLibraryPreferences().installedOnly);
  const [groupFilter, setGroupFilter] = useState(() => readLibraryPreferences().groupFilter);
  const [loaderFilter, setLoaderFilter] = useState(() => readLibraryPreferences().loaderFilter);
  const [statusFilter, setStatusFilter] = useState(() => readLibraryPreferences().statusFilter);
  const [javaFilter, setJavaFilter] = useState(() => readLibraryPreferences().javaFilter);
  const [clientFilter, setClientFilter] = useState(() => readLibraryPreferences().clientFilter);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("Appearance");
  const [cornerStyle, setCornerStyle] = useState<CornerStyle>(readCornerStyle);
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const [feedbackText, setFeedbackText] = useState("");
  const [brandMenuOpen, setBrandMenuOpen] = useState(false);
  const [createProfileOpen, setCreateProfileOpen] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");
  const [newProfileMemoryMb, setNewProfileMemoryMb] = useState("");
  const [newMinecraftVersion, setNewMinecraftVersion] = useState("1.21.11");
  const [minecraftVersions, setMinecraftVersions] = useState<MinecraftVersionInfo[]>([]);
  const [showSnapshots, setShowSnapshots] = useState(false);
  const [newLoaderType, setNewLoaderType] = useState<LoaderType>("fabric");
  const [newLoaderVersion, setNewLoaderVersion] = useState("");
  const [fabricLoaderVersions, setFabricLoaderVersions] = useState<string[]>([]);
  const [availableLoaderTypes, setAvailableLoaderTypes] = useState<LoaderType[]>(["vanilla", "fabric"]);
  const [newSnineEnabled, setNewSnineEnabled] = useState(true);
  const [newProfileIconId, setNewProfileIconId] = useState<string | null>(null);
  const [newProfileIconPreview, setNewProfileIconPreview] = useState<string | null>(null);
  const [iconPickerTarget, setIconPickerTarget] = useState<{ kind: "create"; version: string } | { kind: "profile"; id: string; version: string } | null>(null);
  const [minecraftMemoryMb, setMinecraftMemoryMb] = useState(4096);
  const [downloadConcurrency, setDownloadConcurrency] = useState(4);
  const [minimizeOnGameStart, setMinimizeOnGameStart] = useState(false);
  const [windowSettings, setWindowSettings] = useState<MinecraftWindowSettings>({ width: 1280, height: 720, fullscreen: false });
  const [storageInfo, setStorageInfo] = useState<StorageInfo | null>(null);
  const [isMaximized, setIsMaximized] = useState(false);
  const [toast, setToast] = useState("");
  const [launcherUpdate, setLauncherUpdate] = useState<Update | null>(null);
  const [launcherUpdateBusy, setLauncherUpdateBusy] = useState(false);
  const [launcherUpdateProgress, setLauncherUpdateProgress] = useState("");
  const [downloads, setDownloads] = useState<DownloadProgress[]>([]);
  const [downloadBatchAction, setDownloadBatchAction] = useState<"cancel" | "retry" | null>(null);
  const [runtimeStatus, setRuntimeStatus] = useState<RuntimeStatus | null>(null);
  const [authState, setAuthState] = useState<AuthState>({ status: "loggedOut", account: null, accounts: [] });
  const [loginPrompt, setLoginPrompt] = useState<DeviceLoginPrompt | null>(null);
  const [authMenuOpen, setAuthMenuOpen] = useState(false);
  const [authError, setAuthError] = useState("");
  const [authBusy, setAuthBusy] = useState(false);
  const [installingIds, setInstallingIds] = useState<string[]>([]);
  const [playingIds, setPlayingIds] = useState<string[]>([]);
  const [uninstallTarget, setUninstallTarget] = useState<Profile | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Profile | null>(null);
  const [manageInfo, setManageInfo] = useState<ProfileManageInfo | null>(null);
  const [profileActionIds, setProfileActionIds] = useState<string[]>([]);
  const [resolvedProfileStates, setResolvedProfileStates] = useState<Record<string, string>>({});
  const searchRef = useRef<HTMLInputElement>(null);
  const commandInputRef = useRef<HTMLInputElement>(null);
  const recentRef = useRef<HTMLDivElement>(null);
  const authRef = useRef<HTMLDivElement>(null);
  const brandMenuRef = useRef<HTMLDivElement>(null);
  const toolbarActionsRef = useRef<HTMLDivElement>(null);
  const toastTimer = useRef<number | undefined>(undefined);
  const bootstrapStarted = useRef(false);
  const deferredSearch = useDeferredValue(search);
  const selected = useMemo(() => findSelectedProfile(profiles, selectedId), [profiles, selectedId]);
  const activeDownloads = useMemo(() => downloads.filter(item => item.state === "queued" || item.state === "downloading"), [downloads]);
  const failedDownloads = useMemo(() => downloads.filter(item => item.state === "failed"), [downloads]);
  const completedDownloadCount = useMemo(() => downloads.filter(item => item.state !== "queued" && item.state !== "downloading" && item.state !== "failed").length, [downloads]);
  const downloadProgress = useMemo(() => {
    const received = activeDownloads.reduce((total, item) => total + item.bytesReceived, 0);
    const total = activeDownloads.reduce((sum, item) => sum + (item.totalBytes ?? 0), 0);
    return total > 0 ? Math.min(100, received / total * 100) : 0;
  }, [activeDownloads]);
  const visibleProfiles = useMemo(() => {
    const term = deferredSearch.trim().toLocaleLowerCase();
    const source = libraryTab === "Favorites" ? profiles.filter(profile => profile.favorite && !profile.hidden)
      : libraryTab === "Clients" ? profiles.filter(profile => !profile.hidden)
        : libraryTab === "Profiles" ? profiles
          : [];
    const filtered = source.filter(profile => {
      const java = requiredJavaMajor(profile);
      const needsAttention = ["RepairRequired", "UpdateAvailable"].includes(resolvedProfileStates[profile.id] ?? "") || (profile.clientReady && !profile.installed);
      const matchesGroup = groupFilter === "all" || (groupFilter === "__ungrouped" ? !(profile.groupName ?? "").trim() : (profile.groupName ?? "").trim() === groupFilter);
      const matchesStatus = statusFilter === "all" || (statusFilter === "installed" && profile.installed) || (statusFilter === "not-installed" && !profile.installed) || (statusFilter === "attention" && needsAttention);
      const matchesClient = clientFilter === "all" || (clientFilter === "snine" ? profile.snineclientEnabled : !profile.snineclientEnabled);
      const searchable = `${profile.name} ${profile.groupName ?? ""} ${profile.minecraftVersion} ${profile.clientVersion} ${profile.loaderType} ${profile.loaderVersion ?? ""} Java ${java} ${profile.snineclientEnabled ? "SNineClient" : "Vanilla"} ${profile.installed ? "Installed Ready" : "Not installed Setup"} ${resolvedProfileStates[profile.id] ?? ""}`.toLocaleLowerCase();
      return (!installedOnly || profile.installed) && matchesGroup && (loaderFilter === "all" || profile.loaderType === loaderFilter) && matchesStatus && (javaFilter === "all" || javaFilter === String(java)) && matchesClient && (!term || searchable.includes(term));
    });
    if (sortLabel === "Name A–Z") return filtered.sort((a, b) => a.name.localeCompare(b.name));
    if (sortLabel === "Favorites first") return filtered.sort((a, b) => Number(b.favorite) - Number(a.favorite) || a.name.localeCompare(b.name));
    if (sortLabel === "Installed first") return filtered.sort((a, b) => Number(b.installed) - Number(a.installed) || a.name.localeCompare(b.name));
    if (sortLabel === "Minecraft version") return filtered.sort((a, b) => b.minecraftVersion.localeCompare(a.minecraftVersion, undefined, { numeric: true, sensitivity: "base" }) || a.name.localeCompare(b.name));
    if (sortLabel === "Loader") return filtered.sort((a, b) => a.loaderType.localeCompare(b.loaderType) || a.name.localeCompare(b.name));
    if (sortLabel === "Group") return filtered.sort((a, b) => (a.groupName ?? "").localeCompare(b.groupName ?? "") || a.name.localeCompare(b.name));
    if (sortLabel === "Java runtime") return filtered.sort((a, b) => requiredJavaMajor(a) - requiredJavaMajor(b) || a.name.localeCompare(b.name));
    if (sortLabel === "Recently played") return filtered.sort((a, b) => {
      const left = recentIds.indexOf(a.id); const right = recentIds.indexOf(b.id);
      return (left < 0 ? Number.MAX_SAFE_INTEGER : left) - (right < 0 ? Number.MAX_SAFE_INTEGER : right);
    });
    return filtered;
  }, [profiles, deferredSearch, sortLabel, libraryTab, installedOnly, recentIds, groupFilter, loaderFilter, statusFilter, javaFilter, clientFilter, resolvedProfileStates]);
  const activeLibraryFilterCount = [installedOnly, groupFilter !== "all", loaderFilter !== "all", statusFilter !== "all", javaFilter !== "all", clientFilter !== "all"].filter(Boolean).length;
  const profileGroups = useMemo(() => [...new Set(profiles.map(profile => (profile.groupName ?? "").trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b)), [profiles]);

  useEffect(() => { saveLibraryPreferences({ tab: libraryTab, view, sort: sortLabel, installedOnly, groupFilter, loaderFilter, statusFilter, javaFilter, clientFilter }); }, [libraryTab, view, sortLabel, installedOnly, groupFilter, loaderFilter, statusFilter, javaFilter, clientFilter]);

  useLayoutEffect(() => {
    markStartup("shell_mounted");
    requestAnimationFrame(() => requestAnimationFrame(() => markStartup("first_paint")));
  }, []);
  useEffect(() => {
    if (!isTauri() || !import.meta.env.PROD) return;
    let active = true;
    let pending: Update | null = null;
    const timer = window.setTimeout(() => {
      void check({ timeout: 12_000 }).then(update => {
        if (!active) {
          if (update) void update.close();
          return;
        }
        if (update) {
          pending = update;
          setLauncherUpdate(update);
        }
      }).catch(error => console.warn("Launcher update check failed", error));
    }, 6000);
    return () => {
      active = false;
      window.clearTimeout(timer);
      if (pending) void pending.close();
    };
  }, []);
  useEffect(() => {
    if (!createProfileOpen || !isTauri() || !newMinecraftVersion) { setAvailableLoaderTypes(["vanilla", "fabric"]); return; }
    let active = true;
    void ipc.getSupportedLoaderTypes(newMinecraftVersion).then(items => {
      if (!active) return;
      setAvailableLoaderTypes(items);
      if (!items.includes(newLoaderType)) setNewLoaderType(items[0] ?? "vanilla");
    }).catch(() => { if (active) setAvailableLoaderTypes(["vanilla"]); });
    return () => { active = false; };
  }, [createProfileOpen, newMinecraftVersion]);
  useEffect(() => {
    if (bootstrapStarted.current) return;
    bootstrapStarted.current = true;
    void ipc.bootstrap().then(state => {
      setProfiles(state.profiles);
      setActivity(state.activity);
      setSelectedId(state.selectedProfileId);
      setRecentIds(state.recentProfileIds.length ? state.recentProfileIds : state.selectedProfileId ? [state.selectedProfileId] : []);
      markStartup("local_cache_loaded");
      void Promise.all(state.profiles.map(profile => ipc.resolveProfileState(profile.id).catch(() => null))).then(resolved => {
        const states: Record<string, string> = {};
        resolved.forEach(item => { if (item) states[item.profileId] = item.state; });
        setResolvedProfileStates(states);
        setProfiles(current => current.map(profile => {
          const status = states[profile.id];
          return status === "NotInstalled" ? { ...profile, installed: false } : status === "Installed" || status === "Running" || status === "RepairRequired" || status === "UpdateAvailable" ? { ...profile, installed: true } : profile;
        }));
      });
    }).catch(error => {
      console.error("Launcher bootstrap failed", error);
      showToast("Local launcher data could not be loaded.");
    });
  }, []);
  useEffect(() => {
    if (!createProfileOpen || !isTauri()) return;
    let active = true;
    void ipc.getMinecraftVersions(showSnapshots).then(items => {
      if (!active) return;
      setMinecraftVersions(items);
      if (!items.some(item => item.id === newMinecraftVersion)) setNewMinecraftVersion(items.find(item => item.versionType === "release")?.id ?? items[0]?.id ?? "");
      void ipc.getMinecraftVersions(showSnapshots, true).then(fresh => { if (active) setMinecraftVersions(fresh); }).catch(() => undefined);
    }).catch(error => { if (active) showToast(`Could not load Minecraft versions: ${String(error)}`); });
    return () => { active = false; };
  }, [createProfileOpen, showSnapshots]);
  useEffect(() => {
    if (!createProfileOpen || !["fabric", "quilt"].includes(newLoaderType) || !isTauri() || !newMinecraftVersion) { setFabricLoaderVersions([]); return; }
    let active = true;
    const versionsPromise = newLoaderType === "quilt" ? ipc.getQuiltLoaderVersions(newMinecraftVersion) : ipc.getFabricLoaderVersions(newMinecraftVersion);
    void versionsPromise.then(items => {
      if (!active) return;
      setFabricLoaderVersions(items);
      setNewLoaderVersion(current => items.includes(current) ? current : items[0] ?? "");
    }).catch(() => { if (active) { setFabricLoaderVersions([]); setNewLoaderVersion(""); } });
    return () => { active = false; };
  }, [createProfileOpen, newLoaderType, newMinecraftVersion]);
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    const stops: Array<() => void> = [];
    void Promise.all([
      listen<{ profileId: string; state: string; pid: number | null; message: string | null }>("launch-state", event => {
        const { profileId, state, message } = event.payload;
        setPlayingIds(ids => state === "running" ? ids.includes(profileId) ? ids : [...ids, profileId] : ids.filter(id => id !== profileId));
        if (state === "running" && minimizeOnGameStart) void getCurrentWindow().minimize().catch(error => console.warn("Could not minimize launcher after game start", error));
        void ipc.resolveProfileState(profileId).then(resolved => setResolvedProfileStates(current => ({ ...current, [profileId]: resolved.state }))).catch(() => undefined);
        if (state === "exited" || state === "crashed") void ipc.bootstrap().then(snapshot => setActivity(snapshot.activity)).catch(() => undefined);
        if (state === "crashed") showToast("Minecraft closed unexpectedly. Open profile Manage to inspect logs or repair the installation.");
        else if (message) showToast(message);
      }),
      listen<LaunchVerification>("launch-verification", event => {
        if (!active) return;
        const { fabricLoaded, loaderType = "fabric", snineClientLoaded, snineClientExpected, loaderVersion } = event.payload;
        const loaderExpected = loaderType === "fabric" || loaderType === "quilt";
        if (loaderExpected && !fabricLoaded) showToast(`Minecraft exited before ${loaderType === "quilt" ? "Quilt" : "Fabric"} ${loaderVersion} was confirmed in the game log.`);
        else if (snineClientExpected && !snineClientLoaded) showToast("Minecraft exited before SNineClient was confirmed in the game log.");
      }),
      listen<{ profileId: string; state: string; detail?: string }>("install-progress", event => {
        if (!active) return;
        const { profileId, state } = event.payload;
        if (state === "ready") {
          void ipc.resolveProfileState(profileId).then(resolved => setResolvedProfileStates(current => ({ ...current, [profileId]: resolved.state }))).catch(() => undefined);
          void ipc.bootstrap().then(local => { setProfiles(local.profiles); setActivity(local.activity); setSelectedId(local.selectedProfileId); }).catch(() => undefined);
        } else if (state === "failed") {
          void ipc.resolveProfileState(profileId).then(resolved => setResolvedProfileStates(current => ({ ...current, [profileId]: resolved.state }))).catch(() => undefined);
        } else {
          setResolvedProfileStates(current => ({ ...current, [profileId]: state.includes("repair") ? "Repairing" : state.includes("update") ? "Updating" : "Installing" }));
        }
      }),
    ]).then(unlisteners => { if (active) stops.push(...unlisteners); else unlisteners.forEach(stop => stop()); })
      .catch(error => console.warn("Could not subscribe to launch events", error));
    return () => { active = false; stops.forEach(stop => stop()); };
  }, [minimizeOnGameStart]);
  useEffect(() => {
    if (!settingsOpen || !isTauri()) return;
    void Promise.all([ipc.getMinecraftMemory(), ipc.getDownloadConcurrency(), ipc.getMinimizeOnGameStart(), ipc.getStorageInfo(), ipc.getMinecraftWindowSettings()]).then(([memory, concurrency, minimize, storage, window]) => {
      setMinecraftMemoryMb(memory); setDownloadConcurrency(concurrency); setMinimizeOnGameStart(minimize);
      setStorageInfo(storage); setWindowSettings(window);
    }).catch(error => showToast(`Could not load launcher settings: ${String(error)}`));
  }, [settingsOpen]);
  useEffect(() => {
    if (!isTauri()) return;
    void ipc.detectRuntime(21).then(setRuntimeStatus).catch(error => console.warn("Java runtime detection failed", error));
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    const stops: Array<() => void> = [];
    void Promise.all([
      listen<Partial<AuthState>>("auth-state-changed", event => { if (active) setAuthState(current => ({ ...current, ...event.payload, accounts: event.payload.accounts ?? current.accounts } as AuthState)); }),
      listen<{ account: SafeAccount; persistenceWarning?: string | null }>("auth-login-completed", event => { if (active) { setAuthState(current => ({ ...current, status: "loggedIn", account: event.payload.account, accounts: [event.payload.account, ...current.accounts.filter(item => item.uuid !== event.payload.account.uuid)], persistenceWarning: event.payload.persistenceWarning })); void ipc.authState().then(state => { if (active) setAuthState(state); }); setLoginPrompt(null); setAuthError(""); } }),
      listen<{ message: string }>("auth-login-failed", event => { if (active) { setAuthError(event.payload.message); setLoginPrompt(null); setAuthState(current => ({ ...current, status: current.account ? "reloginRequired" : "loggedOut" })); } }),
      listen("auth-logged-out", () => { if (active) { setAuthState(current => ({ ...current, status: "loggedOut", account: null })); setLoginPrompt(null); } }),
    ]).then(unlisteners => { if (active) stops.push(...unlisteners); else unlisteners.forEach(stop => stop()); }).catch(error => console.warn("Could not subscribe to auth events", error));
    void ipc.authState().then(async initial => {
      if (!active) return;
      setAuthState(initial);
      if (initial.account) {
        try { const restored = await ipc.authRestore(); if (active) setAuthState(restored); }
        catch (error) { if (active) { setAuthState({ ...initial, status: "reloginRequired" }); setAuthError(error instanceof Error ? error.message : String(error)); } }
      }
    }).catch(error => console.warn("Could not restore saved account metadata", error));
    return () => { active = false; stops.forEach(stop => stop()); };
  }, []);
  useEffect(() => {
    if (!authMenuOpen) return;
    const close = (event: PointerEvent) => { if (!authRef.current?.contains(event.target as Node)) setAuthMenuOpen(false); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") { setAuthMenuOpen(false); setLoginPrompt(null); void ipc.authCancelSignIn().catch(() => undefined); } };
    document.addEventListener("pointerdown", close, true); document.addEventListener("keydown", key);
    return () => { document.removeEventListener("pointerdown", close, true); document.removeEventListener("keydown", key); };
  }, [authMenuOpen]);
  useEffect(() => {
    if (!brandMenuOpen) return;
    const close = (event: PointerEvent) => { if (!brandMenuRef.current?.contains(event.target as Node)) setBrandMenuOpen(false); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") setBrandMenuOpen(false); };
    document.addEventListener("pointerdown", close, true); document.addEventListener("keydown", key);
    return () => { document.removeEventListener("pointerdown", close, true); document.removeEventListener("keydown", key); };
  }, [brandMenuOpen]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    let active = true;
    void listen<DownloadProgress>("download-progress", event => {
      setDownloads(items => {
        const index = items.findIndex(item => item.id === event.payload.id);
        if (index < 0) return [...items, event.payload];
        const next = items.slice();
        next[index] = event.payload;
        return next;
      });
    }).then(stop => { if (active) unlisten = stop; else stop(); }).catch(error => console.warn("Could not subscribe to downloads", error));
    return () => { active = false; unlisten?.(); };
  }, []);
  const installClient = useCallback(async (profile: Profile) => {
    if (!isTauri()) { showToast("Client installation is available in the native launcher."); return; }
    setInstallingIds(ids => ids.includes(profile.id) ? ids : [...ids, profile.id]);
    setSideTab("Downloads");
    try {
      if (profile.snineclientEnabled && !profile.clientReady) {
        const result = await ipc.installClient(profile.id);
        setProfiles(items => items.map(item => item.id === profile.id ? { ...item, clientReady: true, clientVersion: result.clientVersion, minecraftVersion: result.minecraftVersion } : item));
        const localState = await ipc.bootstrap();
        setProfiles(localState.profiles);
        setActivity(localState.activity);
        setSelectedId(localState.selectedProfileId);
      }
      const requiredJava = requiredJavaMajor(profile);
      let currentRuntime = runtimeStatus?.requiredMajor === requiredJava ? runtimeStatus : await ipc.detectRuntime(requiredJava);
      setRuntimeStatus(currentRuntime);
      if (!currentRuntime.compatible) {
        await ipc.installManagedJava(requiredJava);
        currentRuntime = await ipc.detectRuntime(requiredJava);
        setRuntimeStatus(currentRuntime);
        if (!currentRuntime.compatible) throw new Error(`Managed Java ${requiredJava} installed, but runtime validation did not find a compatible executable.`);
      }
      const result = await ipc.installMinecraftProfile(profile.id);
      const state = await ipc.bootstrap();
      setProfiles(state.profiles);
      setActivity(state.activity);
      setSelectedId(state.selectedProfileId);
      showToast(profile.loaderType === "fabric" || profile.loaderType === "quilt" ? `Minecraft ${result.minecraftVersion} with ${profile.loaderType === "quilt" ? "Quilt" : "Fabric"} ${result.loaderVersion}${profile.snineclientEnabled ? " and SNineClient" : ""} is ready (${result.libraryCount} libraries, ${result.assetCount} assets).` : `Vanilla Minecraft ${result.minecraftVersion} is ready (${result.libraryCount} libraries, ${result.assetCount} assets).`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      showToast(message.includes("DiskFull") ? "Not enough disk space to install the profile." : `Install failed: ${message}`);
      console.error("SNine client install failed", error);
    } finally {
      setInstallingIds(ids => ids.filter(id => id !== profile.id));
      if (isTauri()) void ipc.resolveProfileState(profile.id).then(resolved => setResolvedProfileStates(states => ({ ...states, [profile.id]: resolved.state }))).catch(() => undefined);
    }
  }, [runtimeStatus]);
  const playProfile = useCallback(async (profile: Profile) => {
    if (!isTauri()) { showToast("Minecraft launch is available in the native launcher."); return; }
    if (playingIds.includes(profile.id)) return;
    try {
      setPlayingIds(ids => ids.includes(profile.id) ? ids : [...ids, profile.id]);
      const result = await ipc.launchMinecraftProfile(profile.id);
      if (result.state !== "running") setPlayingIds(ids => ids.filter(id => id !== profile.id));
      const state = await ipc.bootstrap();
      setActivity(state.activity);
      setRecentIds(state.recentProfileIds);
      showToast(`Started ${profile.name}.`);
    } catch (error) {
      setPlayingIds(ids => ids.filter(id => id !== profile.id));
      const message = error instanceof Error ? error.message : String(error);
      showToast(message);
      console.error("Minecraft launch failed", error);
    }
  }, [playingIds]);
  const reloadLocalData = useCallback(async () => {
    const state = await ipc.bootstrap();
    setProfiles(state.profiles); setActivity(state.activity); setSelectedId(state.selectedProfileId); setRecentIds(state.recentProfileIds);
    if (isTauri()) {
      const resolved = await Promise.all(state.profiles.map(profile => ipc.resolveProfileState(profile.id).catch(() => null)));
      const states: Record<string, string> = {};
      resolved.forEach(item => { if (item) states[item.profileId] = item.state; });
      setResolvedProfileStates(states);
      setProfiles(state.profiles.map(profile => ({ ...profile, installed: ["Installed", "Running", "RepairRequired", "UpdateAvailable"].includes(states[profile.id] ?? "") })));
    }
  }, []);
  const runProfileMaintenance = useCallback(async (profile: Profile, mode: "repair" | "update") => {
    if (!isTauri()) { showToast("Profile maintenance is available in the native launcher."); return; }
    setProfileActionIds(ids => ids.includes(profile.id) ? ids : [...ids, profile.id]); setSideTab("Downloads");
    try {
      const requiredJava = requiredJavaMajor(profile);
      let runtime = runtimeStatus?.requiredMajor === requiredJava ? runtimeStatus : await ipc.detectRuntime(requiredJava); setRuntimeStatus(runtime);
      if (!runtime.compatible) {
        await ipc.installManagedJava(requiredJava); runtime = await ipc.detectRuntime(requiredJava); setRuntimeStatus(runtime);
      }
      if (!runtime.compatible) throw new Error(`A compatible Java ${requiredJava} runtime is required.`);
      if (mode === "repair") {
        const result = await ipc.repairMinecraftProfile(profile.id);
        await reloadLocalData();
        showToast(`Verified and repaired ${profile.name} (${result.libraryCount} libraries, ${result.assetCount} assets checked).`);
      } else {
        const result = await ipc.updateMinecraftProfile(profile.id);
        if (!result.updateAvailable) {
          showToast(result.check.comparable ? `No newer SNineClient version is available (${result.check.currentVersion}).` : "The SNine release endpoint did not provide a comparable version; no update was applied.");
        } else {
          await reloadLocalData();
          showToast(`${profile.name} was updated and verified.`);
        }
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      showToast(`${mode === "repair" ? "Repair" : "Update"} failed: ${message}`);
      console.error(`Profile ${mode} failed`, error);
    } finally {
      setProfileActionIds(ids => ids.filter(id => id !== profile.id));
      if (isTauri()) void ipc.resolveProfileState(profile.id).then(resolved => setResolvedProfileStates(states => ({ ...states, [profile.id]: resolved.state }))).catch(() => undefined);
    }
  }, [reloadLocalData, runtimeStatus]);
  const checkProfileUpdates = useCallback(async (profile: Profile) => {
    if (!isTauri()) { showToast("Update checks are available in the native launcher."); return; }
    setProfileActionIds(ids => ids.includes(profile.id) ? ids : [...ids, profile.id]);
    try {
      const check = await ipc.checkProfileUpdate(profile.id);
      if (check.available) {
        setResolvedProfileStates(states => ({ ...states, [profile.id]: "UpdateAvailable" }));
        showToast(`SNineClient ${check.latestVersion} is available. Select UPDATE on the profile to apply it.`);
      } else {
        setResolvedProfileStates(states => ({ ...states, [profile.id]: "Installed" }));
        showToast(check.comparable ? `No newer SNineClient version is available (${check.currentVersion}).` : "The server did not provide a comparable version; an update cannot be confirmed.");
      }
    } catch (error) { showToast(`Update check failed: ${error instanceof Error ? error.message : String(error)}`); }
    finally {
      setProfileActionIds(ids => ids.filter(id => id !== profile.id));
    }
  }, []);
  const performUninstall = useCallback(async () => {
    if (!uninstallTarget) return;
    const target = uninstallTarget;
    try {
      await ipc.uninstallMinecraftProfile(target.id);
      await reloadLocalData();
      setProfileViewId(null); setManageInfo(null); setUninstallTarget(null);
      showToast(`${target.name} was uninstalled. Personal files and shared caches were kept.`);
    } catch (error) { showToast(error instanceof Error ? error.message : String(error)); }
  }, [uninstallTarget, reloadLocalData]);
  useEffect(() => () => {
    if (toastTimer.current !== undefined) window.clearTimeout(toastTimer.current);
  }, []);
  useEffect(() => { if (!isTauri()) saveIds(RECENTS_KEY, recentIds); }, [recentIds]);
  useEffect(() => {
    if (!recentMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => { if (!recentRef.current?.contains(event.target as Node)) setRecentMenuOpen(false); };
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") setRecentMenuOpen(false); };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    return () => { document.removeEventListener("pointerdown", onPointerDown, true); document.removeEventListener("keydown", onKeyDown); };
  }, [recentMenuOpen]);
  useEffect(() => {
    if (!sortOpen && !viewMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => { if (!toolbarActionsRef.current?.contains(event.target as Node)) { setSortOpen(false); setViewMenuOpen(false); } };
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") { setSortOpen(false); setViewMenuOpen(false); } };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    return () => { document.removeEventListener("pointerdown", onPointerDown, true); document.removeEventListener("keydown", onKeyDown); };
  }, [sortOpen, viewMenuOpen]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const appWindow = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;
    const syncMaximized = () => {
      void appWindow.isMaximized().then(maximized => {
        if (active) setIsMaximized(maximized);
      });
    };
    syncMaximized();
    void appWindow.onResized(syncMaximized).then(stopListening => {
      if (active) unlisten = stopListening;
      else stopListening();
    }).catch(error => console.warn("Could not subscribe to window resize events", error));
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setCommandPaletteOpen(true);
        setCommandQuery("");
        setCommandSelection(0);
      } else if ((event.ctrlKey || event.metaKey) && event.shiftKey && ["Digit1", "Digit2", "Digit3"].includes(event.code) && !commandPaletteOpen) {
        event.preventDefault();
        setPage("Library"); setProfileViewId(null);
        setLibraryTab(event.code === "Digit1" ? "Clients" : event.code === "Digit2" ? "Profiles" : "Favorites");
      } else if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "f" && !commandPaletteOpen) {
        event.preventDefault();
        setSideTab("Friends");
        requestAnimationFrame(() => document.getElementById("social-friend-search")?.focus());
      } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f" && !commandPaletteOpen) {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [commandPaletteOpen]);
  useEffect(() => {
    if (!commandPaletteOpen) return;
    const frame = window.requestAnimationFrame(() => commandInputRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [commandPaletteOpen]);
  const navigate = (next: AppPage) => {
    if (next === page) return;
    const nextHistory = history.slice(0, historyIndex + 1);
    nextHistory.push(next);
    setHistory(nextHistory);
    setHistoryIndex(nextHistory.length - 1);
    setPage(next);
    setSideTab("Friends");
    setProfileViewId(null);
  };
  const moveHistory = (direction: -1 | 1) => {
    const index = Math.min(history.length - 1, Math.max(0, historyIndex + direction));
    if (index !== historyIndex) { setHistoryIndex(index); setPage(history[index]); }
  };
  const selectProfile = useCallback(async (profile: Profile, openDetails = true) => {
    setSelectedId(profile.id);
    setRecentIds(current => [profile.id, ...current.filter(id => id !== profile.id)].slice(0, 8));
    if (isTauri()) {
      await ipc.selectProfile(profile.id);
      const state = await ipc.bootstrap();
      setActivity(state.activity);
    }
    if (openDetails) setProfileViewId(profile.id);
  }, []);
  const updateProfileFlag = (id: string, flag: "favorite" | "hidden") => {
    const profile = profiles.find(item => item.id === id);
    if (!profile) return;
    const value = !profile[flag];
    setProfiles(items => items.map(item => item.id === id ? { ...item, [flag]: value } : item));
    if (isTauri()) void (flag === "favorite" ? ipc.setFavorite(id, value) : ipc.setHidden(id, value)).then(async () => {
      const state = await ipc.bootstrap();
      setActivity(state.activity);
    }).catch(error => {
      setProfiles(items => items.map(item => item.id === id ? { ...item, [flag]: !value } : item));
      showToast(`Could not save profile ${flag}.`);
      console.error(`Failed to persist profile ${flag}`, error);
    });
  };
  const toggleFavorite = (id: string) => updateProfileFlag(id, "favorite");
  const toggleHidden = (id: string) => updateProfileFlag(id, "hidden");
  const addProfile = useCallback(() => { setNewProfileName(`Profile ${profiles.length + 1}`); setNewProfileMemoryMb(""); setNewSnineEnabled(true); setNewLoaderType("fabric"); setNewProfileIconId(null); setNewProfileIconPreview(null); setShowSnapshots(false); setCreateProfileOpen(true); }, [profiles.length]);
  const cancelAllDownloads = async () => {
    if (downloadBatchAction || activeDownloads.length === 0) return;
    const pending = activeDownloads.slice();
    setDownloadBatchAction("cancel");
    const results = await Promise.allSettled(pending.map(item => ipc.cancelDownload(item.id)));
    const cancelled = results.filter(result => result.status === "fulfilled" && result.value).length;
    showToast(`Cancelled ${cancelled} of ${pending.length} active downloads.`);
    setDownloadBatchAction(null);
  };
  const retryAllFailedDownloads = async () => {
    if (downloadBatchAction || failedDownloads.length === 0) return;
    const pending = failedDownloads.slice();
    setDownloadBatchAction("retry");
    const results = await Promise.allSettled(pending.map(item => ipc.retryDownload(item.id)));
    const queued = results.filter(result => result.status === "fulfilled").length;
    showToast(`Queued ${queued} of ${pending.length} failed downloads for retry.`);
    setDownloadBatchAction(null);
  };
  useEffect(() => {
    const onNewProfile = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "n" || event.altKey || commandPaletteOpen) return;
      event.preventDefault();
      setPage("Library"); setProfileViewId(null); addProfile();
    };
    window.addEventListener("keydown", onNewProfile);
    return () => window.removeEventListener("keydown", onNewProfile);
  }, [addProfile, commandPaletteOpen]);
  const launcherCommands = useMemo<LauncherCommand[]>(() => {
    const commands: LauncherCommand[] = [
      { id: "library", label: "Go to Library", description: "Browse your Minecraft profiles", keywords: "home clients profiles", icon: <LibraryBig size={16} />, run: () => { navigate("Library"); setProfileViewId(null); } },
      { id: "cosmetics", label: "Open Cosmetics", description: "Preview skins and manage capes", keywords: "skin cape appearance", icon: <Sparkles size={16} />, run: () => navigate("Cosmetics") },
      { id: "downloads", label: "Open Downloads", description: "View transfer and install progress", keywords: "queue transfer progress", icon: <SVGRepoIcon src={downloadArrowSvg} />, run: () => { navigate("Library"); setSideTab("Downloads"); } },
      { id: "settings", label: "Open Settings", description: "Appearance, Minecraft, downloads and storage", keywords: "preferences configure", icon: <SVGRepoIcon src={settingsGearSvg} />, run: () => setSettingsOpen(true) },
      { id: "create-profile", label: "Create a profile", description: "Set up a Minecraft version and loader", keywords: "new instance client", icon: <Plus size={16} />, run: () => { navigate("Library"); addProfile(); } },
    ];
    for (const profile of profiles.filter(item => !item.hidden)) {
      commands.push({
        id: `profile:${profile.id}`,
        label: `${profile.installed ? "Launch" : "Install"} ${profile.name}`,
        description: `Minecraft ${profile.minecraftVersion} · ${profile.loaderType}${profile.favorite ? " · Favorite" : ""}`,
        keywords: `${profile.name} ${profile.clientVersion} ${profile.loaderVersion ?? ""} ${profile.groupName ?? ""}`,
        icon: <SVGRepoIcon src={gamepadSvg} />,
        run: () => { if (profile.installed) void playProfile(profile); else void installClient(profile); },
      });
      commands.push({
        id: `open:${profile.id}`,
        label: `Open ${profile.name}`,
        description: "Show profile details and management tools",
        keywords: `${profile.name} details manage settings`,
        icon: <ChevronRight size={16} />,
        run: () => { navigate("Library"); void selectProfile(profile, true); },
      });
    }
    return commands;
  }, [profiles, navigate, addProfile, playProfile, installClient, selectProfile]);
  const filteredLauncherCommands = launcherCommands.filter(command => `${command.label} ${command.description} ${command.keywords}`.toLowerCase().includes(commandQuery.trim().toLowerCase()));
  const executeLauncherCommand = (command: LauncherCommand) => {
    setCommandPaletteOpen(false);
    setCommandQuery("");
    command.run();
  };
  const chooseProfileIcon = async (entry: BlockIconEntry) => {
    const target = iconPickerTarget;
    if (!target) return;
    setIconPickerTarget(null);
    if (target.kind === "create") { setNewProfileIconId(entry.id); setNewProfileIconPreview(entry.previewDataUrl); return; }
    const previous = profiles.find(item => item.id === target.id)?.iconId ?? null;
    setProfiles(items => items.map(item => item.id === target.id ? { ...item, iconId: entry.id } : item));
    try { await ipc.setProfileIcon(target.id, entry.id); }
    catch (error) { setProfiles(items => items.map(item => item.id === target.id ? { ...item, iconId: previous } : item)); showToast(`Could not save profile icon: ${String(error)}`); }
  };
  const createProfile = async () => {
    if (!isTauri() && !import.meta.env.DEV) {
      showToast("Profiles can only be managed in the desktop launcher.");
      return;
    }
    const trimmedName = newProfileName.trim();
    if (!trimmedName || trimmedName.length > 48) { showToast("Profile name must be 1–48 characters."); return; }
    const memoryText = newProfileMemoryMb.trim();
    const memoryMb = memoryText ? Number(memoryText) : null;
    if (memoryMb !== null && (!Number.isInteger(memoryMb) || memoryMb < 2048 || memoryMb > 32768 || memoryMb % 512 !== 0)) {
      showToast("Memory must be 2048–32768 MB in 512 MB steps."); return;
    }
    let created: Profile;
    try {
      if (isTauri()) {
        if (newSnineEnabled && (newLoaderType !== "fabric" || !["1.21.11", "26.1.2", "26.3"].includes(newMinecraftVersion))) {
          showToast("SNineClient currently has builds for Fabric on Minecraft 1.21.11, 26.1.2, and 26.3."); return;
        }
        created = await ipc.createProfile({ name: trimmedName, minecraftVersion: newMinecraftVersion, loaderType: newLoaderType, loaderVersion: ["fabric", "quilt"].includes(newLoaderType) ? newLoaderVersion || null : null, snineclientEnabled: newSnineEnabled, memoryMb, iconId: newProfileIconId });
        setProfiles(items => insertCreatedProfile(items, created));
        setSelectedId(created.id);
        setRecentIds(current => [created.id, ...current.filter(id => id !== created.id)].slice(0, 8));
        setProfileViewId(created.id);
        setCreateProfileOpen(false);
        void ipc.bootstrap().then(state => setActivity(state.activity)).catch(() => undefined);
        // The profile is visible before this existing async installation pipeline starts.
        void installClient(created);
      } else {
        created = { id: "demo-" + Date.now(), name: trimmedName, clientVersion: newSnineEnabled ? "1.1.5" : "Vanilla", minecraftVersion: newMinecraftVersion, installed: false, clientReady: false, selected: true, favorite: false, hidden: false, loaderType: newLoaderType, loaderVersion: newLoaderVersion || null, snineclientEnabled: newSnineEnabled, iconId: newProfileIconId, memoryMb };
        setProfiles(items => insertCreatedProfile(items, created));
        setSelectedId(created.id);
        setProfileViewId(created.id);
        setCreateProfileOpen(false);
      }
    } catch (error) {
      showToast(`Could not create profile: ${String(error).replace(/^Error: /, "")}`);
      return;
    }
    setRecentIds(current => [created.id, ...current.filter(id => id !== created.id)].slice(0, 8));
  };
  const renameProfile = async (profile: Profile, name: string) => {
    try {
      if (isTauri()) {
        const updated = await ipc.renameProfile(profile.id, name);
        setProfiles(items => items.map(item => item.id === profile.id ? updated : item));
        const state = await ipc.bootstrap(); setActivity(state.activity);
      } else setProfiles(items => items.map(item => item.id === profile.id ? { ...item, name: name.trim() } : item));
      showToast("Profile renamed.");
    } catch (error) { showToast(`Could not rename profile: ${String(error)}`); }
  };
  const duplicateProfile = async (profile: Profile) => {
    try {
      const copy = isTauri() ? await ipc.duplicateProfile(profile.id, `${profile.name} Copy`) : { ...profile, id: `demo-${Date.now()}`, name: `${profile.name} Copy`, installed: false, clientReady: false, selected: false };
      setProfiles(items => [...items, copy]);
      showToast(`Created ${copy.name}.`);
    } catch (error) { showToast(`Could not duplicate profile: ${String(error)}`); }
  };
  const deleteProfile = async () => {
    if (!deleteTarget) return;
    try {
      if (isTauri()) await ipc.deleteEmptyProfile(deleteTarget.id);
      setProfiles(items => items.filter(item => item.id !== deleteTarget.id));
      setRecentIds(items => items.filter(id => id !== deleteTarget.id));
      if (selectedId === deleteTarget.id) setSelectedId("snineclient-default");
      setProfileViewId(null); setDeleteTarget(null);
      const state = await ipc.bootstrap(); setActivity(state.activity); setSelectedId(state.selectedProfileId);
      showToast("Profile deleted.");
    } catch (error) { showToast(`Could not delete profile: ${String(error)}`); }
  };
  const windowAction = async (action: "minimize" | "toggleMaximize" | "close") => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    try {
      const appWindow = getCurrentWindow();
      if (action === "minimize") await appWindow.minimize();
      else if (action === "toggleMaximize") {
        await appWindow.toggleMaximize();
        setIsMaximized(await appWindow.isMaximized());
      } else await appWindow.close();
    } catch (error) {
      console.error(`Failed to ${action} launcher window`, error);
    }
  };
  const showToast = (message: string) => {
    setToast(message);
    if (toastTimer.current !== undefined) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(""), 2800);
  };
  const dismissLauncherUpdate = () => {
    const update = launcherUpdate;
    setLauncherUpdate(null);
    setLauncherUpdateProgress("");
    if (update) void update.close();
  };
  const installLauncherUpdate = async () => {
    if (!launcherUpdate || launcherUpdateBusy) return;
    setLauncherUpdateBusy(true);
    setLauncherUpdateProgress("Preparing update…");
    try {
      await launcherUpdate.downloadAndInstall(event => {
        if (event.event === "Started") setLauncherUpdateProgress("Downloading update…");
        if (event.event === "Progress") setLauncherUpdateProgress("Downloading update…");
        if (event.event === "Finished") setLauncherUpdateProgress("Installing update…");
      }, { restartAfterInstall: true });
      await relaunch();
    } catch (error) {
      setLauncherUpdateBusy(false);
      setLauncherUpdateProgress("");
      showToast(`Launcher update failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };
  const beginSignIn = async () => {
    setAuthError(""); setAuthMenuOpen(true); setAuthState(current => ({ ...current, status: "authorizing" }));
    try { setLoginPrompt(await ipc.authBeginSignIn()); }
    catch (error) { const message = error instanceof Error ? error.message : String(error); setAuthError(message); setAuthState(current => ({ ...current, status: current.account ? "loggedIn" : "loggedOut" })); }
  };
  const switchAccount = async (uuid: string) => {
    setAuthBusy(true); setAuthError("");
    try { setAuthState(await ipc.authSelectAccount(uuid)); setAuthMenuOpen(false); }
    catch (error) { setAuthError(error instanceof Error ? error.message : String(error)); }
    finally { setAuthBusy(false); }
  };
  const signOut = async () => {
    try { const state = await ipc.authSignOut(); setAuthState(state); setAuthError(""); setAuthMenuOpen(false); setLoginPrompt(null); }
    catch (error) { setAuthError(error instanceof Error ? error.message : String(error)); }
  };
  const retrySessionSave = async () => {
    try { setAuthState(await ipc.authRetryPersistSession()); setAuthError(""); }
    catch (error) { setAuthError(error instanceof Error ? error.message : String(error)); }
  };
  const saveMinecraftMemory = async () => {
    try { await ipc.setMinecraftMemory(minecraftMemoryMb); showToast(`Minecraft memory set to ${minecraftMemoryMb / 1024} GB.`); }
    catch (error) { showToast(error instanceof Error ? error.message : String(error)); }
  };
  const saveDownloadConcurrency = async (value: number) => {
    setDownloadConcurrency(value);
    try { await ipc.setDownloadConcurrency(value); showToast(`Download concurrency set to ${value}.`); }
    catch (error) { showToast(error instanceof Error ? error.message : String(error)); }
  };
  const saveMinimizeOnGameStart = async (value: boolean) => {
    setMinimizeOnGameStart(value);
    try { await ipc.setMinimizeOnGameStart(value); }
    catch (error) { setMinimizeOnGameStart(!value); showToast(error instanceof Error ? error.message : String(error)); }
  };
  const saveWindowSettings = async () => {
    if (!Number.isInteger(windowSettings.width) || windowSettings.width < 640 || windowSettings.width > 7680 || !Number.isInteger(windowSettings.height) || windowSettings.height < 480 || windowSettings.height > 4320) { showToast("Resolution must be between 640×480 and 7680×4320."); return; }
    try { await ipc.setMinecraftWindowSettings(windowSettings); showToast("Minecraft window settings saved for the next launch."); }
    catch (error) { showToast(error instanceof Error ? error.message : String(error)); }
  };
  const chooseCornerStyle = (style: CornerStyle) => {
    setCornerStyle(style);
    try { window.localStorage.setItem(CORNER_STYLE_KEY, style); } catch { /* setting still applies for this session */ }
  };
  const profileView = profiles.find(profile => profile.id === profileViewId) ?? null;
  useEffect(() => {
    setManageInfo(null);
    if (!profileViewId || !isTauri()) return;
    let active = true;
    void ipc.getProfileManageInfo(profileViewId).then(info => { if (active) setManageInfo(info); }).catch(() => undefined);
    return () => { active = false; };
  }, [profileViewId]);

  return <div className="app-shell" data-corners={cornerStyle}>
    <header className="topbar">
      <div className="brand-menu-wrap" ref={brandMenuRef}>
        <button className="brand-menu" aria-label="SNine Launcher menu" aria-haspopup="menu" aria-expanded={brandMenuOpen} onClick={() => setBrandMenuOpen(value => !value)}><img className="brand-mark-image" src={snineLauncherIcon} alt="SNine" /><ChevronDown size={13} /></button>
        {brandMenuOpen && <div className="brand-popover" role="menu" aria-label="SNine Launcher">
          <button className="brand-menu-item" role="menuitem" onClick={() => { setBrandMenuOpen(false); setSettingsOpen(true); }}><SVGRepoIcon src={settingsGearSvg} /><span>Settings</span></button>
          <button className="brand-menu-item" role="menuitem" onClick={() => { setBrandMenuOpen(false); if (authState.account) setAuthMenuOpen(true); else void beginSignIn(); }}><CircleUserRound size={16} /><span>{authState.account?.username ?? "Minecraft account"}</span><ChevronRight size={14} /></button>
          <div className="brand-menu-separator" />
          <button className="brand-menu-item" role="menuitem" onClick={() => { setBrandMenuOpen(false); setSideTab("Downloads"); }}><Download size={16} /><span>Downloads</span></button>
          {authState.account && <><div className="brand-menu-separator" /><button className="brand-menu-item" role="menuitem" onClick={() => { setBrandMenuOpen(false); void signOut(); }}><CircleUserRound size={16} /><span>Sign out</span></button></>}
          <div className="brand-menu-separator" />
          <button className="brand-menu-item" role="menuitem" onClick={() => { setBrandMenuOpen(false); void windowAction("close"); }}><X size={16} /><span>Quit SNine Launcher</span></button>
        </div>}
      </div>
      <div className="history-controls">
        <button className="chrome-button" aria-label="Back" disabled={historyIndex === 0} onClick={() => moveHistory(-1)}><ArrowLeft size={15} /></button>
        <button className="chrome-button" aria-label="Forward" disabled={historyIndex >= history.length - 1} onClick={() => moveHistory(1)}><ArrowRight size={15} /></button>
      </div>
      <nav className="primary-nav" aria-label="Main navigation">
        <button className={"primary-tab " + (page === "Library" ? "selected" : "")} onClick={() => navigate("Library")}><LibraryBig size={16} /> Library</button>
        <button className={"primary-tab " + (page === "Cosmetics" ? "selected" : "")} onClick={() => navigate("Cosmetics")}><Sparkles size={15} /> Cosmetics</button>
      </nav>
      <button className="command-trigger" aria-label="Open quick actions" onClick={() => { setCommandQuery(""); setCommandPaletteOpen(true); }}><Search size={14} /><span>Quick actions</span><kbd>Ctrl K</kbd></button>
      <div className="titlebar-drag-region" data-tauri-drag-region onDoubleClick={() => void windowAction("toggleMaximize")} />
      <div className="account-control-wrap" ref={authRef}>
        <button className="account-control" aria-haspopup="menu" aria-expanded={authMenuOpen} onClick={() => authState.account ? setAuthMenuOpen(value => !value) : void beginSignIn()}>
          {authState.account ? <MinecraftHead uuid={authState.account.uuid} name={authState.account.username} className="account-avatar" /> : <CircleUserRound size={16} />}
          <span>{authState.account?.username ?? (authState.status === "authorizing" ? "Signing in…" : "Sign in")}</span><ChevronDown size={13} />
        </button>
        {authMenuOpen && authState.account && !loginPrompt && <div className="account-popover account-switcher" role="menu" aria-label="Minecraft accounts">
          <small className="account-switcher-label">MINECRAFT ACCOUNTS · {authState.accounts.length}</small>
          <div className="account-switcher-list">{authState.accounts.map(account => <button key={account.uuid} role="menuitemradio" aria-checked={account.uuid === authState.account?.uuid} disabled={authBusy || account.uuid === authState.account?.uuid} onClick={() => void switchAccount(account.uuid)}><MinecraftHead uuid={account.uuid} name={account.username} className="account-avatar" small /><span className="account-switcher-name"><strong>{account.username}</strong><small>{account.uuid === authState.account?.uuid ? "Currently active" : "Switch account"}</small></span>{account.uuid === authState.account?.uuid ? <Check size={15} /> : authBusy ? <LoaderCircle size={15} className="account-spinner" /> : null}</button>)}</div>
          {authState.persistenceWarning && <><div className="account-menu-separator" /><small className="auth-error">{authState.persistenceWarning}</small>{authState.status === "loggedIn" ? <button role="menuitem" onClick={() => void retrySessionSave()}>Retry secure save</button> : <button role="menuitem" onClick={() => void beginSignIn()}>Sign in again</button>}</>}
          {authError && <small className="auth-error account-switch-error">{authError}</small>}
          <div className="account-menu-separator" /><button className="account-add-button" role="menuitem" disabled={authBusy} onClick={() => void beginSignIn()}><UserPlus size={15} />Add another account</button><button role="menuitem" onClick={() => void signOut()}>Sign out current account</button>
        </div>}
        {authMenuOpen && loginPrompt && <div className="account-popover auth-prompt" role="dialog" aria-label="Microsoft sign-in"><strong>Continue with Microsoft</strong><small>Enter this code at Microsoft to connect your Minecraft account.</small><code className="auth-device-code">{loginPrompt.userCode}</code><a href={loginPrompt.verificationUri} target="_blank" rel="noreferrer"><ExternalLink size={14} /> {loginPrompt.verificationUri}</a><small className="auth-waiting">Waiting for Microsoft authentication…</small>{authError && <small className="auth-error">{authError}</small>}<button className="auth-cancel" onClick={() => { void ipc.authCancelSignIn(); setLoginPrompt(null); setAuthState(current => ({ ...current, status: current.account ? "loggedIn" : "loggedOut" })); }}>Cancel sign-in</button></div>}
        {authMenuOpen && !loginPrompt && !authState.account && <div className="account-popover auth-prompt" role="status"><strong>{authState.status === "authorizing" ? "Opening Microsoft sign-in…" : "Minecraft account"}</strong>{authState.status === "authorizing" && <small>Preparing secure device sign-in…</small>}{authError && <small className="auth-error">{authError}</small>}</div>}
      </div>
      <div className="window-controls">
        <button aria-label="Minimize" title="Minimize" onClick={() => void windowAction("minimize")}><Minus size={14} /></button>
        <button aria-label={isMaximized ? "Restore" : "Maximize"} title={isMaximized ? "Restore" : "Maximize"} onClick={() => void windowAction("toggleMaximize")}>{isMaximized ? <Copy size={12} /> : <Square size={11} />}</button>
        <button className="close-control" aria-label="Close" title="Close" onClick={() => void windowAction("close")}><X size={15} /></button>
      </div>
    </header>

    <div className="workspace">
      <main className="content-panel">
        {page === "Library" && profileView ? <ProfileDetails profile={profileView} resolvedState={resolvedProfileStates[profileView.id]} installing={installingIds.includes(profileView.id) || profileActionIds.includes(profileView.id)} runtimeStatus={runtimeStatus} manageInfo={manageInfo} onInstall={() => profileView.installed ? void playProfile(profileView) : void installClient(profileView)} onRepair={() => void runProfileMaintenance(profileView, "repair")} onUpdate={() => void runProfileMaintenance(profileView, "update")} onCheckUpdate={() => void checkProfileUpdates(profileView)} onUninstall={() => setUninstallTarget(profileView)} onDelete={() => setDeleteTarget(profileView)} onOpenFolder={() => void ipc.openMinecraftProfileFolder(profileView.id).catch(error => showToast(String(error)))} onRename={name => void renameProfile(profileView, name)} onGroupChanged={groupName => setProfiles(items => items.map(item => item.id === profileView.id ? { ...item, groupName } : item))} onDuplicate={() => void duplicateProfile(profileView)} onChangeIcon={() => setIconPickerTarget({ kind: "profile", id: profileView.id, version: manageInfo?.minecraftVersion ?? profileView.minecraftVersion })} onBack={() => setProfileViewId(null)} /> : page === "Library" ? <>
          <div className="section-tabs" role="tablist" aria-label="Library categories">
            {(["Clients", "Profiles", "Favorites"] as LibraryTab[]).map(tab => { const count = tab === "Clients" ? profiles.filter(profile => !profile.hidden).length : tab === "Favorites" ? profiles.filter(profile => profile.favorite && !profile.hidden).length : profiles.length; const shortcut = tab === "Clients" ? "1" : tab === "Profiles" ? "2" : "3"; return <button key={tab} role="tab" aria-selected={libraryTab === tab} className={"section-tab " + (libraryTab === tab ? "active" : "")} title={`${tab} · Ctrl+Shift+${shortcut}`} onClick={() => setLibraryTab(tab)}>{tab}<span className="section-tab-count">{count}</span></button>; })}
            <button className="create-profile" onClick={() => void addProfile()}><Plus size={14} /> New profile</button>
          </div>
          <div className="library-toolbar">
            <label className="search-field"><Search size={16} /><input ref={searchRef} value={search} onChange={event => setSearch(event.target.value)} placeholder="Search profiles, groups, loaders…" /></label>
            <span className="toolbar-divider" aria-hidden="true" />
            <div className="toolbar-actions" ref={toolbarActionsRef}>
              <button className={`installed-filter ${installedOnly ? "active" : ""}`} aria-label="Show installed profiles only" aria-pressed={installedOnly} title="Show installed profiles only" onClick={() => setInstalledOnly(value => !value)}><Check size={16} /></button>
              <div className="view-select-wrap">
                <button className="view-select" aria-label="Choose library view" aria-haspopup="menu" aria-expanded={viewMenuOpen} onClick={() => { setSortOpen(false); setViewMenuOpen(value => !value); }}>{view === "grid" ? <Grid2X2 size={16} /> : <List size={17} />}<ChevronDown size={14} /></button>
                {viewMenuOpen && <div className="view-menu" role="menu" aria-label="Library view"><button role="menuitemradio" aria-checked={view === "grid"} onClick={() => { setView("grid"); setViewMenuOpen(false); }}>Grid{view === "grid" && <Check size={14} />}</button><button role="menuitemradio" aria-checked={view === "list"} onClick={() => { setView("list"); setViewMenuOpen(false); }}>List{view === "list" && <Check size={14} />}</button></div>}
              </div>
              <div className="sort-wrap">
                <button className="sort-button" aria-haspopup="menu" aria-expanded={sortOpen} onClick={() => { setViewMenuOpen(false); setSortOpen(value => !value); }}><Settings2 size={14} /><span>Filters &amp; sort{activeLibraryFilterCount ? ` · ${activeLibraryFilterCount}` : ""}</span><ChevronDown size={13} /></button>
                {sortOpen && <div className="sort-menu"><span className="sort-menu-heading">SORT PROFILES</span>{["Recently added", "Recently played", "Favorites first", "Installed first", "Minecraft version", "Loader", "Group", "Java runtime", "Name A–Z"].map(label => <button key={label} onClick={() => { setSortLabel(label); setSortOpen(false); }}>{label}{sortLabel === label && <Check size={13} />}</button>)}
                  <span className="sort-menu-heading sort-menu-heading-spaced">FILTER BY GROUP</span>
                  <FilterMenuOption label="All groups" count={profiles.length} active={groupFilter === "all"} onClick={() => setGroupFilter("all")} />
                  <FilterMenuOption label="Ungrouped" count={profiles.filter(profile => !(profile.groupName ?? "").trim()).length} active={groupFilter === "__ungrouped"} onClick={() => setGroupFilter("__ungrouped")} />
                  {profileGroups.map(group => <FilterMenuOption key={group} label={group} count={profiles.filter(profile => (profile.groupName ?? "").trim() === group).length} active={groupFilter === group} onClick={() => setGroupFilter(group)} />)}
                  <span className="sort-menu-heading sort-menu-heading-spaced">FILTER BY LOADER</span>
                  {[{ id: "all", label: "All loaders" }, ...(["vanilla", "fabric", "quilt", "forge", "neoforge"] as const).map(id => ({ id, label: id === "neoforge" ? "NeoForge" : id[0].toUpperCase() + id.slice(1) }))].map(option => <FilterMenuOption key={option.id} label={option.label} count={option.id === "all" ? profiles.length : profiles.filter(profile => profile.loaderType === option.id).length} active={loaderFilter === option.id} onClick={() => setLoaderFilter(option.id)} />)}
                  <span className="sort-menu-heading sort-menu-heading-spaced">FILTER BY STATUS</span>
                  {[{ id: "all", label: "Any install state" }, { id: "installed", label: "Installed" }, { id: "not-installed", label: "Not installed" }, { id: "attention", label: "Needs attention" }].map(option => <FilterMenuOption key={option.id} label={option.label} count={option.id === "all" ? profiles.length : option.id === "installed" ? profiles.filter(profile => profile.installed).length : option.id === "not-installed" ? profiles.filter(profile => !profile.installed).length : profiles.filter(profile => ["RepairRequired", "UpdateAvailable"].includes(resolvedProfileStates[profile.id] ?? "") || (profile.clientReady && !profile.installed)).length} active={statusFilter === option.id} onClick={() => setStatusFilter(option.id)} />)}
                  <span className="sort-menu-heading sort-menu-heading-spaced">FILTER BY RUNTIME</span>
                  {[{ id: "all", label: "Any Java" }, { id: "21", label: "Java 21" }, { id: "25", label: "Java 25" }].map(option => <FilterMenuOption key={option.id} label={option.label} count={option.id === "all" ? profiles.length : profiles.filter(profile => String(requiredJavaMajor(profile)) === option.id).length} active={javaFilter === option.id} onClick={() => setJavaFilter(option.id)} />)}
                  <span className="sort-menu-heading sort-menu-heading-spaced">CLIENT</span>
                  {[{ id: "all", label: "SNine & Vanilla" }, { id: "snine", label: "SNineClient" }, { id: "vanilla", label: "Vanilla" }].map(option => <FilterMenuOption key={option.id} label={option.label} count={option.id === "all" ? profiles.length : profiles.filter(profile => option.id === "snine" ? profile.snineclientEnabled : !profile.snineclientEnabled).length} active={clientFilter === option.id} onClick={() => setClientFilter(option.id)} />)}
                </div>}
              </div>
            </div>
          </div>
          {visibleProfiles.length === 0 ? <div className="empty-library"><div className="empty-symbol"><Search size={19} /></div><strong>{profiles.length === 0 ? "Create your first profile" : "No profiles match these filters"}</strong><p>{profiles.length === 0 ? "Set up a Minecraft version and loader to get started." : "Clear your search and filters to see your full library."}</p>{profiles.length === 0 ? <button className="secondary-action" onClick={() => void addProfile()}><Plus size={14} /> New profile</button> : <button className="secondary-action" onClick={() => { setSearch(""); setInstalledOnly(false); setGroupFilter("all"); setLoaderFilter("all"); setStatusFilter("all"); setJavaFilter("all"); setClientFilter("all"); setLibraryTab("Profiles"); setSortLabel("Recently added"); }}>Reset filters</button>}</div> :
              <div className={"profile-grid " + (view === "list" ? "list-view" : "")}>
                {visibleProfiles.map(profile => <ProfileCard key={profile.id} profile={profile} listView={view === "list"} chosen={profile.id === selected?.id} favorite={profile.favorite} hidden={profile.hidden} installing={installingIds.includes(profile.id) || profileActionIds.includes(profile.id)} playing={playingIds.includes(profile.id)} runtimeStatus={runtimeStatus} resolvedState={resolvedProfileStates[profile.id]} onOpen={item => void selectProfile(item)} onToggleFavorite={toggleFavorite} onToggleHidden={toggleHidden} onInstall={item => void installClient(item)} onPlay={item => void playProfile(item)} onRepair={item => void runProfileMaintenance(item, "repair")} onUpdate={item => void runProfileMaintenance(item, "update")} onCheckUpdate={item => void checkProfileUpdates(item)} onUninstall={item => setUninstallTarget(item)} onDuplicate={item => void duplicateProfile(item)} onOpenFolder={item => void ipc.openMinecraftProfileFolder(item.id).catch(error => showToast(String(error)))} onSetSelected={item => void selectProfile(item, false).catch(error => showToast(String(error)))} onFilterGroup={group => { setGroupFilter(group || "__ungrouped"); setLibraryTab("Profiles"); setProfileViewId(null); }} onCopyName={item => void navigator.clipboard.writeText(item.name).then(() => showToast(`Copied ${item.name}.`)).catch(() => showToast("Clipboard access is unavailable."))} />)}
              </div>}
        </> : <>
          <CosmeticsPage account={authState.account} onSignIn={() => void beginSignIn()} />
        </>}
      </main>

      <aside className="side-panel">
        {sideTab === "Downloads" ? <>
          <div className="side-tabs social-tabs" role="tablist" aria-label="Social"><button role="tab" aria-selected="false" onClick={() => setSideTab("Friends")}><Users size={16} /> Friends</button><button role="tab" aria-selected="false" onClick={() => setSideTab("Chats")}><MessageSquareText size={15} /> Chats</button></div>
          <div className="side-content"><div className="side-heading download-heading"><div><strong>Download queue</strong><span className="item-count">{activeDownloads.length}</span></div><div className="download-bulk-actions"><button type="button" disabled={activeDownloads.length === 0 || downloadBatchAction !== null} aria-label="Cancel all active downloads" title="Cancel all active downloads" onClick={() => void cancelAllDownloads()}>{downloadBatchAction === "cancel" ? <LoaderCircle size={13} className="account-spinner" /> : <X size={13} />}</button><button type="button" disabled={failedDownloads.length === 0 || downloadBatchAction !== null} aria-label="Retry all failed downloads" title="Retry all failed downloads" onClick={() => void retryAllFailedDownloads()}>{downloadBatchAction === "retry" ? <LoaderCircle size={13} className="account-spinner" /> : <RotateCcw size={13} />}</button><button type="button" disabled={completedDownloadCount === 0} aria-label="Clear completed downloads" title="Clear completed downloads" onClick={() => setDownloads(items => items.filter(item => item.state === "queued" || item.state === "downloading" || item.state === "failed"))}><Trash2 size={13} /></button></div></div>{downloads.length === 0 ? <div className="activity-empty"><div className="activity-symbol"><Download size={15} /></div><strong>Queue is clear</strong><p>Profile downloads will show transfer progress and status here.</p></div> : <div className="download-list">{downloads.slice(-8).reverse().map(item => { const percent = item.totalBytes ? Math.min(100, item.bytesReceived / item.totalBytes * 100) : 0; const active = item.state === "queued" || item.state === "downloading"; return <article className="download-row" key={item.id}><div className="download-row-top"><strong title={item.fileName}>{item.fileName}</strong><button className="download-cancel" disabled={!active && item.state !== "failed"} aria-label={active ? `Cancel ${item.fileName}` : item.state === "failed" ? `Retry ${item.fileName}` : `${item.state}`} onClick={() => item.state === "failed" ? void ipc.retryDownload(item.id).then(() => showToast("Retry queued.")).catch(error => showToast(String(error))) : active ? void ipc.cancelDownload(item.id) : undefined}>{item.state === "failed" ? <RotateCcw size={13} /> : active ? <X size={13} /> : <Check size={13} />}</button></div><small>{item.state === "downloading" ? `${formatBytes(item.bytesReceived)}${item.totalBytes ? ` / ${formatBytes(item.totalBytes)}` : ""} · ${formatBytes(item.bytesPerSecond)}/s${item.etaSeconds ? ` · ${formatDuration(item.etaSeconds)}` : ""}` : item.state === "failed" ? item.error ?? "Download failed" : item.state}</small><div className="download-progress-track"><i style={{ width: `${percent}%` }} /></div></article>; })}</div>}<div className="download-policy"><span>TRANSFER SETTINGS</span><strong>Up to 4 simultaneous files</strong><small>Resumes partial files when the server supports byte ranges.</small></div></div>
        </> : <SocialPanel tab={sideTab} onTabChange={setSideTab} account={authState.account} activity={activity} profiles={profiles} onSelectProfile={profile => void selectProfile(profile)} onSignIn={() => void beginSignIn()} />}
      </aside>
    </div>

    <footer className="bottom-bar">
      <div className="recent-wrapper" ref={recentRef}>
      <button className="recent-button" aria-haspopup="menu" aria-expanded={recentMenuOpen} onClick={() => setRecentMenuOpen(value => !value)}><History size={16} /><span><strong>{selected?.name ?? "Recent profiles"}</strong></span><ChevronDown size={14} /></button>
        {recentMenuOpen && <div className="recent-popover" role="menu" aria-label="Recent profiles"><strong>Recent profiles</strong>{recentIds.map(id => profiles.find(profile => profile.id === id)).filter((profile): profile is Profile => !!profile).slice(0, 6).map(profile => <button type="button" role="menuitem" key={profile.id} onClick={() => { setRecentMenuOpen(false); void selectProfile(profile); }}><ProfileCover profile={profile} className="recent-thumb" /><span><b>{profile.name}</b><small>{profile.installed ? "Installed" : "Not installed"}</small></span>{profile.id === selected?.id && <Check size={14} />}</button>)}</div>}
      </div>
      <button className={"bottom-downloads " + (sideTab === "Downloads" ? "active" : "")} onClick={() => setSideTab("Downloads")} aria-label="Open downloads">
        <SVGRepoIcon src={downloadArrowSvg} />
        <span className="bottom-download-copy"><strong>{activeDownloads.some(item => item.state === "downloading") ? "Downloading" : activeDownloads.length ? "Queued" : "Downloads"}</strong><span className="bottom-download-track"><i style={{ width: `${downloadProgress}%` }} /></span></span>
        <span className="bottom-download-percent">{Math.round(downloadProgress)}%</span>
      </button>
      <div className="bottom-spacer" /><span className="version-label">{LAUNCHER_VERSION} <i>(Windows)</i></span>
      <button className="feedback-button" onClick={() => setFeedbackOpen(true)}><MessageSquareText size={13} /> Feedback</button>
    </footer>
    {commandPaletteOpen && <div className="command-palette-scrim" onClick={() => setCommandPaletteOpen(false)}><section className="command-palette" role="dialog" aria-modal="true" aria-label="Quick actions" onClick={event => event.stopPropagation()}>
      <div className="command-palette-search"><Search size={18} /><input ref={commandInputRef} value={commandQuery} onChange={event => { setCommandQuery(event.target.value); setCommandSelection(0); }} onKeyDown={event => {
        if (event.key === "ArrowDown") { event.preventDefault(); setCommandSelection(index => Math.min(filteredLauncherCommands.length - 1, index + 1)); }
        else if (event.key === "ArrowUp") { event.preventDefault(); setCommandSelection(index => Math.max(0, index - 1)); }
        else if (event.key === "Enter" && filteredLauncherCommands[commandSelection]) { event.preventDefault(); executeLauncherCommand(filteredLauncherCommands[commandSelection]); }
        else if (event.key === "Escape") { event.preventDefault(); setCommandPaletteOpen(false); }
      }} placeholder="Search profiles or run an action…" aria-label="Search quick actions" /><kbd>ESC</kbd></div>
      <div className="command-palette-heading"><span>{commandQuery.trim() ? "SEARCH RESULTS" : "QUICK ACTIONS & PROFILES"}</span><span>{filteredLauncherCommands.length} results</span></div>
      <div className="command-palette-results" role="listbox" aria-label="Quick actions and profiles">{filteredLauncherCommands.length === 0 ? <div className="command-palette-empty"><Search size={18} /><strong>No matching actions</strong><small>Try another profile name or command.</small></div> : filteredLauncherCommands.map((command, index) => <button key={command.id} role="option" aria-selected={index === commandSelection} className={index === commandSelection ? "selected" : ""} onMouseEnter={() => setCommandSelection(index)} onClick={() => executeLauncherCommand(command)}><span className="command-result-icon">{command.icon}</span><span className="command-result-copy"><strong>{command.label}</strong><small>{command.description}</small></span>{index === commandSelection && <kbd>↵</kbd>}</button>)}</div>
      <footer className="command-palette-footer"><span><kbd>↑</kbd><kbd>↓</kbd> Navigate</span><span><kbd>↵</kbd> Run action</span><span><kbd>ESC</kbd> Close</span></footer>
    </section></div>}
    {settingsOpen && <div className="modal-scrim settings-scrim" onClick={() => setSettingsOpen(false)}><section className="settings-window" role="dialog" aria-modal="true" aria-labelledby="settings-heading" onClick={event => event.stopPropagation()}>
      <header className="settings-window-header"><div className="settings-title-icon"><SVGRepoIcon src={settingsGearSvg} /></div><div className="settings-window-title"><small>SNINE LAUNCHER · PREFERENCES</small><h2 id="settings-heading">Settings</h2><p>Make the launcher feel like yours.</p></div><button className="settings-close" aria-label="Close settings" onClick={() => setSettingsOpen(false)}><X size={17} /></button></header>
      <div className="settings-window-body"><nav className="settings-navigation" aria-label="Settings sections"><small className="settings-nav-label">PREFERENCES</small>
        <button className={settingsTab === "Appearance" ? "active" : ""} onClick={() => setSettingsTab("Appearance")}><Sparkles size={16} /><span>Appearance</span></button>
        <button className={settingsTab === "Minecraft" ? "active" : ""} onClick={() => setSettingsTab("Minecraft")}><SVGRepoIcon src={gamepadSvg} /><span>Minecraft</span></button>
        <button className={settingsTab === "Downloads" ? "active" : ""} onClick={() => setSettingsTab("Downloads")}><SVGRepoIcon src={downloadArrowSvg} /><span>Downloads</span></button>
        <button className={settingsTab === "Storage" ? "active" : ""} onClick={() => setSettingsTab("Storage")}><SVGRepoIcon src={databaseStorageSvg} /><span>Storage</span></button>
        <div className="settings-nav-note"><i /><span><strong>Saved automatically</strong><small>Your choices stay on this device.</small></span></div>
      </nav><main className="settings-page-content">
        {settingsTab === "Appearance" && <section className="settings-section"><div className="settings-page-heading"><small>LOOK & FEEL</small><h3>Shape your launcher</h3><p>Choose how soft or sharp the launcher surfaces feel. This applies instantly across the whole app.</p></div><article className="settings-card corner-card"><div className="settings-card-heading"><span><strong>Rounded corners</strong><small>Panels, cards and controls</small></span><span className="live-setting-badge"><i /> LIVE</span></div><div className="corner-style-options" role="radiogroup" aria-label="Rounded corners">{([{ id: "square", label: "Square", detail: "Crisp edges" }, { id: "balanced", label: "Subtle", detail: "A little softness" }, { id: "rounded", label: "Rounded", detail: "Smooth corners" }] as const).map(option => <button key={option.id} role="radio" aria-checked={cornerStyle === option.id} className={cornerStyle === option.id ? "selected" : ""} onClick={() => chooseCornerStyle(option.id)}><i className={"corner-preview " + option.id} /><strong>{option.label}</strong><small>{option.detail}</small></button>)}</div></article><div className="settings-tip"><Sparkles size={15} /><span><strong>Made to fit your style</strong><small>Preview each corner setting instantly and keep the one that feels right.</small></span></div></section>}
        {settingsTab === "Minecraft" && <section className="settings-section"><div className="settings-page-heading"><small>GAME CONFIGURATION</small><h3>Minecraft</h3><p>Set the memory and window behavior used when you launch a profile.</p></div><article className="settings-card"><div className="settings-card-heading"><span><strong>Memory allocation</strong><small>Maximum Java heap on the next launch</small></span><span className="settings-inline-control"><input aria-label="Minecraft memory" type="number" min={2048} max={32768} step={512} value={minecraftMemoryMb} onChange={event => setMinecraftMemoryMb(Number(event.target.value))} onBlur={() => { if (Number.isInteger(minecraftMemoryMb) && minecraftMemoryMb >= 2048 && minecraftMemoryMb <= 32768 && minecraftMemoryMb % 512 === 0) void saveMinecraftMemory(); }} /><small>MB</small></span></div><div className="settings-card-divider" /><div className="settings-card-heading"><span><strong>Game window</strong><small>Applied the next time Minecraft starts</small></span><label className="settings-toggle"><span>Fullscreen</span><input type="checkbox" checked={windowSettings.fullscreen} onChange={event => setWindowSettings(value => ({ ...value, fullscreen: event.target.checked }))} /></label></div><div className="window-dimension-grid"><label>Width<input type="number" min={640} max={7680} value={windowSettings.width} disabled={windowSettings.fullscreen} onChange={event => setWindowSettings(value => ({ ...value, width: Number(event.target.value) }))} /></label><label>Height<input type="number" min={480} max={4320} value={windowSettings.height} disabled={windowSettings.fullscreen} onChange={event => setWindowSettings(value => ({ ...value, height: Number(event.target.value) }))} /></label></div><div className="window-presets">{[[1280,720,"Default"],[1280,720,"720p"],[1920,1080,"1080p"],[2560,1440,"1440p"],[3840,2160,"4K"]].map(([width,height,label]) => <button key={label} onClick={() => setWindowSettings(value => ({ ...value, width: Number(width), height: Number(height), fullscreen: false }))}>{label}</button>)}</div><button className="settings-save-button" onClick={() => void saveWindowSettings()}>Save window settings</button></article><article className="settings-card settings-check-row"><span><strong>Minimize when the game starts</strong><small>Only minimize after Minecraft has launched successfully.</small></span><input type="checkbox" checked={minimizeOnGameStart} onChange={event => void saveMinimizeOnGameStart(event.target.checked)} /></article><div className="settings-tip"><Gamepad2 size={15} /><span><strong>Automatic Java selection</strong><small>The launcher detects the Java version required by each Minecraft release and supports managed Java 21 and 25 runtimes.</small></span></div></section>}
        {settingsTab === "Downloads" && <section className="settings-section"><div className="settings-page-heading"><small>TRANSFER PREFERENCES</small><h3>Downloads</h3><p>Control how many files the launcher transfers at the same time.</p></div><article className="settings-card"><div className="settings-card-heading"><span><strong>Download concurrency</strong><small>Maximum simultaneous transfers</small></span><span className="settings-inline-control"><input aria-label="Download concurrency" type="number" min={1} max={16} step={1} value={downloadConcurrency} onChange={event => setDownloadConcurrency(Number(event.target.value))} onBlur={() => { if (Number.isInteger(downloadConcurrency) && downloadConcurrency >= 1 && downloadConcurrency <= 16) void saveDownloadConcurrency(downloadConcurrency); }} /><small>jobs</small></span></div><p className="settings-card-note">More jobs can speed up large installs, but use more of your connection. Choose between 1 and 16.</p></article></section>}
        {settingsTab === "Storage" && <section className="settings-section"><div className="settings-page-heading"><small>LOCAL FILES</small><h3>Storage</h3><p>See how much space the launcher and Minecraft content currently use.</p></div><article className="settings-card storage-card"><div className="settings-card-heading"><span><strong>Disk usage</strong><small>Launcher-managed files on this device</small></span><button className="settings-refresh-button" onClick={() => void ipc.getStorageInfo().then(setStorageInfo).catch(error => showToast(String(error)))}><RotateCcw size={14} /> Refresh</button></div><div className="storage-summary"><span>Profile instances <b>{storageInfo ? formatBytes(storageInfo.profileInstancesBytes) : "Calculating…"}</b></span><span>Shared libraries <b>{storageInfo ? formatBytes(storageInfo.librariesBytes) : "Calculating…"}</b></span><span>Minecraft assets <b>{storageInfo ? formatBytes(storageInfo.assetsBytes) : "Calculating…"}</b></span><span>Managed runtimes <b>{storageInfo ? formatBytes(storageInfo.runtimesBytes) : "Calculating…"}</b></span><span>Download cache <b>{storageInfo ? formatBytes(storageInfo.downloadCacheBytes) : "Calculating…"}</b></span></div></article></section>}
      </main></div><footer className="settings-window-footer"><span><i /> {LAUNCHER_VERSION} · WINDOWS</span><button onClick={() => setSettingsOpen(false)}>Done</button></footer>
    </section></div>}
    {launcherUpdate && <div className="modal-scrim" onClick={() => { if (!launcherUpdateBusy) dismissLauncherUpdate(); }}><section className="settings-popover feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="launcher-update-heading" onClick={event => event.stopPropagation()}><div><div><small>SNINE LAUNCHER · UPDATE</small><h2 id="launcher-update-heading">Version {launcherUpdate.version} is ready</h2></div><button className="subtle-icon" aria-label="Close update prompt" disabled={launcherUpdateBusy} onClick={dismissLauncherUpdate}><X size={16} /></button></div><p>{launcherUpdate.body || "A signed launcher update is available. Install it now?"}</p>{launcherUpdateProgress && <p role="status">{launcherUpdateProgress}</p>}<div className="uninstall-actions"><button className="secondary-action" disabled={launcherUpdateBusy} onClick={dismissLauncherUpdate}>Later</button><button className="compact-play" disabled={launcherUpdateBusy} onClick={() => void installLauncherUpdate()}>{launcherUpdateBusy ? launcherUpdateProgress || "Updating…" : "Update and restart"}</button></div></section></div>}
    {feedbackOpen && <div className="modal-scrim" onClick={() => setFeedbackOpen(false)}><section className="settings-popover feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="feedback-heading" onClick={event => event.stopPropagation()}><div><div><small>SNINE LAUNCHER · {LAUNCHER_VERSION}</small><h2 id="feedback-heading">Give feedback</h2></div><button className="subtle-icon" aria-label="Close feedback" onClick={() => setFeedbackOpen(false)}><X size={16} /></button></div><p>Write your feedback, then copy it to share with the SNine team.</p><textarea aria-label="Feedback" value={feedbackText} onChange={event => setFeedbackText(event.target.value)} placeholder="What should we improve?" rows={5} /><div className="uninstall-actions"><button className="secondary-action" onClick={() => setFeedbackOpen(false)}>Cancel</button><button className="compact-play" disabled={!feedbackText.trim()} onClick={() => { void navigator.clipboard.writeText(`[SNine Launcher ${LAUNCHER_VERSION}]\n${feedbackText.trim()}`).then(() => { setFeedbackOpen(false); setFeedbackText(""); showToast("Feedback copied to clipboard."); }).catch(() => showToast("Clipboard access is unavailable.")); }}>Copy feedback</button></div></section></div>}
    {createProfileOpen && <ProfileCreationForm
      name={newProfileName} onNameChange={setNewProfileName}
      minecraftVersion={newMinecraftVersion} onMinecraftVersionChange={setNewMinecraftVersion}
      versions={minecraftVersions} showSnapshots={showSnapshots} onShowSnapshotsChange={setShowSnapshots}
      loaderType={newLoaderType} onLoaderTypeChange={value => { setNewLoaderType(value); if (value !== "fabric") setNewSnineEnabled(false); }}
      availableLoaders={availableLoaderTypes}
      loaderVersion={newLoaderVersion} loaderVersions={fabricLoaderVersions} onLoaderVersionChange={setNewLoaderVersion}
      includeSnineClient={newSnineEnabled} onIncludeSnineClientChange={setNewSnineEnabled}
      memory={newProfileMemoryMb} onMemoryChange={setNewProfileMemoryMb}
      iconId={newProfileIconId} iconPreview={newProfileIconPreview} onChooseIcon={() => setIconPickerTarget({ kind: "create", version: newMinecraftVersion })}
      onCreate={() => void createProfile()} onCancel={() => setCreateProfileOpen(false)} />}
    <BlockIconPicker open={!!iconPickerTarget} minecraftVersion={iconPickerTarget?.version ?? ""}
      selectedIconId={iconPickerTarget?.kind === "create" ? newProfileIconId : profiles.find(profile => profile.id === (iconPickerTarget?.kind === "profile" ? iconPickerTarget.id : ""))?.iconId ?? null}
      selectedPreview={iconPickerTarget?.kind === "create" ? newProfileIconPreview : null}
      onSelect={entry => void chooseProfileIcon(entry)} onClose={() => setIconPickerTarget(null)} />
    {uninstallTarget && <div className="modal-scrim" onClick={() => setUninstallTarget(null)}><section className="settings-popover uninstall-dialog" role="alertdialog" aria-modal="true" aria-labelledby="uninstall-heading" onClick={event => event.stopPropagation()}><div><div><small>PROFILE MANAGEMENT</small><h2 id="uninstall-heading">Uninstall {uninstallTarget.name}?</h2></div><button className="subtle-icon" aria-label="Close confirmation" onClick={() => setUninstallTarget(null)}><X size={16} /></button></div><p>This removes the managed client and profile metadata. Saves, screenshots, configurations, user mods, shared Minecraft files, and Java runtimes are kept.</p><div className="uninstall-actions"><button className="secondary-action" onClick={() => setUninstallTarget(null)}>Cancel</button><button className="destructive-action" onClick={() => void performUninstall()}>Uninstall</button></div></section></div>}
    {deleteTarget && <div className="modal-scrim" onClick={() => setDeleteTarget(null)}><section className="settings-popover uninstall-dialog" role="alertdialog" aria-modal="true" aria-labelledby="delete-profile-heading" onClick={event => event.stopPropagation()}><div><div><small>PROFILE MANAGEMENT</small><h2 id="delete-profile-heading">Delete {deleteTarget.name}?</h2></div><button className="subtle-icon" aria-label="Close confirmation" onClick={() => setDeleteTarget(null)}><X size={16} /></button></div><p>This removes the profile entry only. Installed profiles must be uninstalled first; the primary SNineClient profile is protected.</p><div className="uninstall-actions"><button className="secondary-action" onClick={() => setDeleteTarget(null)}>Cancel</button><button className="destructive-action" onClick={() => void deleteProfile()}>Delete profile</button></div></section></div>}
    {toast && <div role="status" className="toast">{toast}</div>}
  </div>;
}

function ProfileDetails({ profile, resolvedState, installing, runtimeStatus, manageInfo, onInstall, onRepair, onUpdate, onCheckUpdate, onUninstall, onDelete, onOpenFolder, onRename, onGroupChanged, onDuplicate, onChangeIcon, onBack }: { profile: Profile; resolvedState?: string; installing: boolean; runtimeStatus: RuntimeStatus | null; manageInfo: ProfileManageInfo | null; onInstall: () => void; onRepair: () => void; onUpdate: () => void; onCheckUpdate: () => void; onUninstall: () => void; onDelete: () => void; onOpenFolder: () => void; onRename: (name: string) => void; onGroupChanged: (groupName: string) => void; onDuplicate: () => void; onChangeIcon: () => void; onBack: () => void }) {
  const [editingName, setEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState(profile.name);
  const [manageTab, setManageTab] = useState<"Overview" | "Mods" | "Resource Packs" | "Game content" | "Settings">("Overview");
  const [contentInitialSection, setContentInitialSection] = useState<"shaderpacks" | "datapacks" | "worlds" | "screenshots" | "logs" | "symlinks">("shaderpacks");
  const [contentMountId, setContentMountId] = useState(0);
  const [profileMemoryInput, setProfileMemoryInput] = useState("");
  const [profileMemoryInherited, setProfileMemoryInherited] = useState(true);
  const [profileGroup, setProfileGroup] = useState(profile.groupName ?? "");
  const [settingsMessage, setSettingsMessage] = useState("");
  const [backupBusy, setBackupBusy] = useState(false);
  const [backupMessage, setBackupMessage] = useState("");
  useEffect(() => { setNameDraft(profile.name); setEditingName(false); setProfileGroup(profile.groupName ?? ""); }, [profile.id, profile.name, profile.groupName]);
  useEffect(() => { setBackupMessage(""); setBackupBusy(false); }, [profile.id]);
  useEffect(() => { setManageTab("Overview"); setSettingsMessage(""); }, [profile.id]);
  useEffect(() => {
    if (manageTab !== "Settings" || !isTauri()) return;
    let active = true;
    void ipc.getProfileMemory(profile.id).then(value => { if (active) { setProfileMemoryInherited(value === null); setProfileMemoryInput(value === null ? "" : String(value)); } }).catch(() => undefined);
    return () => { active = false; };
  }, [manageTab, profile.id]);
  const runtimeReady = runtimeStatus?.compatible !== null && runtimeStatus?.compatible !== undefined;
  const createBackup = async () => {
    if (!isTauri()) { setBackupMessage("Backups are available in the desktop launcher."); return; }
    setBackupBusy(true); setBackupMessage("");
    try {
      const destination = await ipc.chooseProfileBackupDestination(`SNine-${profile.id}-${new Date().toISOString().slice(0, 10)}.zip`);
      if (!destination) return;
      const bytes = await ipc.createProfileBackup(profile.id, destination);
      setBackupMessage(`Backup created · ${formatBytes(bytes)} of profile data.`);
    } catch (error) {
      setBackupMessage(error instanceof Error ? error.message : String(error));
    } finally { setBackupBusy(false); }
  };
  const restoreBackup = async () => {
    if (!isTauri()) { setBackupMessage("Backup restore is available in the desktop launcher."); return; }
    if (!window.confirm(`Restore a backup to ${profile.name}? This replaces all current files in this profile instance.`)) return;
    setBackupBusy(true); setBackupMessage("");
    try {
      const source = await ipc.chooseProfileBackupSource();
      if (!source) return;
      const bytes = await ipc.restoreProfileBackup(profile.id, source);
      setBackupMessage(`Backup restored · ${formatBytes(bytes)} of profile data.`);
    } catch (error) {
      setBackupMessage(error instanceof Error ? error.message : String(error));
    } finally { setBackupBusy(false); }
  };
  const actionLabel = installing ? "VERIFYING" : resolvedState === "RepairRequired" ? "REPAIR" : resolvedState === "UpdateAvailable" ? "UPDATE" : profile.installed ? "PLAY" : profile.clientReady ? runtimeStatus === null ? "CHECKING RUNTIME" : runtimeReady ? "INSTALL MINECRAFT" : `INSTALL JAVA ${requiredJavaMajor(profile)}` : "INSTALL";
  const actionDisabled = installing || (profile.clientReady && !profile.installed && runtimeStatus === null);
  const readiness = installing ? "Setting up profile" : resolvedState === "Running" ? "Minecraft is running" : resolvedState === "RepairRequired" ? "Repair needed" : resolvedState === "UpdateAvailable" ? "Update available" : profile.installed ? runtimeReady ? "Ready to play" : "Java runtime required" : profile.clientReady ? "Minecraft setup needed" : "Ready to install";
  const readinessTone = installing ? "working" : resolvedState === "RepairRequired" ? "warning" : profile.installed && runtimeReady ? "ready" : "neutral";
  const loaderLabel = profile.loaderType === "fabric" || profile.loaderType === "quilt" ? `${profile.loaderType === "quilt" ? "Quilt" : "Fabric"} ${profile.loaderVersion ?? ""}` : "Vanilla";
  return <section className="profile-detail-page">
    <div className="detail-toolbar"><button className="detail-back" onClick={onBack}><ArrowLeft size={15} /> Library</button><span className="detail-breadcrumb">Profile</span></div>
    <div className="detail-hero"><ProfileCover profile={profile} className="detail-cover" /><div className="detail-identity"><span className="detail-eyebrow">PROFILE</span>{editingName ? <form className="profile-rename-form" onSubmit={event => { event.preventDefault(); onRename(nameDraft); setEditingName(false); }}><input autoFocus maxLength={48} aria-label="Profile name" value={nameDraft} onChange={event => setNameDraft(event.target.value)} /><button className="subtle-icon" aria-label="Save profile name" disabled={!nameDraft.trim()}><Check size={15} /></button><button type="button" className="subtle-icon" aria-label="Cancel rename" onClick={() => { setNameDraft(profile.name); setEditingName(false); }}><X size={15} /></button></form> : <div className="profile-title-edit"><h1>{profile.name}</h1><button className="subtle-icon" aria-label="Rename profile" title="Rename profile" onClick={() => setEditingName(true)}><Pencil size={14} /></button></div>}<p>Minecraft {profile.minecraftVersion} · {loaderLabel} · SNineClient {profile.snineclientEnabled ? "On" : "Off"}</p><div className="detail-hero-actions"><button type="button" className="secondary-action" onClick={onChangeIcon}>Change icon</button><button className="compact-play" disabled={actionDisabled || installing} title={profile.clientReady && runtimeReady && !profile.installed ? "Install Minecraft and Fabric before playing." : undefined} onClick={resolvedState === "RepairRequired" ? onRepair : resolvedState === "UpdateAvailable" ? onUpdate : onInstall}>{profile.installed && resolvedState !== "RepairRequired" && resolvedState !== "UpdateAvailable" ? <Gamepad2 size={15} /> : <Download size={15} />}{actionLabel}</button></div></div><aside className="detail-hero-status"><span className={`detail-status-pill ${readinessTone}`}><i />{readiness}</span><strong>{profile.installed ? "Installation" : "Profile setup"}</strong><small>Minecraft {profile.minecraftVersion}</small><small>{loaderLabel}{profile.snineclientEnabled ? " · SNineClient" : ""}</small><small>{runtimeStatus ? `Java ${runtimeStatus.requiredMajor}${runtimeReady ? " ready" : " needed"}` : "Checking Java runtime…"}</small></aside></div>
    <div className="profile-manage-tabs" role="tablist" aria-label="Profile management">{(["Overview", "Mods", "Resource Packs", "Game content", "Settings"] as const).map(tab => <button key={tab} role="tab" aria-selected={manageTab === tab} className={manageTab === tab ? "active" : ""} onClick={() => { if (tab === "Game content") { setContentInitialSection("shaderpacks"); setContentMountId(value => value + 1); } setManageTab(tab); }}>{tab}</button>)}</div>
    {manageTab === "Overview" && <div className="detail-overview"><div className="detail-section-heading"><div><span>PROFILE</span><h2>Overview</h2></div><div className="manage-action-row"><button className="secondary-action" disabled={backupBusy || (!profile.installed && !profile.clientReady)} onClick={() => void createBackup()}><Download size={14} />{backupBusy ? "Creating backup…" : "Create backup"}</button><button className="secondary-action" disabled={backupBusy || (!profile.installed && !profile.clientReady)} onClick={() => void restoreBackup()}><History size={14} />Restore backup</button><button className="secondary-action" onClick={onDuplicate}><Copy size={14} /> Duplicate</button></div></div><div className="detail-info-grid"><div><span>Status</span><strong><i className={profile.clientReady ? "status-dot online" : "status-dot"} />{profile.clientReady ? runtimeReady ? "Client and Java ready · Minecraft/loader setup required" : `Client installed · Java ${requiredJavaMajor(profile)} required` : "Ready to install"}</strong></div><div><span>Client version</span><strong>{manageInfo?.clientVersion ?? profile.clientVersion}</strong></div><div><span>Minecraft version</span><strong>{manageInfo?.minecraftVersion ?? profile.minecraftVersion}</strong></div><div><span>Java runtime</span><strong>{manageInfo?.runtime ?? (runtimeStatus?.compatible ? `${runtimeStatus.compatible.detail} ready${runtimeStatus.managed ? " · managed" : ""}` : runtimeStatus?.candidates.length ? `Java ${runtimeStatus.candidates[0].major ?? "?"} found · Java ${runtimeStatus.requiredMajor} required` : "Java runtime not found")}</strong></div></div>{backupMessage && <p className="profile-mod-note" role="status">{backupMessage}</p>}</div>}
    {manageTab === "Mods" && <ProfileModsPage key={profile.id} profileId={profile.id} minecraftVersion={manageInfo?.minecraftVersion ?? profile.minecraftVersion} loaderType={profile.loaderType} installed={profile.installed} />}
    {manageTab === "Resource Packs" && <ProfileResourcePacksPage key={profile.id} profileId={profile.id} minecraftVersion={manageInfo?.minecraftVersion ?? profile.minecraftVersion} installed={profile.installed} />}
    {manageTab === "Settings" && <ProfileGroupEditor profileId={profile.id} groupName={profileGroup} onChange={setProfileGroup} onSaved={onGroupChanged} />}
    {manageTab === "Game content" && <ProfileContentPage key={`${profile.id}-${contentMountId}`} profileId={profile.id} installed={profile.installed} running={resolvedState === "Running"} initialSection={contentInitialSection} />}
    {manageTab === "Settings" && <div className="detail-overview"><div className="detail-section-heading"><div><span>PROFILE</span><h2>Settings</h2></div></div><div className="manage-info-row"><span>Minecraft version</span><strong>{manageInfo?.minecraftVersion ?? profile.minecraftVersion}</strong></div><div className="manage-info-row"><span>Loader</span><strong>{profile.loaderType === "fabric" || profile.loaderType === "quilt" ? `${profile.loaderType === "quilt" ? "Quilt" : "Fabric"} ${manageInfo?.loaderVersion ?? "managed by launcher"}` : profile.loaderType}</strong></div><div className="manage-info-row"><span>SNineClient</span><strong>{profile.snineclientEnabled ? profile.clientVersion : "Not included"}</strong></div><label className="profile-memory-setting"><span>Profile memory limit <small>{profileMemoryInherited ? "Using global default" : "Applied on next launch"}</small></span><span><input disabled={!isTauri()} type="number" min={2048} max={32768} step={512} value={profileMemoryInput} placeholder="Global" onChange={event => setProfileMemoryInput(event.target.value)} onBlur={() => { const value = Number(profileMemoryInput); if (Number.isInteger(value) && value >= 2048 && value <= 32768 && value % 512 === 0 && isTauri()) void ipc.setProfileMemory(profile.id, value).then(() => { setProfileMemoryInherited(false); setSettingsMessage("Memory setting saved."); }).catch(error => setSettingsMessage(String(error))); }} /> MB</span></label><button className="secondary-action" disabled={!isTauri()} onClick={() => void ipc.setProfileMemory(profile.id, null).then(() => { setProfileMemoryInput(""); setProfileMemoryInherited(true); setSettingsMessage("Using the launcher memory default."); }).catch(error => setSettingsMessage(String(error)))}>Use global memory default</button>{settingsMessage && <p className="profile-mod-note" role="status">{settingsMessage}</p>}<p>Version and loader changes stay unavailable until a compatible install provider is active.</p><div className="manage-action-row"><button className="secondary-action" onClick={onOpenFolder}><ExternalLink size={14} /> Open profile folder</button>{!profile.installed && !profile.clientReady && profile.id !== "snineclient-default" && <button className="destructive-action" onClick={onDelete}><X size={14} /> Delete profile</button>}</div></div>}
    {profile.installed && <div className="detail-overview manage-actions"><div className="detail-section-heading"><div><span>MANAGED INSTALL</span><h2>Manage</h2></div></div><div className="manage-info-row"><span>{profile.loaderType === "fabric" ? "Fabric Loader" : profile.loaderType === "quilt" ? "Quilt Loader" : "Loader"}</span><strong>{manageInfo?.loaderVersion ?? "Reading install metadata…"}</strong></div><div className="manage-info-row"><span>Profile storage</span><strong>{manageInfo ? formatBytes(manageInfo.storageBytes) : "Calculating…"}</strong></div><div className="manage-info-row"><span>Location</span><strong className="manage-location" title={manageInfo?.directory}>{manageInfo?.directory ?? "Managed profile folder"}</strong></div><div className="manage-action-row"><button className="secondary-action" onClick={onOpenFolder}><ExternalLink size={14} /> Open folder</button><button className="secondary-action" onClick={() => { setContentInitialSection("logs"); setContentMountId(value => value + 1); setManageTab("Game content"); }}><FileText size={14} /> View game logs</button><button className="secondary-action" onClick={onCheckUpdate}><Download size={14} /> Check updates</button><button className="secondary-action" disabled={installing} onClick={onRepair}><Settings2 size={14} /> Verify / Repair</button><button className="destructive-action" disabled={installing} onClick={onUninstall}><X size={14} /> Uninstall</button></div></div>}
  </section>;
}

function ProfileGroupEditor({ profileId, groupName, onChange, onSaved }: { profileId: string; groupName: string; onChange: (value: string) => void; onSaved: (value: string) => void }) {
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const save = async () => {
    if (!isTauri()) return;
    setSaving(true); setStatus("");
    try { await ipc.setProfileGroup(profileId, groupName); onSaved(groupName.trim()); setStatus("Group saved."); }
    catch (error) { setStatus(error instanceof Error ? error.message : String(error)); }
    finally { setSaving(false); }
  };
  return <div className="detail-overview"><div className="detail-section-heading"><div><span>PROFILE ORGANIZATION</span><h2>Group</h2></div></div><label className="profile-memory-setting"><span>Profile group <small>A label for organizing this profile.</small></span><span><input type="text" maxLength={48} value={groupName} placeholder="No group" onChange={event => onChange(event.target.value)} /></span></label><button className="secondary-action" disabled={saving || !isTauri()} onClick={() => void save()}>{saving ? "Saving…" : "Save group"}</button>{status && <p className="profile-mod-note" role="status">{status}</p>}</div>;
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let amount = value;
  let unit = -1;
  do { amount /= 1024; unit += 1; } while (amount >= 1024 && unit < units.length - 1);
  return `${amount.toFixed(amount < 10 ? 1 : 0)} ${units[unit]}`;
}

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return minutes > 0 ? `${minutes}m ${seconds % 60}s` : `${seconds}s`;
}


function readIds(key: string): string[] {
  try {
    const value = window.localStorage.getItem(key);
    const parsed: unknown = value ? JSON.parse(value) : [];
    return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === "string") : [];
  } catch { return []; }
}

function saveIds(key: string, ids: string[]): void {
  try { window.localStorage.setItem(key, JSON.stringify(ids)); } catch { /* local persistence is best-effort */ }
}

function readLibraryPreferences(): LibraryPreferences {
  const defaults: LibraryPreferences = { tab: "Clients", view: "list", sort: "Recently added", installedOnly: false, groupFilter: "all", loaderFilter: "all", statusFilter: "all", javaFilter: "all", clientFilter: "all" };
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(LIBRARY_PREFS_KEY) ?? "null");
    if (!parsed || typeof parsed !== "object") return defaults;
    const value = parsed as Partial<LibraryPreferences>;
    return {
      tab: value.tab === "Profiles" || value.tab === "Favorites" ? value.tab : "Clients",
      view: value.view === "grid" ? "grid" : "list",
      sort: ["Recently added", "Recently played", "Favorites first", "Installed first", "Minecraft version", "Loader", "Group", "Java runtime", "Name A–Z"].includes(value.sort ?? "") ? value.sort! : defaults.sort,
      installedOnly: value.installedOnly === true,
      groupFilter: typeof value.groupFilter === "string" ? value.groupFilter : defaults.groupFilter,
      loaderFilter: ["all", "vanilla", "fabric", "quilt", "forge", "neoforge"].includes(value.loaderFilter ?? "") ? value.loaderFilter! : defaults.loaderFilter,
      statusFilter: ["all", "installed", "not-installed", "attention"].includes(value.statusFilter ?? "") ? value.statusFilter! : defaults.statusFilter,
      javaFilter: ["all", "21", "25"].includes(value.javaFilter ?? "") ? value.javaFilter! : defaults.javaFilter,
      clientFilter: ["all", "snine", "vanilla"].includes(value.clientFilter ?? "") ? value.clientFilter! : defaults.clientFilter,
    };
  } catch { return defaults; }
}

function FilterMenuOption({ label, count, active, onClick }: { label: string; count: number; active: boolean; onClick: () => void }) {
  return <button className="library-filter-option" aria-pressed={active} onClick={onClick}><span>{label}</span><small>{count}</small>{active && <Check size={13} />}</button>;
}

function saveLibraryPreferences(preferences: LibraryPreferences): void {
  try { window.localStorage.setItem(LIBRARY_PREFS_KEY, JSON.stringify(preferences)); } catch { /* best-effort preference persistence */ }
}

function readCornerStyle(): CornerStyle {
  try {
    const saved = window.localStorage.getItem(CORNER_STYLE_KEY);
    if (saved === "square" || saved === "balanced" || saved === "rounded") return saved;
  } catch { /* use the default if local preferences are unavailable */ }
  return "balanced";
}

