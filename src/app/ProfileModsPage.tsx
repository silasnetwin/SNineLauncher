import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Clipboard, Download, PackageOpen, Plus, Search, Settings2, Sparkles, Trash2, Upload, X } from "lucide-react";
import { ipc, type LoaderType, type ManagedMod, type ModrinthProject, type ModUpdateStatus } from "../lib/ipc";
import { ensureDefaultS9Packs, getS9PackPreference, setS9PackPreference, type S9ModPack } from "../lib/s9Packs";
import bundledPackCatalog from "../../s9-client-packs/catalog.json";

const searchCache = new Map<string, { savedAt: number; results: ModrinthProject[] }>();
const projectIconCache = new Map<string, string>();
const CACHE_TTL = 5 * 60_000;
const browseCategories = ["all", "optimization", "utility", "library", "technology", "worldgen"] as const;
const PACK_CATALOG_URL = "https://raw.githubusercontent.com/silasnetwin/SNineLauncher/main/s9-client-packs/catalog.json";
type PackCatalogSource = "GitHub" | "Built in";

function parsePackCatalog(value: unknown): S9ModPack[] | null {
  if (!value || typeof value !== "object") return null;
  const catalog = value as { format?: unknown; version?: unknown; packs?: unknown };
  if (catalog.format !== "snine-pack-catalog" || catalog.version !== 1 || !Array.isArray(catalog.packs) || catalog.packs.length > 24) return null;
  const packs: S9ModPack[] = [];
  for (const candidate of catalog.packs) {
    if (!candidate || typeof candidate !== "object") return null;
    const pack = candidate as Record<string, unknown>;
    if (typeof pack.id !== "string" || !/^[a-z0-9][a-z0-9-]{0,47}$/.test(pack.id) || typeof pack.name !== "string" || pack.name.length > 64 || typeof pack.description !== "string" || pack.description.length > 240 || !["fabric", "quilt", "forge", "neoforge", "vanilla"].includes(String(pack.loader)) || !Array.isArray(pack.minecraftVersions) || !pack.minecraftVersions.every(version => typeof version === "string" && /^[a-zA-Z0-9.+-]{1,32}$/.test(version)) || !Array.isArray(pack.mods) || pack.mods.length > 64) return null;
    const mods: S9ModPack["mods"] = [];
    for (const entry of pack.mods) {
      if (!entry || typeof entry !== "object") return null;
      const mod = entry as Record<string, unknown>;
      if (typeof mod.projectId !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(mod.projectId) || typeof mod.slug !== "string" || !/^[a-zA-Z0-9_-]{1,96}$/.test(mod.slug)) return null;
      mods.push({ projectId: mod.projectId, slug: mod.slug });
    }
    packs.push({ id: pack.id, name: pack.name, description: pack.description, loader: pack.loader as LoaderType, minecraftVersions: pack.minecraftVersions as string[], mods });
  }
  return packs;
}

export function ProfileModsPage({ profileId, minecraftVersion, loaderType = "fabric", installed, defaultSnineProfile = false }: { profileId: string; minecraftVersion: string; loaderType?: LoaderType; installed: boolean; defaultSnineProfile?: boolean }) {
  const [view, setView] = useState<"Installed" | "Browse" | "Packs">("Installed");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ModrinthProject[]>([]);
  const [mods, setMods] = useState<ManagedMod[]>([]);
  const [modsLoaded, setModsLoaded] = useState(false);
  const [updates, setUpdates] = useState<ModUpdateStatus[]>([]);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState("");
  const [searchError, setSearchError] = useState("");
  const [browseActionError, setBrowseActionError] = useState("");
  const [notice, setNotice] = useState("");
  const [working, setWorking] = useState<string | null>(null);
  const [removeTarget, setRemoveTarget] = useState<ManagedMod | null>(null);
  const [confirmUpdates, setConfirmUpdates] = useState(false);
  const [statusFilter, setStatusFilter] = useState<"all" | "enabled" | "disabled">("all");
  const [sourceFilter, setSourceFilter] = useState<"all" | "managed" | "local">("all");
  const [sortOrder, setSortOrder] = useState<"az" | "za" | "file">("az");
  const [selectedFiles, setSelectedFiles] = useState<string[]>([]);
  const [manifestOpen, setManifestOpen] = useState(false);
  const [manifestText, setManifestText] = useState("");
  const [confirmBulkRemove, setConfirmBulkRemove] = useState(false);
  const [projectIcons, setProjectIcons] = useState<Record<string, string>>({});
  const [packCatalog, setPackCatalog] = useState<S9ModPack[]>(() => parsePackCatalog(bundledPackCatalog) ?? []);
  const [packCatalogSource, setPackCatalogSource] = useState<PackCatalogSource>("Built in");
  const [packSyncing, setPackSyncing] = useState(false);
  const [packPreferences, setPackPreferences] = useState<Record<string, boolean>>(() => Object.fromEntries((parsePackCatalog(bundledPackCatalog) ?? []).map(pack => [pack.id, getS9PackPreference(profileId, pack.id) ?? defaultSnineProfile])));
  const [browseCategory, setBrowseCategory] = useState<(typeof browseCategories)[number]>("all");
  const [browseSort, setBrowseSort] = useState<"downloads" | "name">("downloads");
  const [searchRetry, setSearchRetry] = useState(0);
  const requestId = useRef(0);
  const syncedDefaultProfiles = useRef(new Set<string>());
  const flushSearchRef = useRef(false);
  const canManageModrinthMods = loaderType === "fabric";
  const compatiblePacks = useMemo(() => packCatalog.filter(pack => pack.loader === loaderType && pack.minecraftVersions.includes(minecraftVersion)), [loaderType, minecraftVersion, packCatalog]);
  const normalizedQuery = query.trim();
  const installedIds = useMemo(() => new Set(mods.map(mod => mod.projectId).filter(Boolean)), [mods]);
  const updateIds = useMemo(() => new Set(updates.filter(update => update.updateAvailable).map(update => update.projectId)), [updates]);
  const visibleMods = useMemo(() => mods.filter(mod => {
    const search = query.trim().toLocaleLowerCase();
    const matchesSearch = !search || [displayName(mod), mod.filename, mod.slug, mod.projectId].some(value => value.toLocaleLowerCase().includes(search));
    const matchesStatus = statusFilter === "all" || mod.enabled === (statusFilter === "enabled");
    const matchesSource = sourceFilter === "all" || mod.managed === (sourceFilter === "managed");
    return matchesSearch && matchesStatus && matchesSource;
  }).sort((a, b) => sortOrder === "file" ? a.filename.localeCompare(b.filename) : sortOrder === "za" ? displayName(b).localeCompare(displayName(a)) : displayName(a).localeCompare(displayName(b))), [mods, query, statusFilter, sourceFilter, sortOrder]);
  const visibleResults = useMemo(() => results
    .filter(mod => browseCategory === "all" || mod.categories.includes(browseCategory))
    .sort((a, b) => browseSort === "name" ? a.title.localeCompare(b.title) : b.downloads - a.downloads), [results, browseCategory, browseSort]);

  useEffect(() => {
    if (!installed) return;
    let current = true;
    void ipc.getManagedMods(profileId).then(value => { if (current) { setMods(value); setModsLoaded(true); } }).catch(reason => { if (current) { setError(toMessage(reason)); setModsLoaded(true); } });
    return () => { current = false; };
  }, [profileId, installed]);

  useEffect(() => {
    if (!installed) return;
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 7000);
    void fetch(PACK_CATALOG_URL, { headers: { Accept: "application/json" }, signal: controller.signal, cache: "no-cache" })
      .then(async response => {
        if (!response.ok) throw new Error(`Catalog unavailable (HTTP ${response.status}).`);
        const text = await response.text();
        if (text.length > 200_000) throw new Error("Pack catalog exceeded its size limit.");
        const parsed = parsePackCatalog(JSON.parse(text));
        if (!parsed) throw new Error("Pack catalog format is not supported.");
        setPackCatalog(parsed);
        setPackCatalogSource("GitHub");
      })
      .catch(() => { setPackCatalog(parsePackCatalog(bundledPackCatalog) ?? []); setPackCatalogSource("Built in"); })
      .finally(() => window.clearTimeout(timeout));
    return () => { window.clearTimeout(timeout); controller.abort(); };
  }, [installed]);

  useEffect(() => {
    if (!installed || !defaultSnineProfile || !modsLoaded || !canManageModrinthMods || syncedDefaultProfiles.current.has(profileId)) return;
    syncedDefaultProfiles.current.add(profileId);
    let current = true;
    setPackSyncing(true);
    void ensureDefaultS9Packs(profileId, minecraftVersion, loaderType).then(async result => {
      if (!current) return;
      setMods(await ipc.getManagedMods(profileId));
      if (result.failed.length) setError(`Some default S9 Pack mods could not be installed: ${result.failed.join(", ")}.`);
      else if (result.installed) setNotice(`Default S9 Packs installed · ${result.installed} mods.`);
    }).catch(reason => { if (current) setError(toMessage(reason)); })
      .finally(() => { if (current) setPackSyncing(false); });
    return () => { current = false; };
  }, [installed, defaultSnineProfile, modsLoaded, canManageModrinthMods, profileId, minecraftVersion, loaderType]);

  useEffect(() => {
    const iconMods = mods.filter(mod => !mod.iconDataUrl);
    const lookupIds = [...new Set(iconMods.flatMap(modIconLookupIds))];
    if (!lookupIds.length) { setProjectIcons({}); return; }
    let current = true;
    const missing = lookupIds.filter(id => !projectIconCache.has(id));
    if (missing.length) {
      void ipc.getModrinthProjectIcons(missing).then(entries => {
        if (!current) return;
        const returned = new Map(entries.flatMap(entry => [[entry.projectId, entry.iconUrl] as const, [entry.slug, entry.iconUrl] as const]));
        for (const id of missing) projectIconCache.set(id, returned.get(id) ?? "");
        for (const entry of entries) projectIconCache.set(entry.slug, entry.iconUrl);
        setProjectIcons(Object.fromEntries(lookupIds.filter(id => projectIconCache.has(id)).map(id => [id, projectIconCache.get(id) ?? ""])));
      }).catch(() => { /* Keep the local fallback visible when Modrinth is offline. */ });
    }
    setProjectIcons(Object.fromEntries(lookupIds.filter(id => projectIconCache.has(id)).map(id => [id, projectIconCache.get(id) ?? ""])));
    return () => { current = false; };
  }, [mods]);

  useEffect(() => {
    const id = ++requestId.current;
    if (view !== "Browse") { setBusy(false); setSearchError(""); setResults([]); return; }
    if (normalizedQuery.length === 1) { setBusy(false); setSearchError(""); setResults([]); return; }
    setSearchError("");
    const key = `${minecraftVersion}\n${normalizedQuery ? normalizedQuery.toLocaleLowerCase() : "__featured__"}`;
    const cached = searchCache.get(key);
    if (cached) setResults(cached.results);
    else setResults([]);
    if (cached && Date.now() - cached.savedAt < CACHE_TTL) { flushSearchRef.current = false; setBusy(false); return; }
    setBusy(!cached);
    const delay = flushSearchRef.current ? 0 : normalizedQuery ? 320 : 0;
    flushSearchRef.current = false;
    const timer = window.setTimeout(() => {
      setSearchError("");
      const search = normalizedQuery ? ipc.searchModrinthFabricMods(normalizedQuery, minecraftVersion) : ipc.getModrinthFabricRecommendations(minecraftVersion);
      void search.then(value => {
        if (requestId.current !== id) return;
        searchCache.set(key, { savedAt: Date.now(), results: value });
        setResults(value);
      }).catch(reason => {
        if (requestId.current === id) setSearchError(toMessage(reason));
      }).finally(() => { if (requestId.current === id) setBusy(false); });
    }, delay);
    return () => { window.clearTimeout(timer); };
  }, [view, normalizedQuery, minecraftVersion, searchRetry]);

  const refresh = async () => {
    setChecking(true); setNotice(""); setError("");
    try {
      const value = await ipc.checkModUpdates(profileId);
      setUpdates(value);
      const count = value.filter(item => item.updateAvailable).length;
      setNotice(count ? `${count} update${count === 1 ? "" : "s"} available.` : "Your installed mods are up to date.");
    } catch (reason) { setError(toMessage(reason)); }
    finally { setChecking(false); }
  };
  const updateAll = async () => {
    if (!canManageModrinthMods) { setError("Modrinth mod installs and updates are currently supported for Fabric profiles only."); return; }
    const pending = updates.filter(item => item.updateAvailable);
    if (!pending.length) return;
    setConfirmUpdates(false); setBusy(true); setError("");
    const failed: string[] = [];
    for (const item of pending) {
      try { setMods(await ipc.installModrinthMod(profileId, item.projectId, item.slug)); }
      catch { failed.push(item.slug); }
    }
    setUpdates(current => current.filter(item => failed.includes(item.slug)));
    setBusy(false);
    setNotice(failed.length ? `Some updates could not be installed: ${failed.join(", ")}.` : `${pending.length} mod${pending.length === 1 ? "" : "s"} updated.`);
  };
  const install = async (projectId: string, title: string, slug = projectId) => {
    if (!canManageModrinthMods) { setBrowseActionError("Modrinth mod installs are currently supported for Fabric profiles only."); return; }
    setWorking(projectId); setBrowseActionError(""); setError(""); setNotice("");
    try { setMods(await ipc.installModrinthMod(profileId, projectId, slug)); setNotice(`${title} installed.`); }
    catch (reason) { const message = toMessage(reason); if (view === "Browse") setBrowseActionError(message); else setError(message); }
    finally { setWorking(null); }
  };
  const packIsEnabled = (pack: S9ModPack) => packPreferences[pack.id] ?? getS9PackPreference(profileId, pack.id) ?? defaultSnineProfile;
  const togglePack = async (pack: S9ModPack) => {
    if (!canManageModrinthMods) { setError("S9 Packs currently require a Fabric profile."); return; }
    const enable = !packIsEnabled(pack);
    setPackSyncing(true); setError(""); setNotice("");
    const failed: string[] = [];
    let nextMods = mods;
    for (const entry of pack.mods) {
      const current = nextMods.find(mod => mod.managed && mod.projectId === entry.projectId);
      try {
        if (enable && !current) nextMods = await ipc.installModrinthMod(profileId, entry.projectId, entry.slug);
        else if (current && current.enabled !== enable) {
          await ipc.setModEnabled(profileId, current.filename, enable);
          nextMods = nextMods.map(mod => mod.filename === current.filename ? { ...mod, enabled: enable } : mod);
        }
        setMods(nextMods);
      } catch { failed.push(entry.slug); }
    }
    setPackSyncing(false);
    if (failed.length) {
      setError(`Could not ${enable ? "enable" : "disable"} all ${pack.name} mods: ${failed.join(", ")}.`);
      return;
    }
    setS9PackPreference(profileId, pack.id, enable);
    setPackPreferences(current => ({ ...current, [pack.id]: enable }));
    setNotice(enable ? `${pack.name} enabled and installed.` : `${pack.name} disabled. The mod files are kept.`);
  };
  const toggle = async (mod: ManagedMod) => {
    setWorking(mod.filename); setError("");
    try {
      await ipc.setModEnabled(profileId, mod.filename, !mod.enabled);
      setMods(current => current.map(item => item.filename === mod.filename ? { ...item, enabled: !mod.enabled } : item));
      setNotice(`${displayName(mod)} ${mod.enabled ? "disabled" : "enabled"}.`);
    } catch (reason) { setError(toMessage(reason)); }
    finally { setWorking(null); }
  };
  const remove = async () => {
    if (!removeTarget) return;
    setWorking(removeTarget.filename); setError("");
    try {
      await ipc.removeManagedMod(profileId, removeTarget.filename);
      setMods(current => current.filter(mod => mod.filename !== removeTarget.filename));
      setNotice(`${displayName(removeTarget)} removed.`); setRemoveTarget(null);
    } catch (reason) { setError(toMessage(reason)); }
    finally { setWorking(null); }
  };
  const bulkAction = async (action: "enable" | "disable" | "remove") => {
    const selected = mods.filter(mod => selectedFiles.includes(mod.filename) && (action === "remove" || mod.enabled !== (action === "enable")));
    setConfirmBulkRemove(false); setBusy(true); setError("");
    const failed: string[] = [];
    for (const mod of selected) {
      try {
        if (action === "remove") await ipc.removeManagedMod(profileId, mod.filename);
        else await ipc.setModEnabled(profileId, mod.filename, action === "enable");
      } catch { failed.push(mod.filename); }
    }
    if (action === "remove") setMods(current => current.filter(mod => !selected.includes(mod) || failed.includes(mod.filename)));
    else setMods(current => current.map(mod => selected.includes(mod) && !failed.includes(mod.filename) ? { ...mod, enabled: action === "enable" } : mod));
    setSelectedFiles(failed);
    setBusy(false);
    setNotice(`${selected.length - failed.length} mod${selected.length - failed.length === 1 ? "" : "s"} ${action === "remove" ? "removed" : action === "enable" ? "enabled" : "disabled"}${failed.length ? ` · ${failed.length} failed` : ""}.`);
  };
  const copyText = async (value: string, label: string) => {
    try { await navigator.clipboard.writeText(value); setNotice(`${label} copied to clipboard.`); }
    catch { setError("Clipboard access is unavailable."); }
  };
  const createManifest = () => JSON.stringify({ format: "snine-modpack", version: 1, minecraft: minecraftVersion, loader: loaderType, mods: mods.filter(mod => mod.managed && mod.projectId).map(({ projectId, slug }) => ({ projectId, slug })) }, null, 2);
  const importManifest = async () => {
    let parsed: { format?: string; version?: number; minecraft?: string; loader?: string; mods?: Array<{ projectId?: string; slug?: string }> };
    try { parsed = JSON.parse(manifestText); }
    catch { setError("That is not valid JSON."); return; }
    if (parsed.format !== "snine-modpack" || parsed.version !== 1 || !Array.isArray(parsed.mods)) { setError("This is not a supported SNine modpack manifest."); return; }
    if (parsed.minecraft !== minecraftVersion || parsed.loader !== loaderType) { setError(`This pack targets Minecraft ${parsed.minecraft ?? "?"} with ${parsed.loader ?? "?"}; this profile uses ${minecraftVersion} with ${loaderType}.`); return; }
    const queue = parsed.mods.filter((mod): mod is { projectId: string; slug: string } => typeof mod.projectId === "string" && typeof mod.slug === "string").filter(mod => !installedIds.has(mod.projectId));
    setManifestOpen(false); setBusy(true); setError("");
    const failed: string[] = [];
    for (const mod of queue) { try { setMods(await ipc.installModrinthMod(profileId, mod.projectId, mod.slug)); } catch { failed.push(mod.slug); } }
    setBusy(false); setNotice(`${queue.length - failed.length} mod${queue.length - failed.length === 1 ? "" : "s"} imported${failed.length ? ` · failed: ${failed.join(", ")}` : ""}.`);
  };

  return <div className="detail-overview profile-mods-page">
    <div className="detail-section-heading"><div><span>PROFILE CONTENT</span><h2>Mods</h2></div></div>
    {!installed ? <p className="profile-mod-note">Install this profile to manage compatible mods.</p> : <>
      <div className="mods-section-topline">
        <div><span className="mods-section-eyebrow">PROFILE LIBRARY</span><strong>{view === "Installed" ? "Your mods" : view === "Browse" ? "Discover mods" : "S9 Client Packs"}</strong><small>{view === "Installed" ? `${mods.length} installed · Minecraft ${minecraftVersion}` : `Compatible with ${loaderType} · Minecraft ${minecraftVersion}`}</small></div>
        <div className="mods-subtabs" role="tablist" aria-label="Mod views">
          <button role="tab" aria-selected={view === "Installed"} className={view === "Installed" ? "active" : ""} onClick={() => setView("Installed")}>Installed <span>{mods.length}</span></button>
          <button role="tab" aria-label="Browse Modrinth" aria-selected={view === "Browse"} className={view === "Browse" ? "active" : ""} disabled={!canManageModrinthMods} title={!canManageModrinthMods ? "Mod management currently requires a Fabric profile" : undefined} onClick={() => { if (canManageModrinthMods) { setBrowseActionError(""); setView("Browse"); } }}><Plus size={14} /> Add mods</button>
          <button role="tab" aria-selected={view === "Packs"} className={view === "Packs" ? "active" : ""} onClick={() => setView("Packs")}><PackageOpen size={14} /> S9 packs</button>
        </div>
      </div>
      {view === "Installed" ? <>
        <div className="mods-toolbar"><label className="search-field profile-mod-search"><Search size={17} /><input value={query} onChange={event => setQuery(event.target.value)} placeholder="Search installed mods…" aria-label="Search installed mods" />{query && <button type="button" className="mod-search-clear" aria-label="Clear search" onClick={() => setQuery("")}><X size={15} /></button>}</label><div className="mods-library-actions"><button className="secondary-action" disabled={!mods.some(mod => mod.managed)} onClick={() => void copyText(createManifest(), "Modpack manifest")}><Clipboard size={14} /> Export pack</button><button className="secondary-action" onClick={() => { setManifestText(""); setManifestOpen(true); }}><Upload size={14} /> Import pack</button>{updateIds.size > 0 && canManageModrinthMods && <button className="mod-update-action" disabled={busy || checking} onClick={() => setConfirmUpdates(true)}><Download size={14} /> Update all ({updateIds.size})</button>}<button className="secondary-action" disabled={!canManageModrinthMods || checking || busy || mods.length === 0} title={!canManageModrinthMods ? "Modrinth mod updates currently require a Fabric profile" : undefined} onClick={() => void refresh()}><Download size={14} />{checking ? "Checking…" : "Check updates"}</button></div></div>
        {mods.length > 0 && <div className="mods-library-controls" aria-label="Installed mod filters"><div className="mods-filter-bar" aria-label="Filter by status"><span><Settings2 size={14} /> Status</span>{(["all", "enabled", "disabled"] as const).map(filter => <button key={filter} className={statusFilter === filter ? "active" : ""} aria-pressed={statusFilter === filter} onClick={() => setStatusFilter(filter)}>{filter === "all" ? "All" : filter === "enabled" ? "Enabled" : "Disabled"}<small>{filter === "all" ? mods.length : mods.filter(mod => mod.enabled === (filter === "enabled")).length}</small></button>)}</div><div className="mods-filter-bar" aria-label="Filter by source"><span>Source</span>{(["all", "managed", "local"] as const).map(filter => <button key={filter} className={sourceFilter === filter ? "active" : ""} aria-pressed={sourceFilter === filter} onClick={() => setSourceFilter(filter)}>{filter === "all" ? "All" : filter === "managed" ? "Modrinth" : "Local"}<small>{filter === "all" ? mods.length : mods.filter(mod => mod.managed === (filter === "managed")).length}</small></button>)}</div><label className="mods-sort-select">Sort by<select value={sortOrder} onChange={event => setSortOrder(event.target.value as typeof sortOrder)}><option value="az">Name A–Z</option><option value="za">Name Z–A</option><option value="file">File name</option></select></label></div>}
        {mods.length > 0 && <div className={`mods-selection-bar ${selectedFiles.length ? "has-selection" : ""}`} aria-label="Bulk mod actions"><button className="mods-select-visible" onClick={() => setSelectedFiles(selectedFiles.length === visibleMods.length ? [] : visibleMods.map(mod => mod.filename))}><Check size={14} />{selectedFiles.length === visibleMods.length && visibleMods.length ? "Deselect visible" : "Select visible"}</button>{selectedFiles.length > 0 && <><span className="mods-selected-count">{selectedFiles.length} selected</span><div className="mods-bulk-actions"><button disabled={busy} onClick={() => void bulkAction("enable")}>Enable</button><button disabled={busy} onClick={() => void bulkAction("disable")}>Disable</button><button className="mods-bulk-remove" disabled={busy} onClick={() => setConfirmBulkRemove(true)}><Trash2 size={13} /> Remove</button><button onClick={() => void copyText(mods.filter(mod => selectedFiles.includes(mod.filename)).map(displayName).join("\n"), "Mod names")}>Copy names</button><button onClick={() => void copyText(mods.filter(mod => selectedFiles.includes(mod.filename)).map(mod => mod.filename).join("\n"), "Mod file names")}>Copy files</button></div><button className="mods-clear-selection" onClick={() => setSelectedFiles([])}><X size={13} /> Clear</button></>}</div>}        {notice && <p className="profile-mod-note" role="status">{notice}</p>}{error && <p className="profile-mod-error" role="alert">{error}</p>}
        {mods.length === 0 ? <div className="mods-empty mods-empty-card"><div className="mods-empty-icon"><PackageOpen size={20} /></div><strong>Start with your first mod</strong><p>Find compatible Fabric mods for Minecraft {minecraftVersion} and install them directly into this profile.</p><button className="compact-play" onClick={() => setView("Browse")}><Plus size={14} /> Browse mods</button></div> : visibleMods.length === 0 ? <div className="mods-empty mods-empty-card"><Search size={18} /><strong>No matching mods</strong><p>Change the search or choose another status filter.</p><button className="secondary-action" onClick={() => { setQuery(""); setStatusFilter("all"); setSourceFilter("all"); }}>Clear filters</button></div> : <div className="managed-mod-list">{visibleMods.map(mod => <article className="managed-mod-row" key={mod.filename}>
          <input type="checkbox" aria-label={`Select ${displayName(mod)}`} checked={selectedFiles.includes(mod.filename)} onChange={event => setSelectedFiles(current => event.target.checked ? [...current, mod.filename] : current.filter(file => file !== mod.filename))} />
          <ProjectIcon iconUrl={mod.iconDataUrl ?? undefined} fallbackIconUrl={projectIcons[mod.projectId || mod.modId] || modIconLookupIds(mod).map(id => projectIcons[id]).find(Boolean)} label={displayName(mod)} className="mod-row-icon" /><span className="managed-mod-copy"><b>{displayName(mod)}</b><small>{mod.managed ? `Modrinth · ${mod.slug}` : `Local mod · ${mod.modId}`}</small></span>
          <span className={`mod-state ${mod.enabled ? "enabled" : "disabled"}`}><i />{mod.enabled ? "Enabled" : "Disabled"}</span>
          <div className="mod-row-actions">{updateIds.has(mod.projectId) && <button className="mod-update-action" disabled={working !== null} onClick={() => void install(mod.projectId, displayName(mod), mod.slug)}><Download size={13} /> Update</button>}
          <button className={`secondary-action mod-enable-action ${mod.enabled ? "is-enabled" : ""}`} disabled={working !== null} onClick={() => void toggle(mod)}>{working === mod.filename ? "Saving…" : mod.enabled ? "Disable" : "Enable"}</button>
          <button className="mod-remove-action" aria-label={`Remove ${displayName(mod)}`} disabled={working !== null} onClick={() => setRemoveTarget(mod)}><Trash2 size={14} /><span>Remove</span></button></div>
        </article>)}</div>}
      </> : view === "Browse" ? <>
        <section className="mods-search-panel" aria-label="Find compatible mods">
          <div className="mods-browse-intro"><div className="mods-browse-icon"><Search size={17} /></div><div><strong>Find your next mod</strong><p>Search Modrinth. Results are checked for this profile’s Minecraft version and Fabric loader.</p></div><span className="mods-target-chip">FABRIC · {minecraftVersion}</span></div>
          <label className="search-field profile-mod-search browse-mod-search"><Search size={17} /><input value={query} onChange={event => { setQuery(event.target.value); setBrowseCategory("all"); }} onKeyDown={event => { if (event.key === "Enter") { flushSearchRef.current = true; setSearchRetry(value => value + 1); } else if (event.key === "Escape" && query) { setQuery(""); setBrowseCategory("all"); } }} placeholder="Search by mod name, feature, or creator" aria-label="Search Modrinth mods" />{query && <button type="button" className="mod-search-clear" aria-label="Clear search" onClick={() => { setQuery(""); setBrowseCategory("all"); }}><X size={15} /></button>}</label>
          <div className="mods-search-hints"><span>Type to search · Enter to search now · Esc to clear</span><span>Compatible mods only</span></div>
        </section>
        {results.length > 0 && <div className="mods-browse-controls"><div className="mods-category-filters" aria-label="Filter mod categories">{browseCategories.filter(category => category === "all" || results.some(mod => mod.categories.includes(category))).map(category => <button key={category} className={browseCategory === category ? "active" : ""} aria-pressed={browseCategory === category} onClick={() => setBrowseCategory(category)}>{category === "all" ? "All categories" : category === "worldgen" ? "World generation" : category[0].toUpperCase() + category.slice(1)}</button>)}</div><label className="mods-sort-select">Sort <select value={browseSort} onChange={event => setBrowseSort(event.target.value as typeof browseSort)}><option value="downloads">Most downloaded</option><option value="name">Name A–Z</option></select></label></div>}
        {normalizedQuery.length === 1 && <p className="mod-search-hint">Type one more character to search Modrinth.</p>}
        {notice && <p className="profile-mod-note" role="status">{notice}</p>}{browseActionError && <div className="profile-mod-error" role="alert"><span>Could not install this mod. {browseActionError}</span></div>}{searchError && <div className="profile-mod-error" role="alert"><span>Modrinth search failed. {searchError}</span><button className="secondary-action" onClick={() => setSearchRetry(value => value + 1)}>Retry search</button></div>}
        {busy && <div className="mod-search-status" role="status">{normalizedQuery ? "Searching Modrinth…" : "Loading popular compatible mods…"}</div>}
        {busy && results.length === 0 && <div className="mod-search-skeletons" aria-hidden="true">{Array.from({ length: 4 }, (_, index) => <div key={index} />)}</div>}
        {!busy && (normalizedQuery.length >= 2 || normalizedQuery.length === 0) && !searchError && results.length === 0 && <div className="mods-empty"><Search size={18} /><strong>{normalizedQuery ? "No compatible mods found" : "No featured mods available"}</strong><p>{normalizedQuery ? "Try a shorter name, another spelling, or a different category." : "Check your connection or search for a mod by name."}</p></div>}
        {!busy && results.length > 0 && visibleResults.length === 0 && <div className="mods-empty"><Settings2 size={18} /><strong>No mods in this category</strong><p>Choose another category or show all categories.</p><button className="secondary-action" onClick={() => setBrowseCategory("all")}>Show all categories</button></div>}
        {(normalizedQuery.length >= 2 || normalizedQuery.length === 0) && results.length > 0 && <p className="mod-search-summary">{normalizedQuery ? `${visibleResults.length} compatible result${visibleResults.length === 1 ? "" : "s"}` : "Popular compatible mods"} · Fabric · Minecraft {minecraftVersion}</p>}
        <div className="modrinth-results">{visibleResults.map(mod => {
          const isInstalled = installedIds.has(mod.projectId);
          const hasUpdate = updateIds.has(mod.projectId);
          return <article className="modrinth-result" key={mod.projectId}>
            <ProjectIcon iconUrl={mod.iconUrl} label={mod.title} className="modrinth-icon-fallback" />
            <div className="mod-result-copy"><strong>{mod.title}</strong><small>by {mod.author}</small><p>{mod.description}</p><span><b>Fabric</b><i />Minecraft {minecraftVersion}{mod.downloads ? <><i />{formatCompact(mod.downloads)} downloads</> : null}</span></div>
            <button className={isInstalled && !hasUpdate ? "secondary-action" : "compact-play"} disabled={working !== null || (isInstalled && !hasUpdate)} onClick={() => void install(mod.projectId, mod.title, mod.slug)}>{working === mod.projectId ? "Installing…" : hasUpdate ? "Update" : isInstalled ? <><Check size={14} /> Installed</> : <><Download size={14} /> Install</>}</button>
          </article>;
        })}</div>
      </> : <section className="s9-packs-page" aria-label="S9 Client Packs">
        <div className="s9-packs-heading"><span><small>CURATED FOR S9 CLIENT</small><strong>SNine Packs</strong><p>Default SNine profiles install compatible packs automatically. Switch a pack off to disable its mods; files stay in the profile.</p></span><i className={packCatalogSource === "GitHub" ? "connected" : ""}><span />{packCatalogSource === "GitHub" ? "GitHub synced" : "Built-in list"}</i></div>
        {notice && <p className="profile-mod-note" role="status">{notice}</p>}{error && <p className="profile-mod-error" role="alert">{error}</p>}
        {compatiblePacks.length === 0 ? <div className="mods-empty"><PackageOpen size={18} /><strong>No packs for this profile yet</strong><p>Pack availability depends on its Minecraft version and loader.</p></div> : <div className="s9-pack-grid">{compatiblePacks.map(pack => {
          const included = new Set(pack.mods.map(mod => mod.projectId));
          const alreadyInstalled = [...included].filter(projectId => installedIds.has(projectId)).length;
          const enabled = packIsEnabled(pack);
          return <article className="s9-pack-card" key={pack.id}><div className="s9-pack-icon"><PackageOpen size={19} /></div><div className="s9-pack-copy"><h3>{pack.name}</h3><p>{pack.description}</p><small>{included.size} mods · Fabric · Minecraft {minecraftVersion}</small></div><div className="s9-pack-footer"><span>{enabled ? `${alreadyInstalled} of ${included.size} installed · enabled` : `${alreadyInstalled} installed · disabled`}</span><button type="button" role="switch" aria-checked={enabled} className={enabled ? "compact-play s9-pack-toggle active" : "secondary-action s9-pack-toggle"} disabled={packSyncing || busy || !canManageModrinthMods} onClick={() => void togglePack(pack)}>{packSyncing ? "Working…" : enabled ? <><Check size={14} /> On</> : <><Download size={14} /> Enable</>}</button></div></article>;
        })}</div>}
        {!canManageModrinthMods && <p className="profile-mod-note">S9 packs currently require a Fabric profile.</p>}
      </section>}
    </>}
    {removeTarget && <div className="modal-scrim" onClick={() => setRemoveTarget(null)}><section className="settings-popover mod-remove-dialog" role="alertdialog" aria-modal="true" aria-labelledby="remove-mod-heading" onClick={event => event.stopPropagation()}><div><div><small>PROFILE MODS</small><h2 id="remove-mod-heading">Remove {displayName(removeTarget)}?</h2></div></div><p>This removes the mod file from this profile. Other profile files and shared game data stay in place.</p><div className="uninstall-actions"><button className="secondary-action" disabled={working !== null} onClick={() => setRemoveTarget(null)}>Cancel</button><button className="destructive-action" disabled={working !== null} onClick={() => void remove()}>{working ? "Removing…" : "Remove mod"}</button></div></section></div>}
    {confirmUpdates && <div className="modal-scrim" onClick={() => setConfirmUpdates(false)}><section className="settings-popover mod-remove-dialog" role="alertdialog" aria-modal="true" aria-labelledby="update-mods-heading" onClick={event => event.stopPropagation()}><div><div><small>PROFILE MODS</small><h2 id="update-mods-heading">Update {updateIds.size} mod{updateIds.size === 1 ? "" : "s"}?</h2></div></div><p>Each mod will be downloaded and verified through Modrinth. Your current files remain managed by the launcher.</p><div className="uninstall-actions"><button className="secondary-action" disabled={busy} onClick={() => setConfirmUpdates(false)}>Cancel</button><button className="compact-play" disabled={busy} onClick={() => void updateAll()}>{busy ? "Updating…" : "Update"}</button></div></section></div>}
    {manifestOpen && <div className="modal-scrim" onClick={() => setManifestOpen(false)}><section className="settings-popover mod-remove-dialog" role="dialog" aria-modal="true" aria-labelledby="import-pack-heading" onClick={event => event.stopPropagation()}><div><div><small>SNINE MODPACK</small><h2 id="import-pack-heading">Import a modpack</h2></div><button className="subtle-icon" aria-label="Close" onClick={() => setManifestOpen(false)}><X size={16} /></button></div><p>Paste a SNine manifest. Imports add compatible Modrinth projects and keep existing files intact.</p><textarea className="modpack-manifest-input" value={manifestText} onChange={event => setManifestText(event.target.value)} placeholder="Paste modpack JSON here" /><div className="uninstall-actions"><button className="secondary-action" onClick={() => setManifestOpen(false)}>Cancel</button><button className="compact-play" disabled={busy || !manifestText.trim() || !canManageModrinthMods} onClick={() => void importManifest()}>{busy ? "Importing…" : "Validate and import"}</button></div></section></div>}
    {confirmBulkRemove && <div className="modal-scrim" onClick={() => setConfirmBulkRemove(false)}><section className="settings-popover mod-remove-dialog" role="alertdialog" aria-modal="true" aria-labelledby="bulk-remove-heading" onClick={event => event.stopPropagation()}><div><div><small>PROFILE MODS</small><h2 id="bulk-remove-heading">Remove {selectedFiles.length} selected mods?</h2></div></div><p>This removes only the selected mod files from this profile.</p><div className="uninstall-actions"><button className="secondary-action" onClick={() => setConfirmBulkRemove(false)}>Cancel</button><button className="destructive-action" disabled={busy} onClick={() => void bulkAction("remove")}>Remove mods</button></div></section></div>}
  </div>;
}

function displayName(mod: ManagedMod): string {
  const base = mod.filename.replace(/\.jar$/i, "").replace(/[-_]+/g, " ").replace(/\s+\d[\w.+-]*$/, "").trim();
  const label = base || mod.modId.replace(/[-_]+/g, " ");
  return label.replace(/\b\w/g, character => character.toUpperCase());
}
function modIconLookupIds(mod: ManagedMod): string[] {
  const primary = mod.projectId || mod.modId;
  if (mod.managed) return [...new Set([primary, mod.slug].filter(Boolean))];
  const filenameSlug = mod.filename.replace(/\.jar(?:\.disabled)?$/i, "").replace(/[-_]?v?\d[\w.+-]*$/i, "").replace(/[._\s]+/g, "-").toLowerCase();
  return [...new Set([mod.modId, filenameSlug].filter(Boolean))];
}
function ProjectIcon({ iconUrl, fallbackIconUrl, label, className }: { iconUrl?: string; fallbackIconUrl?: string; label: string; className: string }) {
  const [sourceIndex, setSourceIndex] = useState(0);
  const sources = [...new Set([iconUrl, fallbackIconUrl].filter((source): source is string => Boolean(source)))];
  useEffect(() => setSourceIndex(0), [iconUrl, fallbackIconUrl]);
  const source = sources[sourceIndex];
  if (source) return <img className={className} src={source} alt="" title={label} loading="lazy" decoding="async" referrerPolicy="no-referrer" onError={() => setSourceIndex(index => index + 1)} />;
  return <span className={`${className} modrinth-icon-fallback`} role="img" aria-label={`${label} icon unavailable`}><Sparkles size={17} /></span>;
}
function toMessage(reason: unknown): string { return reason instanceof Error ? reason.message.replace(/^Error: /, "") : String(reason).replace(/^Error: /, ""); }
function formatCompact(value: number): string { return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(value); }
