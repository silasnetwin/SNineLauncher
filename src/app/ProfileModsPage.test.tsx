/** @vitest-environment jsdom */
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ipc, type ManagedMod, type ModrinthProject } from "../lib/ipc";
import { ProfileModsPage } from "./ProfileModsPage";

vi.mock("../lib/ipc", () => ({ ipc: {
  getManagedMods: vi.fn(), searchModrinthFabricMods: vi.fn(), installModrinthMod: vi.fn(),
  checkModUpdates: vi.fn(), setModEnabled: vi.fn(), removeManagedMod: vi.fn(),
} }));

afterEach(() => cleanup());
const mod: ManagedMod = { profileId: "profile-a", modId: "sodium", projectId: "project-sodium", slug: "sodium", versionId: "opaque-version-id", filename: "sodium-fabric-0.8.14.jar", hash: "private-hash", enabled: true, managed: true };
const project: ModrinthProject = { projectId: "project-lithium", slug: "lithium", title: "Lithium", description: "Server and game optimization", author: "CaffeineMC", iconUrl: "", downloads: 12345, categories: ["fabric"], gameVersions: ["1.21.11"] };

describe("profile mods experience", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ipc.getManagedMods).mockResolvedValue([mod]);
    vi.mocked(ipc.installModrinthMod).mockResolvedValue([mod]);
    vi.mocked(ipc.searchModrinthFabricMods).mockResolvedValue([project]);
    vi.mocked(ipc.checkModUpdates).mockResolvedValue([]);
    vi.mocked(ipc.setModEnabled).mockResolvedValue();
    vi.mocked(ipc.removeManagedMod).mockResolvedValue();
  });

  it("shows installed mods from local metadata without exposing internal version or hash", async () => {
    render(<ProfileModsPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    expect(await screen.findByText(/sodium fabric/i)).toBeTruthy();
    expect(screen.queryByText("opaque-version-id")).toBeNull();
    expect(screen.queryByText("private-hash")).toBeNull();
    expect(screen.getByText("Enabled")).toBeTruthy();
  });

  it("disables a mod immediately after native persistence succeeds", async () => {
    render(<ProfileModsPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(await screen.findByRole("button", { name: "Disable" }));
    expect(ipc.setModEnabled).toHaveBeenCalledWith("profile-a", mod.filename, false);
    expect(await screen.findByText("Disabled")).toBeTruthy();
  });

  it("requires confirmation before removing an installed mod", async () => {
    render(<ProfileModsPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(await screen.findByRole("button", { name: /remove sodium fabric/i }));
    expect(screen.getByRole("alertdialog").textContent).toContain("Remove Sodium Fabric?");
    expect(ipc.removeManagedMod).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Remove mod" }));
    expect(ipc.removeManagedMod).toHaveBeenCalledWith("profile-a", mod.filename);
  });

  it("searches compatible Modrinth results in Browse and installs by one clear action", async () => {
    render(<ProfileModsPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(screen.getByRole("tab", { name: "Browse Modrinth" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Search Modrinth mods" }), "sodium");
    expect(await screen.findByText("Lithium")).toBeTruthy();
    expect(await screen.findByText("Fabric · 1.21.11")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Install" }));
    expect(ipc.installModrinthMod).toHaveBeenCalledWith("profile-a", "project-lithium", "lithium");
  });

  it("keeps a useful empty state for a profile with no mods", async () => {
    vi.mocked(ipc.getManagedMods).mockResolvedValue([]);
    render(<ProfileModsPage profileId="profile-empty" minecraftVersion="1.21.11" installed />);
    expect(await screen.findByText("No mods installed")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Browse mods" }));
    expect(screen.getByRole("tab", { name: "Browse Modrinth" }).getAttribute("aria-selected")).toBe("true");
  });

  it("asks before updating all mods and applies the native update plan", async () => {
    vi.mocked(ipc.checkModUpdates).mockResolvedValue([{ projectId: "project-sodium", slug: "sodium", currentVersionId: "old", latestVersionId: "new", updateAvailable: true }]);
    render(<ProfileModsPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    await userEvent.click(await screen.findByRole("button", { name: "Update all (1)" }));
    expect(screen.getByRole("alertdialog").textContent).toContain("Update 1 mod?");
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Update" }));
    expect(ipc.installModrinthMod).toHaveBeenCalledWith("profile-a", "project-sodium", "sodium");
  });

  it("ignores an older search response when a newer query is active", async () => {
    let resolveOld!: (value: ModrinthProject[]) => void;
    let resolveNew!: (value: ModrinthProject[]) => void;
    const oldResponse = new Promise<ModrinthProject[]>(resolve => { resolveOld = resolve; });
    const newResponse = new Promise<ModrinthProject[]>(resolve => { resolveNew = resolve; });
    vi.mocked(ipc.searchModrinthFabricMods).mockImplementation(query => query === "stale" ? oldResponse : newResponse);
    render(<ProfileModsPage profileId="search-race-profile" minecraftVersion="1.21.11" installed />);
    await userEvent.click(screen.getByRole("tab", { name: "Browse Modrinth" }));
    const search = screen.getByRole("textbox", { name: "Search Modrinth mods" });
    await userEvent.type(search, "stale");
    await waitFor(() => expect(ipc.searchModrinthFabricMods).toHaveBeenCalledTimes(1));
    await userEvent.clear(search);
    await userEvent.type(search, "current");
    await waitFor(() => expect(ipc.searchModrinthFabricMods).toHaveBeenCalledTimes(2));
    resolveOld([{ ...project, projectId: "stale-id", title: "Stale Result" }]);
    resolveNew([{ ...project, projectId: "current-id", title: "Current Result" }]);
    expect(await screen.findByText("Current Result")).toBeTruthy();
    expect(screen.queryByText("Stale Result")).toBeNull();
  });
});
