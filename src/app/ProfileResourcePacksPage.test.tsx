/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ipc, type ModrinthProject, type ResourcePackInfo } from "../lib/ipc";
import { ProfileResourcePacksPage } from "./ProfileResourcePacksPage";

vi.mock("../lib/ipc", () => ({ ipc: {
  getProfileResourcePacks: vi.fn(), openProfileResourcePacks: vi.fn(), removeProfileResourcePack: vi.fn(),
  importProfileResourcePack: vi.fn(), selectResourcePackFile: vi.fn(), searchModrinthResourcePacks: vi.fn(), installModrinthResourcePack: vi.fn(),
} }));
afterEach(() => cleanup());

const pack: ResourcePackInfo = { filename: "forest.zip", name: "Forest Pack", description: "A calm green palette", packFormat: 34, bytes: 1024 };
const project: ModrinthProject = { projectId: "forest-pack", slug: "forest-pack", title: "Forest Pack", description: "A forest texture set", author: "Creator", iconUrl: "", downloads: 2400, categories: [], gameVersions: ["1.21.11"] };

describe("profile resource pack management", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ipc.getProfileResourcePacks).mockResolvedValue([pack]);
    vi.mocked(ipc.openProfileResourcePacks).mockResolvedValue();
    vi.mocked(ipc.removeProfileResourcePack).mockResolvedValue();
    vi.mocked(ipc.importProfileResourcePack).mockResolvedValue(pack);
    vi.mocked(ipc.selectResourcePackFile).mockResolvedValue("C:\\Users\\silas\\Downloads\\forest.zip");
    vi.mocked(ipc.searchModrinthResourcePacks).mockResolvedValue([project]);
    vi.mocked(ipc.installModrinthResourcePack).mockResolvedValue(pack);
  });

  it("shows packs from native profile metadata and confirms removal", async () => {
    render(<ProfileResourcePacksPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    expect(await screen.findByText("Forest Pack")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Remove Forest Pack" }));
    expect(ipc.removeProfileResourcePack).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Remove pack" }));
    expect(ipc.removeProfileResourcePack).toHaveBeenCalledWith("profile-a", "forest.zip");
  });

  it("searches and installs a Minecraft-version-compatible Modrinth resource pack", async () => {
    render(<ProfileResourcePacksPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(screen.getByRole("tab", { name: "Browse Modrinth" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Search Modrinth resource packs" }), "woods");
    expect(await screen.findByText("Forest Pack")).toBeTruthy();
    expect(ipc.searchModrinthResourcePacks).toHaveBeenCalledWith("woods", "1.21.11");
    await userEvent.click(screen.getByRole("button", { name: "Install" }));
    expect(ipc.installModrinthResourcePack).toHaveBeenCalledWith("profile-a", "forest-pack");
  });

  it("retains the local management surface when Modrinth is unavailable", async () => {
    vi.mocked(ipc.searchModrinthResourcePacks).mockRejectedValue(new Error("Network unavailable"));
    render(<ProfileResourcePacksPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(screen.getByRole("tab", { name: "Browse Modrinth" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Search Modrinth resource packs" }), "forest");
    expect(await screen.findByRole("alert")).toBeTruthy();
    await userEvent.click(screen.getByRole("tab", { name: /Installed/ }));
    expect(await screen.findByText("Forest Pack")).toBeTruthy();
  });

  it("sends the native file-dialog selection to the profile-scoped validator", async () => {
    render(<ProfileResourcePacksPage profileId="profile-a" minecraftVersion="1.21.11" installed />);
    await userEvent.click(screen.getByRole("button", { name: "Add from computer" }));
    expect(await screen.findByRole("status")).toBeTruthy();
    expect(ipc.selectResourcePackFile).toHaveBeenCalledOnce();
    expect(ipc.importProfileResourcePack).toHaveBeenCalledWith("profile-a", "C:\\Users\\silas\\Downloads\\forest.zip");
  });
});
