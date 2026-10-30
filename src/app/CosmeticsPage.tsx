import { useCallback, useEffect, useState } from "react";
import { Check, EyeOff, RefreshCw, Shirt, Sparkles, Upload } from "lucide-react";
import { ipc, type AccountCosmetics, type SafeAccount } from "../lib/ipc";
import { Skin3DPreview } from "./Skin3DPreview";

type Props = { account: SafeAccount | null; onSignIn: () => void };

export function CosmeticsPage({ account, onSignIn }: Props) {
  const [cosmetics, setCosmetics] = useState<AccountCosmetics | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [previewSkinId, setPreviewSkinId] = useState<string | null>(null);
  const [previewCapeId, setPreviewCapeId] = useState<string | null>(null);
  const [skinVariant, setSkinVariant] = useState<"classic" | "slim">("classic");

  const refresh = useCallback(async () => {
    if (!account) { setCosmetics(null); return; }
    setLoading(true); setError("");
    try { setCosmetics(await ipc.authGetAccountCosmetics()); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setLoading(false); }
  }, [account?.uuid]);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    if (!cosmetics) return;
    setPreviewSkinId(current => current && cosmetics.skins.some(skin => skin.id === current)
      ? current : cosmetics.skins.find(skin => skin.state.toUpperCase() === "ACTIVE")?.id ?? cosmetics.skins[0]?.id ?? null);
    setPreviewCapeId(current => current === "none" || (current && cosmetics.capes.some(cape => cape.id === current))
      ? current : cosmetics.capes.find(cape => cape.state.toUpperCase() === "ACTIVE")?.id ?? null);
  }, [cosmetics]);

  const runAction = async (action: () => Promise<void>, success: string) => {
    setBusy(true); setError(""); setNotice("");
    try { await action(); setNotice(success); await refresh(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };

  const uploadSkin = async () => {
    const path = await ipc.chooseSkinFile();
    if (!path) return;
    if (!window.confirm(`Upload this ${skinVariant === "slim" ? "Slim" : "Classic"}-model skin to ${cosmetics?.username ?? account?.username}? This changes the skin on your Minecraft account.`)) return;
    await runAction(() => ipc.authUploadSkin(path, skinVariant), "Skin uploaded to your Minecraft account.");
  };

  if (!account) return <section className="cosmetics-page">
    <header className="cosmetics-heading"><div><span>YOUR MINECRAFT PROFILE</span><h1>Cosmetics</h1><p>View and manage the skins and capes owned by your Minecraft account.</p></div><div className="cosmetics-mark"><Sparkles size={22} /></div></header>
    <div className="cosmetics-empty"><div className="empty-symbol"><Shirt size={20} /></div><strong>Connect a Minecraft account</strong><p>Sign in to load your owned skins and capes.</p><button className="compact-play" onClick={onSignIn}>Sign in with Microsoft</button></div>
  </section>;

  return <section className="cosmetics-page">
    <header className="cosmetics-heading"><div><span>YOUR MINECRAFT PROFILE</span><h1>Cosmetics</h1><p>Manage the skins and capes owned by {cosmetics?.username ?? account.username}.</p></div><button className="secondary-action" disabled={loading || busy} onClick={() => void refresh()}><RefreshCw size={14} />{loading ? "Refreshing…" : "Refresh"}</button></header>
    {loading && !cosmetics && <div className="cosmetics-empty" role="status"><div className="cosmetic-loading" /><strong>Loading Minecraft cosmetics…</strong><p>Your account token stays inside the launcher.</p></div>}
    {error && <div className="cosmetics-message error" role="alert">{error}<button className="secondary-action" disabled={loading} onClick={() => void refresh()}>Try again</button></div>}
    {notice && <div className="cosmetics-message" role="status"><Check size={15} />{notice}</div>}
    {cosmetics && <>
      {(() => {
        const previewSkin = cosmetics.skins.find(skin => skin.id === previewSkinId) ?? cosmetics.skins[0];
        const previewCape = previewCapeId === "none" ? undefined : cosmetics.capes.find(cape => cape.id === previewCapeId) ?? cosmetics.capes.find(cape => cape.state.toUpperCase() === "ACTIVE");
        return previewSkin ? <div className="cosmetics-model-showcase">
          <Skin3DPreview className="cosmetics-model-canvas" skinUrl={previewSkin.url} variant={previewSkin.variant} capeUrl={previewCape?.url} />
          <div className="cosmetics-model-copy"><span>3D PLAYER PREVIEW</span><h2>{previewSkin.variant.toLowerCase() === "slim" ? "Alex model" : "Steve model"}</h2><p>Drag the player to rotate. The preview uses your Minecraft skin texture{previewCape ? " and selected cape" : ""}.</p><div className="cosmetics-model-selection"><small>Skin</small><strong>{previewSkin.state.toUpperCase() === "ACTIVE" ? "Active skin" : `Skin ${previewSkin.id.slice(0, 8)}`}</strong><small>Cape</small><strong>{previewCape?.alias || "No cape"}</strong></div><button className="secondary-action" onClick={() => setPreviewCapeId(previewCape ? "none" : null)}>{previewCape ? "Preview without cape" : "Preview active cape"}</button></div>
        </div> : <div className="cosmetics-model-empty"><Shirt size={20} /><strong>No skin available for a 3D preview</strong><p>Choose or add a Minecraft skin to preview your player model.</p></div>;
      })()}
      <div className="cosmetics-section-title"><div><span>SKIN COLLECTION</span><h2>Owned skins</h2></div><div className="skin-upload-controls"><label>Model<select aria-label="Skin player model" value={skinVariant} onChange={event => setSkinVariant(event.target.value as "classic" | "slim")}><option value="classic">Classic</option><option value="slim">Slim</option></select></label><button className="secondary-action" disabled={busy || loading} onClick={() => void uploadSkin()}><Upload size={14} />Upload PNG skin</button><span className="cosmetics-count">{cosmetics.skins.length}</span></div></div>
      {cosmetics.skins.length === 0 ? <div className="cosmetics-empty compact"><Shirt size={19} /><strong>No skins returned</strong><p>Refresh after checking your Minecraft account online.</p></div> : <div className="cosmetic-grid">{cosmetics.skins.map(skin => {
        const active = skin.state.toUpperCase() === "ACTIVE";
        return <article className={`cosmetic-card ${active ? "active" : ""}`} key={skin.id}>
          <div className="cosmetic-card-top"><button className="cosmetic-texture-frame cosmetic-preview-button" aria-label={`Preview ${skin.variant.toLowerCase()} skin in 3D`} onClick={() => setPreviewSkinId(skin.id)}><img src={skin.url} alt="" loading="lazy" referrerPolicy="no-referrer" /></button><span className={`cosmetic-state ${active ? "active" : ""}`}>{active ? "Active" : "Owned"}</span></div>
          <strong>{skin.variant.toLowerCase() === "slim" ? "Slim · Alex model" : "Classic · Steve model"}</strong>
          <small>Skin ID {skin.id.slice(0, 8)}</small>
          <div className="cosmetic-actions"><button className="secondary-action" onClick={() => setPreviewSkinId(skin.id)}>{previewSkinId === skin.id ? "Previewing" : "Preview"}</button><button className={active ? "secondary-action" : "compact-play"} disabled={busy || active} onClick={() => void runAction(() => ipc.authActivateSkin(skin.id), "Active skin updated.")}>{active ? <><Check size={14} /> Equipped</> : "Use skin"}</button></div>
        </article>;
      })}</div>}

      <div className="cosmetics-section-title cape-title"><div><span>PROFILE COSMETICS</span><h2>Capes</h2></div><span className="cosmetics-count">{cosmetics.capes.length}</span></div>
      <div className="cape-controls"><p>Choose a cape owned by this account, or hide the cape in game.</p><button className="secondary-action" disabled={busy || !cosmetics.capes.some(cape => cape.state.toUpperCase() === "ACTIVE")} onClick={() => void runAction(() => ipc.authHideCape(), "Cape hidden.")}><EyeOff size={14} /> Hide active cape</button></div>
      {cosmetics.capes.length === 0 ? <div className="cosmetics-empty compact"><Sparkles size={19} /><strong>No capes on this account</strong><p>When you earn or redeem a cape, it will appear here.</p></div> : <div className="cape-grid">{cosmetics.capes.map(cape => {
        const active = cape.state.toUpperCase() === "ACTIVE";
        return <article className={`cape-card ${active ? "active" : ""}`} key={cape.id}><img src={cape.url} alt={`${cape.alias || "Minecraft"} cape texture`} loading="lazy" referrerPolicy="no-referrer" /><div><strong>{cape.alias || "Minecraft cape"}</strong><small>{active ? "Currently equipped" : "Owned by this account"}</small></div><button className="secondary-action" onClick={() => setPreviewCapeId(cape.id)}>{previewCapeId === cape.id ? "Previewing" : "Preview"}</button><button className={active ? "secondary-action" : "compact-play"} disabled={busy || active} onClick={() => void runAction(() => ipc.authActivateCape(cape.id), "Active cape updated.")}>{active ? <><Check size={14} /> Active</> : "Equip"}</button></article>;
      })}</div>}
      <p className="cosmetics-footnote">Changes are applied to your Minecraft account and may take a moment to appear in game.</p>
    </>}
  </section>;
}
