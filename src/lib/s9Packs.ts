import bundledPackCatalog from "../../s9-client-packs/catalog.json";
import { ipc, type LoaderType } from "./ipc";

export type S9ModPack = {
  id: string;
  name: string;
  description: string;
  loader: LoaderType;
  minecraftVersions: string[];
  mods: Array<{ projectId: string; slug: string }>;
};

const preferenceKey = (profileId: string, packId: string) => `snine.launcher.s9-pack.${profileId}.${packId}.v1`;
const catalog = bundledPackCatalog as { packs: S9ModPack[] };

export function isDefaultSnineProfile(profileId: string): boolean {
  return /^snineclient-(?:default|\d)/.test(profileId);
}

export function getS9PackPreference(profileId: string, packId: string): boolean | null {
  try {
    const value = localStorage.getItem(preferenceKey(profileId, packId));
    return value === "enabled" ? true : value === "disabled" ? false : null;
  } catch {
    return null;
  }
}

export function setS9PackPreference(profileId: string, packId: string, enabled: boolean): void {
  try {
    localStorage.setItem(preferenceKey(profileId, packId), enabled ? "enabled" : "disabled");
  } catch {
    // The current UI state still works if browser storage is unavailable.
  }
}

export async function ensureDefaultS9Packs(
  profileId: string,
  minecraftVersion: string,
  loaderType: LoaderType,
): Promise<{ installed: number; failed: string[] }> {
  if (!isDefaultSnineProfile(profileId)) return { installed: 0, failed: [] };
  const packs = catalog.packs.filter(pack => pack.loader === loaderType && pack.minecraftVersions.includes(minecraftVersion));
  const mods = await ipc.getManagedMods(profileId);
  const installedProjects = new Set(mods.filter(mod => mod.managed).map(mod => mod.projectId));
  let installed = 0;
  const failed: string[] = [];

  for (const pack of packs) {
    if (getS9PackPreference(profileId, pack.id) === false) continue;
    for (const mod of pack.mods) {
      if (installedProjects.has(mod.projectId)) continue;
      try {
        await ipc.installModrinthMod(profileId, mod.projectId, mod.slug);
        installedProjects.add(mod.projectId);
        installed += 1;
      } catch {
        failed.push(mod.slug);
      }
    }
  }
  return { installed, failed };
}

