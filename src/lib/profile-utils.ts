import type { Profile } from "./ipc";

export function findSelectedProfile(profiles: Profile[], selectedId: string | null): Profile | undefined {
  return profiles.find(profile => profile.id === selectedId) ?? profiles.find(profile => profile.selected) ?? profiles[0];
}

export function insertCreatedProfile(current: Profile[], created: Profile): Profile[] {
  return [...current.map(profile => ({ ...profile, selected: false })), { ...created, selected: true }];
}
