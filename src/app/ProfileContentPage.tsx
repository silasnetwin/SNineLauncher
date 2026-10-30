import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, Copy, ExternalLink, FolderOpen, Globe2, Image, Link2, PackageOpen, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { ipc, type ProfileContentItem, type ProfileContentSection, type ProfileLogTail, type ProfileSymlink } from "../lib/ipc";

type ContentTab = ProfileContentSection | "symlinks";
const sections: Array<{ id: ContentTab; label: string }> = [
  { id: "shaderpacks", label: "Shader packs" },
  { id: "datapacks", label: "Data packs" },
  { id: "worlds", label: "Worlds" },
  { id: "screenshots", label: "Screenshots" },
  { id: "logs", label: "Logs" },
  { id: "symlinks", label: "Symlinks" },
];

export function ProfileContentPage({ profileId, installed, running = false, initialSection = "shaderpacks" }: { profileId: string; installed: boolean; running?: boolean; initialSection?: ContentTab }) {
  const [section, setSection] = useState<ContentTab>(initialSection);
  const [items, setItems] = useState<ProfileContentItem[]>([]);
  const [world, setWorld] = useState("");
  const [worlds, setWorlds] = useState<ProfileContentItem[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [links, setLinks] = useState<ProfileSymlink[]>([]);
  const [sourcePath, setSourcePath] = useState("");
  const [linkName, setLinkName] = useState("");
  const [linkKind, setLinkKind] = useState<"folder" | "file">("folder");

  const refreshWorlds = useCallback(async (): Promise<ProfileContentItem[]> => {
    try {
      const entries = await ipc.getProfileContent(profileId, "worlds");
      const valid = entries.filter(item => item.directory);
      setWorlds(valid);
      setWorld(current => valid.some(item => item.name === current) ? current : valid[0]?.name ?? "");
      return valid;
    } catch (reason) { setError(String(reason)); }
    return [];
  }, [profileId]);

  const refresh = useCallback(async () => {
    if (!installed) { setItems([]); return; }
    setError("");
    try {
      if (section === "symlinks") { setLinks(await ipc.getProfileSymlinks(profileId)); setItems([]); return; }
      let selectedWorld = world;
      if (section === "worlds" || section === "datapacks") {
        const availableWorlds = await refreshWorlds();
        if (section === "datapacks" && !availableWorlds.some(item => item.name === selectedWorld)) {
          selectedWorld = availableWorlds[0]?.name ?? "";
          setWorld(selectedWorld);
        }
      }
      const result = await ipc.getProfileContent(profileId, section, section === "datapacks" ? selectedWorld || undefined : undefined);
      setItems(result);
    } catch (reason) { setItems([]); setError(String(reason)); }
  }, [installed, profileId, refreshWorlds, section, world]);

  useEffect(() => { void refresh(); }, [refresh]);


  const importFile = async () => {
    if (section !== "shaderpacks" && section !== "datapacks") return;
    if (section === "datapacks" && !world) { setError("Create or select a world before adding a data pack."); return; }
    const path = await ipc.selectProfileContentFile(section);
    if (!path) return;
    setBusy(true); setError(""); setMessage("");
    try { await ipc.importProfileContent(profileId, section, path, section === "datapacks" ? world : undefined); await refresh(); setMessage("Added to this profile."); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  const remove = async (item: ProfileContentItem) => {
    if (section === "symlinks") return;
    if (!window.confirm(`Remove ${item.name} from this profile?`)) return;
    setBusy(true); setError("");
    try { await ipc.removeProfileContent(profileId, section, item.name, section === "datapacks" ? world : undefined); await refresh(); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  const chooseLinkSource = async (kind: "folder" | "file") => {
    setLinkKind(kind);
    try {
      const path = await ipc.selectSymlinkSource(kind === "folder");
      if (path) { setSourcePath(path); setLinkName(path.split(/[\\/]/).filter(Boolean).at(-1) ?? ""); }
    } catch (reason) { setError(String(reason)); }
  };

  const createLink = async () => {
    if (!sourcePath || !linkName.trim()) return;
    setBusy(true); setError(""); setMessage("");
    try { await ipc.createProfileSymlink(profileId, sourcePath, linkName.trim()); setSourcePath(""); setLinkName(""); await refresh(); setMessage("Link added to this profile."); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  const canImport = section === "shaderpacks" || section === "datapacks";
  const icon = section === "worlds" ? <Globe2 size={17} /> : section === "screenshots" ? <Image size={17} /> : <PackageOpen size={17} />;
  return <div className="detail-overview profile-content-page">
    <div className="detail-section-heading"><div><span>PROFILE FILES</span><h2>Game content</h2></div></div>
    <div className="profile-content-tabs" role="tablist" aria-label="Profile game content">
      {sections.map(item => <button key={item.id} role="tab" aria-selected={section === item.id} className={section === item.id ? "active" : ""} onClick={() => { setSection(item.id); setMessage(""); setError(""); }}>{item.label}</button>)}
    </div>
    {!installed ? <p className="profile-mod-note">Install this profile to manage its game files.</p> : <>
      {section === "symlinks" ? <div className="profile-symlink-panel">
        <p>Link an existing file or folder into this profile. The selected source stays in its current location; removing a link never deletes its source.</p>
        <div className="profile-symlink-form"><div><button className={linkKind === "folder" ? "compact-play" : "secondary-action"} disabled={busy} onClick={() => void chooseLinkSource("folder")}><FolderOpen size={14} /> Choose folder</button><button className={linkKind === "file" ? "compact-play" : "secondary-action"} disabled={busy} onClick={() => void chooseLinkSource("file")}><ExternalLink size={14} /> Choose file</button><span>{sourcePath || "No source selected"}</span></div><label>Link name<input value={linkName} onChange={event => setLinkName(event.target.value)} maxLength={180} placeholder="Name inside this profile" /></label><button className="compact-play" disabled={busy || !sourcePath || !linkName.trim()} onClick={() => void createLink()}><Link2 size={14} /> Create link</button></div>
        {links.length === 0 ? <div className="mods-empty"><Link2 size={18} /><strong>No profile links</strong><p>Choose a file or folder above to share it with this Minecraft instance.</p></div> : <div className="profile-content-list">{links.map(link => <article key={link.name}><span className="profile-content-icon"><Link2 size={16} /></span><span><strong>{link.name}</strong><small>{link.directory ? "Folder link" : "File link"} · {link.targetExists ? "Target available" : "Target missing"}</small></span><button className="subtle-icon" title="Remove link" disabled={busy} onClick={() => void ipc.removeProfileSymlink(profileId, link.name).then(refresh).catch(reason => setError(String(reason)))}><Trash2 size={15} /></button></article>)}</div>}
      </div> : <>
      <div className="profile-content-toolbar">
        {section === "datapacks" ? <label>World <select value={world} onChange={event => setWorld(event.target.value)}><option value="">Select a world…</option>{worlds.map(item => <option key={item.name} value={item.name}>{item.name}</option>)}</select></label> : <span>Stored in this profile</span>}
        <div>
          <button className="secondary-action" disabled={busy} title="Refresh this folder" onClick={() => void refresh()}><RefreshCw size={14} /> Refresh</button>
          {(section === "worlds" || section === "screenshots" || section === "logs" || section === "shaderpacks" || (section === "datapacks" && world)) && <button className="secondary-action" disabled={busy} onClick={() => void ipc.openProfileContentFolder(profileId, section, section === "datapacks" ? world : undefined).catch(reason => setError(String(reason)))}><FolderOpen size={14} /> Open folder</button>}
          {canImport && <button className="compact-play" disabled={busy || (section === "datapacks" && !world)} onClick={() => void importFile()}><Plus size={14} />{busy ? "Adding…" : `Add ${section === "shaderpacks" ? "shader pack" : "data pack"}`}</button>}
        </div>
      </div>
      {message && <p className="profile-mod-note" role="status">{message}</p>}{error && <p className="profile-mod-error" role="alert">{error}</p>}
      {section === "logs" ? <ProfileLogsConsole profileId={profileId} files={items.filter(item => !item.directory && item.name.toLowerCase().endsWith(".log"))} running={running} />
        : section === "datapacks" && !world ? <div className="mods-empty"><Globe2 size={18} /><strong>No world selected</strong><p>Data packs belong to a specific Minecraft world.</p></div>
          : items.length === 0 ? <div className="mods-empty"><span>{icon}</span><strong>{section === "worlds" ? "No worlds found" : section === "screenshots" ? "No screenshots found" : `No ${section === "shaderpacks" ? "shader packs" : "data packs"} added`}</strong><p>{section === "datapacks" ? "Add a ZIP data pack to the selected world's datapacks folder." : section === "shaderpacks" ? "Add a ZIP or JAR shader pack, then enable it from the in-game video settings." : "Files will appear here when Minecraft creates them."}</p></div>
            : <div className="profile-content-list">{items.map(item => (
              <article key={item.name}>
                <span className="profile-content-icon">{item.directory ? <Globe2 size={16} /> : icon}</span>
                <span><strong>{item.name}</strong><small>{item.directory ? "Minecraft world" : formatBytes(item.bytes)}</small></span>
                <button className="subtle-icon" title={item.directory ? "Open world folder" : "Open file"} onClick={() => void ipc.openProfileContentItem(profileId, section, item.name, section === "datapacks" ? world : undefined).catch(reason => setError(String(reason)))}><ExternalLink size={15} /></button>
                {canImport && <button className="subtle-icon" title="Remove" disabled={busy || item.directory} onClick={() => void remove(item)}><Trash2 size={15} /></button>}
              </article>
            ))}</div>}
      </>}
    </>}
  </div>;
}

function ProfileLogsConsole({ profileId, files, running }: { profileId: string; files: ProfileContentItem[]; running: boolean }) {
  const [selected, setSelected] = useState("");
  const [tail, setTail] = useState<ProfileLogTail | null>(null);
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState<"All" | "Errors" | "Warnings">("All");
  const [follow, setFollow] = useState(true);
  const [refreshKey, setRefreshKey] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    setSelected(current => files.some(file => file.name === current) ? current : files.find(file => file.name === "latest.log")?.name ?? files[0]?.name ?? "");
  }, [files]);
  useEffect(() => {
    if (!selected) { setTail(null); setError(""); return; }
    let active = true;
    let inFlight = false;
    const load = async (quiet: boolean) => {
      if (inFlight) return;
      inFlight = true;
      if (!quiet) setBusy(true);
      try { const result = await ipc.readMinecraftProfileLog(profileId, selected); if (active) { setTail(result); setError(""); } }
      catch (reason) { if (active) setError(String(reason).replace(/^Error: /, "")); }
      finally { inFlight = false; if (active && !quiet) setBusy(false); }
    };
    void load(false);
    const timer = running ? window.setInterval(() => void load(true), 3000) : undefined;
    return () => { active = false; if (timer !== undefined) window.clearInterval(timer); };
  }, [profileId, selected, running, refreshKey]);
  const visibleLines = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return (tail?.content.replace(/\r/g, "").split("\n") ?? []).filter(line => {
      if (needle && !line.toLocaleLowerCase().includes(needle)) return false;
      if (level === "Errors" && !/(\bERROR\b|\bFATAL\b|Exception|Caused by:)/i.test(line)) return false;
      if (level === "Warnings" && !/\bWARN(?:ING)?\b/i.test(line)) return false;
      return true;
    });
  }, [tail, query, level]);
  useEffect(() => { if (follow && logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight; }, [visibleLines, follow]);
  useEffect(() => { if (!notice) return; const timer = window.setTimeout(() => setNotice(""), 2200); return () => window.clearTimeout(timer); }, [notice]);
  const copyVisible = async () => {
    try { await navigator.clipboard.writeText(visibleLines.join("\n")); setNotice(`Copied ${visibleLines.length.toLocaleString()} visible lines.`); }
    catch { setError("Clipboard access is unavailable."); }
  };
  if (files.length === 0) return <div className="profile-log-empty"><strong>No game logs found</strong><p>Start Minecraft once, then refresh this folder to inspect its log.</p></div>;
  return <section className="profile-log-console" aria-label="Minecraft log viewer">
    <div className="profile-log-toolbar"><label>Log file<select aria-label="Select log file" value={selected} onChange={event => setSelected(event.target.value)}>{files.map(file => <option key={file.name} value={file.name}>{file.name}</option>)}</select></label><span className={`profile-log-live ${running ? "active" : ""}`}><i />{running ? "Live · refreshes every 3s" : "Paused · game not running"}</span><button className="secondary-action" disabled={busy || !selected} onClick={() => setRefreshKey(value => value + 1)}><RefreshCw size={14} /> Refresh</button><button className="secondary-action" disabled={!tail || visibleLines.length === 0} onClick={() => void copyVisible()}><Copy size={14} /> Copy visible</button></div>
    <div className="profile-log-filters"><label className="search-field"><Search size={14} /><input aria-label="Search logs" value={query} onChange={event => setQuery(event.target.value)} placeholder="Search log text…" /></label><div role="group" aria-label="Filter log severity">{(["All", "Errors", "Warnings"] as const).map(option => <button key={option} className={level === option ? "active" : ""} aria-pressed={level === option} onClick={() => setLevel(option)}>{option}</button>)}</div><button className="profile-log-follow" aria-pressed={follow} onClick={() => setFollow(value => !value)}>{follow ? <Check size={13} /> : null} Follow latest</button></div>
    {tail?.truncated && <p className="profile-log-note" role="status">Showing the last 1 MiB of {tail.fileName} ({formatBytes(tail.bytes)} total).</p>}{error && <p className="profile-mod-error" role="alert">{error}</p>}{notice && <p className="profile-mod-note" role="status">{notice}</p>}
    {busy && !tail ? <div className="mods-empty">Loading log…</div> : <><div className="profile-log-summary">{tail?.fileName ?? selected} · {visibleLines.length.toLocaleString()} lines{tail ? ` · ${formatBytes(tail.bytes)}` : ""}</div><pre className="profile-log-output" ref={logRef} aria-label="Filtered Minecraft log output"><code>{visibleLines.map((line, index) => <span key={index} className={/(\bERROR\b|\bFATAL\b|Exception|Caused by:)/i.test(line) ? "error" : /\bWARN(?:ING)?\b/i.test(line) ? "warning" : ""}>{line || " "}{"\n"}</span>)}</code></pre></>}
  </section>;
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB"];
  let index = -1; let amount = value;
  do { amount /= 1024; index++; } while (amount >= 1024 && index < units.length - 1);
  return `${amount.toFixed(amount < 10 ? 1 : 0)} ${units[index]}`;
}
