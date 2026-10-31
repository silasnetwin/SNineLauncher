import { useEffect, useState } from "react";
import { Search, X } from "lucide-react";
import { ipc, type BlockIconEntry, type LoaderType, type MinecraftVersionInfo } from "../lib/ipc";
import snineLauncherIcon from "../assets/brand/snine-launcher-icon.png";

export { insertCreatedProfile } from "../lib/profile-utils";

export type ProfileCreationFormProps = {
  name: string; onNameChange: (value: string) => void;
  minecraftVersion: string; onMinecraftVersionChange: (value: string) => void;
  versions: MinecraftVersionInfo[]; showSnapshots: boolean; onShowSnapshotsChange: (value: boolean) => void;
  loaderType: LoaderType; onLoaderTypeChange: (value: LoaderType) => void;
  availableLoaders: LoaderType[];
  loaderVersion: string; loaderVersions: string[]; onLoaderVersionChange: (value: string) => void;
  includeSnineClient: boolean; onIncludeSnineClientChange: (value: boolean) => void;
  memory: string; onMemoryChange: (value: string) => void;
  iconId: string | null; iconPreview: string | null; onChooseIcon: () => void;
  onCreate: () => void; onCancel: () => void;
};

export function ProfileCreationForm(props: ProfileCreationFormProps) {
  const incompatible = props.includeSnineClient && (props.loaderType !== "fabric" || !["1.21.11", "26.1.2", "26.3"].includes(props.minecraftVersion));
  const visibleVersions = props.versions.filter(item => props.showSnapshots || item.versionType === "release");
  const invalidMemory = props.memory.trim() !== "" && (!Number.isInteger(Number(props.memory)) || Number(props.memory) < 2048 || Number(props.memory) > 32768 || Number(props.memory) % 512 !== 0);
  const ready = props.name.trim().length > 0 && props.name.trim().length <= 48 && visibleVersions.some(item => item.id === props.minecraftVersion) && (!(props.loaderType === "fabric" || props.loaderType === "quilt") || !!props.loaderVersion) && !incompatible && !invalidMemory;
  return <div className="modal-scrim" onClick={props.onCancel}><form className="settings-popover profile-create-dialog" role="dialog" aria-modal="true" aria-labelledby="create-profile-heading" onSubmit={event => { event.preventDefault(); if (ready) props.onCreate(); }} onClick={event => event.stopPropagation()}>
    <div><div><small>PROFILE MANAGEMENT</small><h2 id="create-profile-heading">Create profile</h2></div><button type="button" className="subtle-icon" aria-label="Close create profile" onClick={props.onCancel}><X size={16} /></button></div>
    <label className="profile-name-field"><span>Profile name</span><input autoFocus maxLength={48} value={props.name} onChange={event => props.onNameChange(event.target.value)} placeholder="My Minecraft profile" /></label>
    <div className="profile-icon-field"><span>Profile icon</span><div>{props.iconPreview || props.includeSnineClient ? <img src={props.iconPreview ?? snineLauncherIcon} alt={props.iconId ?? (props.includeSnineClient ? "SNineClient logo" : "Selected Minecraft icon")} /> : <div className="create-icon-cover" aria-hidden="true" />}<strong>{props.iconId ?? (props.includeSnineClient ? "SNineClient logo" : "Default Minecraft icon")}</strong><button type="button" className="secondary-action" onClick={props.onChooseIcon}>Choose icon…</button></div></div>
    <div className="profile-create-config">
      <label><span>Minecraft version</span><input aria-label="Minecraft version" list="minecraft-version-options" value={props.minecraftVersion} onChange={event => props.onMinecraftVersionChange(event.target.value)} placeholder="Search releases…" /><datalist id="minecraft-version-options">{visibleVersions.map(item => <option key={item.id} value={item.id} label={`${item.id} · ${new Date(item.releaseTime).toLocaleDateString()} · ${item.versionType}`} />)}</datalist></label>
      <label className="snapshot-toggle"><input aria-label="Show snapshots" type="checkbox" checked={props.showSnapshots} onChange={event => props.onShowSnapshotsChange(event.target.checked)} /><span>Show snapshots</span></label>
      <label><span>Mod loader</span><select aria-label="Mod loader" value={props.loaderType} onChange={event => props.onLoaderTypeChange(event.target.value as LoaderType)}>{props.availableLoaders.map(loader => <option key={loader} value={loader}>{loader === "neoforge" ? "NeoForge" : loader[0].toUpperCase() + loader.slice(1)}</option>)}</select></label>
      {(props.loaderType === "fabric" || props.loaderType === "quilt") && <label><span>{props.loaderType === "quilt" ? "Quilt Loader version" : "Fabric Loader version"}</span><select aria-label={`${props.loaderType === "quilt" ? "Quilt" : "Fabric"} Loader version`} value={props.loaderVersion} onChange={event => props.onLoaderVersionChange(event.target.value)}>{props.loaderVersions.map(version => <option key={version} value={version}>{version}</option>)}</select></label>}
      <label className="snapshot-toggle"><input aria-label="Include SNineClient" type="checkbox" checked={props.includeSnineClient} onChange={event => props.onIncludeSnineClientChange(event.target.checked)} /><span>Include SNineClient</span></label>
      {incompatible && <p className="profile-create-note" role="alert">SNineClient currently has builds for Fabric on Minecraft 1.21.11, 26.1.2, and 26.3. <button type="button" className="inline-action" onClick={() => { props.onLoaderTypeChange("fabric"); props.onMinecraftVersionChange("1.21.11"); }}>Switch to supported configuration</button></p>}
    </div>
    <label className="profile-name-field"><span>Memory limit <small>Optional · 2048–32768 MB, 512 MB steps</small></span><input aria-label="Memory limit" type="number" min={2048} max={32768} step={512} value={props.memory} onChange={event => props.onMemoryChange(event.target.value)} placeholder="Use launcher default" /></label>
    <p className="profile-create-note">The profile appears in your Library as soon as it is created; installation then runs in the background.</p>
    <div className="uninstall-actions"><button type="button" className="secondary-action" onClick={props.onCancel}>Cancel</button><button className="compact-play" type="submit" disabled={!ready}>Create profile</button></div>
  </form></div>;
}

export function BlockIconPicker({ open, minecraftVersion, selectedIconId, selectedPreview, onSelect, onClose }: { open: boolean; minecraftVersion: string; selectedIconId: string | null; selectedPreview: string | null; onSelect: (item: BlockIconEntry) => void; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<BlockIconEntry[]>([]);
  const [offset, setOffset] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!open || !("__TAURI_INTERNALS__" in window)) return;
    let active = true;
    setItems([]); setOffset(0); setError(""); setBusy(true);
    const timer = window.setTimeout(() => { void ipc.getBlockIconCatalog("latest", query, 0, 64).then(result => { if (active) { setItems(result); setOffset(result.length); } }).catch(reason => { if (active) setError(String(reason)); }).finally(() => { if (active) setBusy(false); }); }, query ? 150 : 0);
    return () => { active = false; window.clearTimeout(timer); };
  }, [open, minecraftVersion, query]);
  const loadMore = async () => {
    if (busy || !open) return;
    setBusy(true);
    try { const next = await ipc.getBlockIconCatalog("latest", query, offset, 64); setItems(current => [...current, ...next]); setOffset(current => current + next.length); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  if (!open) return null;
  const sourceVersion = items[0]?.sourceVersion;
  return <div className="modal-scrim icon-picker-scrim" onClick={onClose}><section className="settings-popover block-picker-dialog" role="dialog" aria-modal="true" aria-labelledby="block-picker-heading" onClick={event => event.stopPropagation()}><div><div><small>{sourceVersion ? `MINECRAFT ${sourceVersion}` : "LATEST MINECRAFT RELEASE"} · BLOCKS AND ITEMS</small><h2 id="block-picker-heading">Choose a Minecraft icon</h2></div><button type="button" className="subtle-icon" aria-label="Close block picker" onClick={onClose}><X size={16} /></button></div><label className="search-field block-picker-search"><Search size={16} /><input autoFocus aria-label="Search Minecraft blocks and items" value={query} onChange={event => setQuery(event.target.value)} placeholder="Search blocks and items…" /></label>{selectedPreview && <div className="current-block-selection"><img src={selectedPreview} alt="Selected Minecraft icon" /><span>{selectedIconId}</span></div>}{busy && items.length === 0 && <p>Loading the latest Minecraft release icons…</p>}{error && <p className="profile-create-note" role="status">{error} Your current profile icon stays selected.</p>}<div className="block-icon-grid">{items.map(item => <button type="button" key={item.id} className="block-icon-option" title={`${item.id} · ${item.kind}`} onClick={() => onSelect(item)}>{item.previewDataUrl ? <img loading="lazy" src={item.previewDataUrl} alt="" /> : <span className="block-icon-fallback">?</span>}<span>{item.label}</span><small>{item.kind === "block_and_item" ? "Block & item" : item.kind}</small></button>)}</div>{items.length > 0 && <button type="button" className="secondary-action block-load-more" disabled={busy} onClick={() => void loadMore()}>{busy ? "Loading…" : "Load more icons"}</button>}{!busy && !error && items.length === 0 && <p>No blocks or items matched this search.</p>}</section></div>;
}
