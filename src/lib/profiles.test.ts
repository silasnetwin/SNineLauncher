import { describe, expect, it } from "vitest";
import { findSelectedProfile } from "./profile-utils";
import type { Profile } from "./ipc";

const profile = (id: string, selected = false): Profile => ({ id, name: id, selected, clientVersion: "Latest", minecraftVersion: "Latest", installed: false, clientReady: false, favorite: false, hidden: false, loaderType: "fabric", loaderVersion: null, snineclientEnabled: true, iconId: null, memoryMb: null });

describe("profile selection", () => {
  it("uses the explicitly selected profile", () => {
    expect(findSelectedProfile([profile("a"), profile("b")], "b")?.id).toBe("b");
  });

  it("falls back to the first cached profile when selection is absent", () => {
    expect(findSelectedProfile([profile("a")], null)?.id).toBe("a");
    expect(findSelectedProfile([], null)).toBeUndefined();
  });
});
