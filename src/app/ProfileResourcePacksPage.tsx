import { useEffect, useMemo, useRef, useState } from "react";
import { Check, Download, ExternalLink, PackageOpen, Search, Trash2, Upload } from "lucide-react";
import { ipc, type ModrinthProject, type ResourcePackInfo } from "../lib/ipc";

const packSearchCache = new Map<string, { at: number; items: ModrinthProject[] }>();

export function ProfileResourcePacksPage({ profileId, minecraftVersion, installed }: { profileId: string; minecraftVersion: string; installed: boolean }) {
  const [packs, setPacks] = useState<ResourcePackInfo[]>([]);
  const [view, setView] = useState<"Installed" | "Browse">("Installed");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ModrinthProject[]>([]);
  const [searching, setSearching] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [installingProject, setInstallingProject] = useState<string | null>(null);
  const [removeTarget, setRemoveTarget] = useState<ResourcePackInfo | null>(null);
  const searchRequest = useRef(0);
  const installedProjects = useMemo(() => new Set(packs.map(pack => pack.filename.match(/^snine-modrinth-([A-Za-z0-9_-]+)\.zip$/)?.[1]).filter((id): id is string => !!id)), [packs]);
  useEffect(() => {
    let active = true;
    if (!installed) { setPacks([]); setError(""); return () => { active = false; }; }
    setError("");
    void ipc.getProfileResourcePacks(profileId).then(value => { if (active) setPacks(value); }).catch(reason => { if (active) setError(message(reason)); });
    return () => { active = false; };
  }, [profileId, installed]);
  useEffect(() => {
    const id = ++searchRequest.current;
    const term = query.trim();
    if (view !== "Browse" || term.length < 2) { setResults([]); setSearching(false); setError(""); return; }
    setError("");
    const key = `${term.toLocaleLowerCase()}\n${minecraftVersion}`;
    const cached = packSearchCache.get(key);
    if (cached) setResults(cached.items);
    if (cached && Date.now() - cached.at < 300_000) { setSearching(false); return; }
    setSearching(!cached);
    const timer = window.setTimeout(() => {
      void ipc.searchModrinthResourcePacks(term, minecraftVersion).then(items => {
        if (searchRequest.current === id) { setResults(items); setError(""); packSearchCache.set(key, { at: Date.now(), items }); }
      }).catch(reason => { if (searchRequest.current === id) setError(message(reason)); }).finally(() => { if (searchRequest.current === id) setSearching(false); });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [view, query, profileId, minecraftVersion]);
  const importPack = async () => {
    setBusy(true); setError(""); setNotice("");
    try {
      const sourcePath = await ipc.selectResourcePackFile();
      if (!sourcePath) return;
      const saved = await ipc.importProfileResourcePack(profileId, sourcePath);
      setPacks(current => [...current.filter(pack => pack.filename !== saved.filename), saved].sort((a, b) => a.name.localeCompare(b.name)));
      setNotice(`${saved.name} added to this profile.`);
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const remove = async () => {
    if (!removeTarget) return;
    setBusy(true); setError("");
    try { await ipc.removeProfileResourcePack(profileId, removeTarget.filename); setPacks(current => current.filter(pack => pack.filename !== removeTarget.filename)); setNotice(`${removeTarget.name} removed.`); setRemoveTarget(null); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const installProject = async (project: ModrinthProject) => {
    setInstallingProject(project.projectId); setError(""); setNotice("");
    try { const pack = await ipc.installModrinthResourcePack(profileId, project.projectId); setPacks(current => [...current.filter(item => item.filename !== pack.filename), pack].sort((a, b) => a.name.localeCompare(b.name))); setNotice(`${pack.name} installed.`); }
    catch (reason) { setError(message(reason)); }
    finally { setInstallingProject(null); }
  };
  return <div className="detail-overview profile-mods-page resource-packs-page">
    <div className="detail-section-heading"><div><span>PROFILE CONTENT</span><h2>Resource Packs</h2></div></div>
    {!installed ? <p className="profile-mod-note">Install this profile before managing resource packs.</p> : <>
      <div className="mods-subtabs" role="tablist" aria-label="Resource pack views"><button role="tab" aria-selected={view === "Installed"} className={view === "Installed" ? "active" : ""} onClick={() => setView("Installed")}>Installed <span>{packs.length}</span></button><button role="tab" aria-selected={view === "Browse"} className={view === "Browse" ? "active" : ""} onClick={() => setView("Browse")}>Browse Modrinth</button></div>
      {view === "Installed" ? <><div className="resource-pack-toolbar"><span>Available in Minecraft’s Resource Packs menu</span><div><button className="secondary-action" disabled={busy} onClick={() => void ipc.openProfileResourcePacks(profileId).catch(reason => setError(message(reason)))}><ExternalLink size={14} /> Open folder</button><button className="compact-play" disabled={busy} onClick={() => void importPack()}><Upload size={14} />{busy ? "Adding…" : "Add from computer"}</button></div></div>
      <p className="profile-mod-note">Imported ZIP packs are available in-game. Enable or order them from Minecraft’s Resource Packs screen.</p>
      {notice && <p className="profile-mod-note" role="status">{notice}</p>}{error && <p className="profile-mod-error" role="alert">{error}</p>}
      {packs.length === 0 ? <div className="mods-empty"><PackageOpen size={18} /><strong>No resource packs added</strong><p>Import a ZIP pack or open the profile folder to manage files.</p><button className="secondary-action" onClick={() => void ipc.openProfileResourcePacks(profileId).catch(reason => setError(message(reason)))}><ExternalLink size={14} /> Open resource pack folder</button></div> : <div className="resource-pack-list">{packs.map(pack => <article className="resource-pack-row" key={pack.filename}><div className="mod-row-icon"><PackageOpen size={16} /></div><span><strong>{pack.name}</strong><small>{pack.description} · {pack.packFormat === null ? "Pack format unknown" : `Format ${pack.packFormat}`} · {formatBytes(pack.bytes)}</small></span><span className="resource-pack-state">Available in game</span><button className="subtle-icon mod-remove-action" aria-label={`Remove ${pack.name}`} disabled={busy} onClick={() => setRemoveTarget(pack)}><Trash2 size={15} /></button></article>)}</div>}
      </> : <><label className="search-field profile-mod-search"><Search size={16} /><input autoFocus value={query} onChange={event => setQuery(event.target.value)} aria-label="Search Modrinth resource packs" placeholder="Search compatible resource packs" /></label><p className="profile-mod-note">Resource packs compatible with Minecraft {minecraftVersion}.</p>{searching && <div className="mod-search-status" role="status">Searching Modrinth…</div>}{error && <p className="profile-mod-error" role="alert">Modrinth couldn't be reached. {error}</p>}{query.trim().length < 2 && <div className="mods-empty compact"><Search size={18} /><strong>Browse resource packs</strong><p>Search by pack name or theme.</p></div>}{!searching && query.trim().length >= 2 && !error && !results.length && <div className="mods-empty"><PackageOpen size={18} /><strong>No compatible resource packs found</strong><p>Try another search.</p></div>}<div className="modrinth-results">{results.map(project => { const isInstalled = installedProjects.has(project.projectId); return <article className="modrinth-result" key={project.projectId}>{project.iconUrl ? <img src={project.iconUrl} loading="lazy" decoding="async" alt="" referrerPolicy="no-referrer" /> : <div className="modrinth-icon-fallback"><PackageOpen size={16} /></div>}<div className="mod-result-copy"><strong>{project.title}</strong><small>by {project.author}{project.downloads ? ` · ${new Intl.NumberFormat(undefined, { notation: "compact" }).format(project.downloads)} downloads` : ""}</small><p>{project.description}</p><span>Minecraft · {minecraftVersion}</span></div><button className={isInstalled ? "secondary-action" : "compact-play"} disabled={busy || installingProject !== null || isInstalled} onClick={() => void installProject(project)}>{installingProject === project.projectId ? "Installing…" : isInstalled ? <><Check size={14} /> Installed</> : <><Download size={14} /> Install</>}</button></article>; })}</div></>}
    </>}
    {removeTarget && <div className="modal-scrim" onClick={() => setRemoveTarget(null)}><section className="settings-popover mod-remove-dialog" role="alertdialog" aria-modal="true" aria-labelledby="remove-pack-heading" onClick={event => event.stopPropagation()}><div><div><small>PROFILE RESOURCE PACKS</small><h2 id="remove-pack-heading">Remove {removeTarget.name}?</h2></div></div><p>This removes the ZIP from this profile’s resource pack folder.</p><div className="uninstall-actions"><button className="secondary-action" disabled={busy} onClick={() => setRemoveTarget(null)}>Cancel</button><button className="destructive-action" disabled={busy} onClick={() => void remove()}>{busy ? "Removing…" : "Remove pack"}</button></div></section></div>}
  </div>;
}
function message(reason: unknown): string { return reason instanceof Error ? reason.message.replace(/^Error: /, "") : String(reason).replace(/^Error: /, ""); }
function formatBytes(value: number): string { if (value < 1024) return `${value} B`; return `${(value / 1024 / 1024).toFixed(1)} MB`; }
