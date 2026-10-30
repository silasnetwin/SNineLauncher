import { useEffect, useState, type CSSProperties } from "react";
import { ipc } from "../lib/ipc";

const textures = new Map<string, string>();
const pending = new Map<string, Promise<string | null>>();
const failedTextures = new Set<string>();

export function MinecraftHead({ uuid, name, className, small = false }: {
  uuid: string;
  name: string;
  className: string;
  small?: boolean;
}) {
  const normalizedUuid = uuid.replaceAll("-", "").toLowerCase();
  const [texture, setTexture] = useState<string | null>(textures.get(normalizedUuid) ?? null);
  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    let mounted = true;
    setTexture(textures.get(normalizedUuid) ?? null);
    setImageFailed(failedTextures.has(normalizedUuid));
    if (!/^[a-f\d]{32}$/.test(normalizedUuid) || textures.has(normalizedUuid) || failedTextures.has(normalizedUuid)) {
      setTexture(textures.get(normalizedUuid) ?? null);
      return () => { mounted = false; };
    }
    let request = pending.get(normalizedUuid);
    if (!request) {
      request = ipc.authGetSkinTexture(normalizedUuid)
        .then(url => {
          if (url) textures.set(normalizedUuid, url);
          return url;
        })
        .catch(() => null)
        .finally(() => pending.delete(normalizedUuid));
      pending.set(normalizedUuid, request);
    }
    void request.then(url => { if (mounted) setTexture(url); });
    return () => { mounted = false; };
  }, [normalizedUuid]);

  const style = { "--avatar-hue": `${stableHue(name)}deg`, "--skin-texture": texture ? `url("${texture}")` : "none" } as CSSProperties;
  const hasSkin = !!texture && !imageFailed;
  const classes = [className, small ? "account-avatar-small" : "", hasSkin ? "has-minecraft-skin" : "avatar-fallback"].filter(Boolean).join(" ");
  return <span className={classes} aria-label={`${name} Minecraft skin head`} style={style}>
    {hasSkin ? <><img className="minecraft-head-source" src={texture} alt="" loading="lazy" referrerPolicy="no-referrer" onError={() => { failedTextures.add(normalizedUuid); setImageFailed(true); }} /><span className="minecraft-head-layer minecraft-head-face" /><span className="minecraft-head-layer minecraft-head-hat" /></> : initials(name)}
  </span>;
}

function initials(value: string): string {
  const parts = value.trim().split(/[\s._-]+/).filter(Boolean);
  return parts.slice(0, 2).map(part => part[0]?.toUpperCase() ?? "").join("") || "?";
}

function stableHue(value: string): number {
  return [...value].reduce((hash, character) => (hash * 31 + character.charCodeAt(0)) % 360, 190);
}
