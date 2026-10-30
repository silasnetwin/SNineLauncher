import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, Copy, CopyPlus, Download, FolderOpen, Heart, ListFilter, MoreHorizontal, Play, Settings2, X } from "lucide-react";
import type { Profile, RuntimeStatus } from "../lib/ipc";
import { ipc } from "../lib/ipc";
import snineLauncherIcon from "../assets/brand/snine-launcher-icon.png";

const LATEST_ICON_VERSION = "latest";
const DEFAULT_MINECRAFT_ICON = "minecraft:grass_block";
const iconPreviewCache = new Map<string, string>();
const iconPreviewRequests = new Map<string, Promise<string | null>>();

export const ProfileCover = memo(function ProfileCover({ profile, className = "", artwork }: { profile: Profile; className?: string; artwork?: string }) {
  const iconId = profile.iconId ?? (profile.snineclientEnabled ? null : DEFAULT_MINECRAFT_ICON);
  const key = iconId ? `${LATEST_ICON_VERSION}:${iconId}` : "snine-logo";
  const [iconPreview, setIconPreview] = useState<string | null>(() => iconId ? iconPreviewCache.get(key) ?? null : null);
  useEffect(() => {
    if (!iconId || !("__TAURI_INTERNALS__" in window)) return;
    let active = true;
    const cached = iconPreviewCache.get(key);
    if (cached) { setIconPreview(cached); return () => { active = false; }; }
    let request = iconPreviewRequests.get(key);
    if (!request) {
      request = ipc.getBlockIconCatalog(LATEST_ICON_VERSION, iconId, 0, 1)
        .then(items => items.find(item => item.id === iconId)?.previewDataUrl ?? null)
        .then(preview => { if (preview) iconPreviewCache.set(key, preview); return preview; })
        .finally(() => iconPreviewRequests.delete(key));
      iconPreviewRequests.set(key, request);
    }
    void request.then(preview => { if (active && preview) setIconPreview(preview); }).catch(() => undefined);
    return () => { active = false; };
  }, [iconId, key]);
  return <span className={`cover-art ${className}`}><img className={!artwork && iconPreview && iconId ? "pixel-icon" : undefined} src={artwork ?? iconPreview ?? snineLauncherIcon} alt="" loading="lazy" decoding="async" draggable="false" /></span>;
});

export type ProfileCardProps = {
  profile: Profile;
  listView: boolean;
  chosen: boolean;
  favorite: boolean;
  hidden: boolean;
  onOpen: (profile: Profile) => void;
  onToggleFavorite: (id: string) => void;
  onToggleHidden: (id: string) => void;
  onInstall: (profile: Profile) => void;
  onPlay: (profile: Profile) => void;
  onRepair: (profile: Profile) => void;
  onUpdate: (profile: Profile) => void;
  onCheckUpdate: (profile: Profile) => void;
  onUninstall: (profile: Profile) => void;
  onDuplicate: (profile: Profile) => void;
  onOpenFolder: (profile: Profile) => void;
  onSetSelected: (profile: Profile) => void;
  onFilterGroup: (groupName: string) => void;
  onCopyName: (profile: Profile) => void;
  installing: boolean;
  playing: boolean;
  runtimeStatus: RuntimeStatus | null;
  resolvedState?: string;
};

export const ProfileCard = memo(function ProfileCard({ profile, listView, chosen, favorite, hidden, onOpen, onToggleFavorite, onToggleHidden, onInstall, onPlay, onRepair, onUpdate, onCheckUpdate, onUninstall, onDuplicate, onOpenFolder, onSetSelected, onFilterGroup, onCopyName, installing, playing, runtimeStatus, resolvedState }: ProfileCardProps) {
  const [menuPosition, setMenuPosition] = useState<{ left: number; top: number } | null>(null);
  const overflowRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const openMenu = useCallback((point?: { x: number; y: number }) => {
    const rect = overflowRef.current?.getBoundingClientRect();
    const menuWidth = 254;
    const left = point?.x ?? (rect ? rect.right - 5 : 16);
    const top = point?.y ?? (rect ? rect.top + 20 : 16);
    setMenuPosition({
      left: Math.max(8, Math.min(left, window.innerWidth - menuWidth - 8)),
      top: Math.max(8, Math.min(top, window.innerHeight - 360)),
    });
  }, []);

  useLayoutEffect(() => {
    if (!menuPosition) return;
    const first = menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)");
    first?.focus();
  }, [menuPosition]);
  useEffect(() => {
    if (!menuPosition) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node) && !overflowRef.current?.contains(event.target as Node)) setMenuPosition(null);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); setMenuPosition(null); overflowRef.current?.focus(); }
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      const items = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
      const current = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "ArrowDown" ? (current + 1) % items.length : (current - 1 + items.length) % items.length;
      event.preventDefault(); items[next]?.focus();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    return () => { document.removeEventListener("pointerdown", onPointerDown, true); document.removeEventListener("keydown", onKeyDown); };
  }, [menuPosition]);

  const runtimeReady = runtimeStatus?.compatible !== null && runtimeStatus?.compatible !== undefined;
  const runtimeAction = profile.clientReady && !runtimeReady;
  const requiredJavaMajor = ["26.1.2", "26.2", "26.3"].includes(profile.minecraftVersion) ? 25 : 21;
  const actionLabel = playing || resolvedState === "Running" ? "RUNNING" : installing ? profile.clientReady ? runtimeAction ? "INSTALLING JAVA" : "VERIFYING" : "INSTALLING" : resolvedState === "RepairRequired" ? "REPAIR" : resolvedState === "UpdateAvailable" ? "UPDATE" : profile.installed ? "PLAY" : profile.clientReady ? runtimeStatus === null ? "CHECKING RUNTIME" : runtimeReady ? "INSTALL MINECRAFT" : `INSTALL JAVA ${requiredJavaMajor}` : "INSTALL";
  const statusLabel = playing || resolvedState === "Running" ? "RUNNING" : installing ? "INSTALLING" : resolvedState === "RepairRequired" ? "REPAIR NEEDED" : resolvedState === "UpdateAvailable" ? "UPDATE READY" : profile.installed ? "INSTALLED" : profile.clientReady ? "GAME SETUP NEEDED" : "NOT INSTALLED";
  const statusTone = resolvedState === "RepairRequired" ? "warning" : playing || resolvedState === "Running" || (profile.installed && resolvedState !== "UpdateAvailable") ? "ready" : installing || resolvedState === "UpdateAvailable" || profile.clientReady ? "working" : "neutral";
  const loaderName = profile.loaderType === "neoforge" ? "NeoForge" : profile.loaderType[0].toUpperCase() + profile.loaderType.slice(1);
  const primaryDisabled = installing || playing || (profile.clientReady && runtimeStatus === null);
  const primaryAction = () => { if (primaryDisabled) { setMenuPosition(null); } else if (resolvedState === "RepairRequired") { onRepair(profile); setMenuPosition(null); } else if (resolvedState === "UpdateAvailable") { onUpdate(profile); setMenuPosition(null); } else { profile.installed ? onPlay(profile) : onInstall(profile); setMenuPosition(null); } };
  const showMenu = menuPosition ? createPortal(
    <div className="profile-context-menu" role="menu" aria-label={`${profile.name} actions`} ref={menuRef} style={{ left: menuPosition.left, top: menuPosition.top }} onPointerDown={event => event.stopPropagation()}>
      <MenuItem icon={<Settings2 size={16} />} label="Open profile" onClick={() => { setMenuPosition(null); onOpen(profile); }} />
      <MenuItem icon={<Check size={16} />} label={chosen ? "Selected profile" : "Make selected profile"} disabled={chosen} onClick={() => { onSetSelected(profile); setMenuPosition(null); }} />
      <MenuItem icon={<Heart size={16} fill={favorite ? "currentColor" : "none"} />} label={favorite ? "Remove from favorites" : "Add to favorites"} onClick={() => { onToggleFavorite(profile.id); setMenuPosition(null); }} />
      <MenuItem icon={<CopyPlus size={16} />} label="Duplicate profile" onClick={() => { onDuplicate(profile); setMenuPosition(null); }} />
      <MenuItem icon={<Copy size={16} />} label="Copy profile name" onClick={() => { onCopyName(profile); setMenuPosition(null); }} />
      {(profile.installed || profile.clientReady) && <MenuItem icon={<FolderOpen size={16} />} label="Open game folder" onClick={() => { onOpenFolder(profile); setMenuPosition(null); }} />}
      <MenuItem icon={<ListFilter size={16} />} label={profile.groupName?.trim() ? `Show group: ${profile.groupName.trim()}` : "Show ungrouped profiles"} onClick={() => { onFilterGroup(profile.groupName?.trim() ?? ""); setMenuPosition(null); }} />
      <div className="profile-menu-separator" />
      <MenuItem icon={profile.installed ? <Play size={16} /> : <Download size={16} />} label={actionLabel} disabled={primaryDisabled} onClick={primaryAction} />
      <MenuItem icon={<Settings2 size={16} />} label="Manage" onClick={() => { setMenuPosition(null); onOpen(profile); }} />
      <div className="profile-menu-separator" />
      {profile.installed && <><MenuItem icon={<Download size={16} />} label="Check for updates" disabled={playing || installing} onClick={() => { setMenuPosition(null); onCheckUpdate(profile); }} /><MenuItem icon={<Settings2 size={16} />} label="Verify / Repair" disabled={playing || installing} onClick={() => { setMenuPosition(null); onRepair(profile); }} /><MenuItem icon={<X size={16} />} label="Uninstall" disabled={playing || installing} onClick={() => { setMenuPosition(null); onUninstall(profile); }} /><div className="profile-menu-separator" /></>}
      <MenuItem icon={hidden ? <Check size={16} /> : <X size={16} />} label={hidden ? "Show in library" : "Hide from library"} onClick={() => { onToggleHidden(profile.id); setMenuPosition(null); }} />
    </div>, document.body) : null;

  const favoriteButton = <button className={listView ? `profile-row-action ${favorite ? "is-favorite" : ""}` : `cover-action favorite-action ${favorite ? "is-favorite" : ""}`} aria-label={favorite ? `Remove ${profile.name} from favorites` : `Add ${profile.name} to favorites`} aria-pressed={favorite} title={favorite ? "Remove from favorites" : "Add to favorites"} onClick={() => onToggleFavorite(profile.id)}><Heart size={18} fill={favorite ? "currentColor" : "none"} /></button>;
  const menuButton = <button className={listView ? "profile-row-action" : "cover-action overflow-action"} aria-label={`${profile.name} menu`} aria-haspopup="menu" aria-expanded={menuPosition !== null} title="More options" ref={overflowRef} onClick={() => menuPosition ? setMenuPosition(null) : openMenu()}><MoreHorizontal size={19} /></button>;
  const primaryButton = <button className={listView ? "profile-row-primary" : "cover-primary-action"} disabled={primaryDisabled} onClick={primaryAction}>{profile.installed ? <Play size={16} fill="currentColor" /> : <Download size={16} />}{actionLabel}</button>;
  const profileIcon = <ProfileCover profile={profile} />;
  const profileInfo = <button className={`profile-card-info ${listView ? "profile-row-info" : ""}`} onClick={() => onOpen(profile)}><strong>{profile.name}</strong><small>Minecraft {profile.minecraftVersion}</small><span className="profile-meta-badges"><i className={`profile-status-badge ${statusTone}`}>{statusLabel}</i><i>{loaderName}{profile.loaderVersion ? ` ${profile.loaderVersion}` : ""}</i><i>Java {requiredJavaMajor}</i><i>{profile.snineclientEnabled ? "SNineClient" : "Vanilla"}</i>{profile.groupName?.trim() && <i className="group-badge">{profile.groupName.trim()}</i>}</span></button>;

  if (listView) return <article className={`profile-card profile-row ${chosen ? "chosen" : ""} ${hidden ? "is-hidden" : ""}`} onContextMenu={event => { event.preventDefault(); openMenu({ x: event.clientX, y: event.clientY }); }}>
    <button className="profile-row-icon" aria-label={`Open ${profile.name}`} onClick={() => onOpen(profile)} onDoubleClick={() => profile.installed && !playing ? onPlay(profile) : onOpen(profile)}>{profileIcon}</button>
    {profileInfo}
    <div className="profile-row-actions">{primaryButton}{favoriteButton}{menuButton}</div>
    {showMenu}
  </article>;

  return <article className={`profile-card ${chosen ? "chosen" : ""} ${hidden ? "is-hidden" : ""}`} onContextMenu={event => { event.preventDefault(); openMenu({ x: event.clientX, y: event.clientY }); }}>
    <div className="cover-stage">
      <button className="cover-hitarea" aria-label={`Open ${profile.name}`} onClick={() => onOpen(profile)} onDoubleClick={() => profile.installed && !playing ? onPlay(profile) : onOpen(profile)}>
        {profileIcon}
      </button>
      <span className="cover-hover-shade" aria-hidden="true" />
      <div className="cover-actions">
        {favoriteButton}
        {menuButton}
      </div>
      {primaryButton}
    </div>
    {profileInfo}
    {showMenu}
  </article>;
});

function MenuItem({ icon, label, disabled = false, title, onClick }: { icon: ReactNode; label: string; disabled?: boolean; title?: string; onClick?: () => void }) {
  return <button type="button" role="menuitem" className={`profile-menu-item ${label === "Uninstall" ? "destructive" : ""}`} disabled={disabled} title={title} onClick={onClick}><span className="profile-menu-icon">{icon}</span><span>{label}</span></button>;
}

