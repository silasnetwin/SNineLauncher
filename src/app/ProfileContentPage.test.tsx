/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../lib/ipc";
import { ProfileContentPage } from "./ProfileContentPage";

vi.mock("../lib/ipc", () => ({ ipc: {
  getProfileContent: vi.fn(), openProfileContentFolder: vi.fn(), openProfileContentItem: vi.fn(),
  importProfileContent: vi.fn(), removeProfileContent: vi.fn(), selectProfileContentFile: vi.fn(),
  getProfileSymlinks: vi.fn(), createProfileSymlink: vi.fn(), removeProfileSymlink: vi.fn(), selectSymlinkSource: vi.fn(),
} }));
afterEach(() => cleanup());

describe("profile game content management", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ipc.getProfileContent).mockImplementation(async (_id, section) => section === "worlds" ? [{ name: "Survival", directory: true, bytes: 0 }] : []);
    vi.mocked(ipc.openProfileContentFolder).mockResolvedValue();
    vi.mocked(ipc.openProfileContentItem).mockResolvedValue();
    vi.mocked(ipc.importProfileContent).mockResolvedValue({ name: "clean-shader.zip", directory: false, bytes: 2048 });
    vi.mocked(ipc.removeProfileContent).mockResolvedValue();
    vi.mocked(ipc.selectProfileContentFile).mockResolvedValue("C:\\QA\\clean-shader.zip");
    vi.mocked(ipc.getProfileSymlinks).mockResolvedValue([]);
    vi.mocked(ipc.createProfileSymlink).mockResolvedValue({ name: "Shared", directory: true, targetExists: true });
    vi.mocked(ipc.removeProfileSymlink).mockResolvedValue();
    vi.mocked(ipc.selectSymlinkSource).mockResolvedValue("C:\\Shared");
  });

  it("shows profile-scoped shader files and opens their managed folder", async () => {
    vi.mocked(ipc.getProfileContent).mockImplementation(async (_id, section) => section === "shaderpacks" ? [{ name: "Complementary.zip", directory: false, bytes: 4096 }] : []);
    render(<ProfileContentPage profileId="profile-a" installed />);
    expect(await screen.findByText("Complementary.zip")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Open folder" }));
    expect(ipc.openProfileContentFolder).toHaveBeenCalledWith("profile-a", "shaderpacks", undefined);
  });

  it("requires a world and installs data packs into the selected world context", async () => {
    render(<ProfileContentPage profileId="profile-a" installed />);
    await userEvent.click(screen.getByRole("tab", { name: "Data packs" }));
    expect(await screen.findByRole("combobox", { name: "World" })).toBeTruthy();
    const select = screen.getByRole("combobox", { name: "World" });
    await userEvent.selectOptions(select, "Survival");
    await userEvent.click(screen.getByRole("button", { name: "Add data pack" }));
    expect(ipc.importProfileContent).toHaveBeenCalledWith("profile-a", "datapacks", "C:\\QA\\clean-shader.zip", "Survival");
  });

  it("keeps actions unavailable before the profile is installed", () => {
    render(<ProfileContentPage profileId="profile-a" installed={false} />);
    expect(screen.getByText("Install this profile to manage its game files.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Add shader pack/ })).toBeNull();
  });

  it("creates a profile-scoped link from the user-selected source and link name", async () => {
    render(<ProfileContentPage profileId="profile-a" installed />);
    await userEvent.click(screen.getByRole("tab", { name: "Symlinks" }));
    await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    expect(await screen.findByDisplayValue("Shared")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Create link" }));
    expect(ipc.createProfileSymlink).toHaveBeenCalledWith("profile-a", "C:\\Shared", "Shared");
  });
});
