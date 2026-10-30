/** @vitest-environment jsdom */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BlockIconPicker, insertCreatedProfile, ProfileCreationForm } from "./ProfileManagementForms";
import type { BlockIconEntry, LoaderType, MinecraftVersionInfo, Profile } from "../lib/ipc";

afterEach(() => cleanup());

vi.mock("../lib/ipc", () => ({
  ipc: { getBlockIconCatalog: vi.fn() },
}));
import { ipc } from "../lib/ipc";

const versions: MinecraftVersionInfo[] = [
  { id: "1.21.11", versionType: "release", releaseTime: "2026-01-01T00:00:00Z", url: "https://example.test/1.21.11.json" },
  { id: "26.1.2", versionType: "release", releaseTime: "2026-09-01T00:00:00Z", url: "https://example.test/26.1.2.json" },
  { id: "25w01a", versionType: "snapshot", releaseTime: "2025-01-01T00:00:00Z", url: "https://example.test/25w01a.json" },
];

function formProps(overrides: Partial<React.ComponentProps<typeof ProfileCreationForm>> = {}) {
  return {
    name: "Vanilla Test", onNameChange: vi.fn(), minecraftVersion: "1.21.11", onMinecraftVersionChange: vi.fn(),
    versions, showSnapshots: false, onShowSnapshotsChange: vi.fn(), loaderType: "vanilla" as const, availableLoaders: ["vanilla", "fabric", "quilt"] as LoaderType[], onLoaderTypeChange: vi.fn(),
    loaderVersion: "", loaderVersions: [], onLoaderVersionChange: vi.fn(), includeSnineClient: false, onIncludeSnineClientChange: vi.fn(),
    memory: "4096", onMemoryChange: vi.fn(), iconId: null, iconPreview: null, onChooseIcon: vi.fn(), onCreate: vi.fn(), onCancel: vi.fn(),
    ...overrides,
  };
}

describe("profile creation form", () => {
  it("shows the real version choices and creates a valid Vanilla profile", async () => {
    const props = formProps();
    render(<ProfileCreationForm {...props} />);
    expect(screen.getByLabelText("Minecraft version")).toBeTruthy();
    expect(document.querySelector('#minecraft-version-options option[value="1.21.11"]')).not.toBeNull();
    expect(screen.getByRole("combobox", { name: "Mod loader" })).toHaveProperty("value", "vanilla");
    await userEvent.click(screen.getByRole("button", { name: "Create profile" }));
    expect(props.onCreate).toHaveBeenCalledOnce();
  });

  it("hides snapshots by default and rejects a version outside the current manifest list", () => {
    const props = formProps({ minecraftVersion: "26.4" });
    const { rerender } = render(<ProfileCreationForm {...props} />);
    expect(document.querySelector('#minecraft-version-options option[value="25w01a"]')).toBeNull();
    expect(screen.getByRole("button", { name: "Create profile" })).toHaveProperty("disabled", true);
    rerender(<ProfileCreationForm {...props} showSnapshots onShowSnapshotsChange={vi.fn()} />);
    expect(document.querySelector('#minecraft-version-options option[value="25w01a"]')).toBeTruthy();
  });

  it("changes loader and SNineClient toggle, and exposes Fabric loader versions", async () => {
    const props = formProps({ loaderType: "fabric", loaderVersion: "0.19.5", loaderVersions: ["0.19.5", "0.19.4"], includeSnineClient: true });
    render(<ProfileCreationForm {...props} />);
    expect(screen.getByRole("combobox", { name: "Fabric Loader version" })).toHaveProperty("value", "0.19.5");
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Fabric Loader version" }), "0.19.4");
    expect(props.onLoaderVersionChange).toHaveBeenCalledWith("0.19.4");
    await userEvent.click(screen.getAllByRole("checkbox", { name: "Include SNineClient" }).at(-1)!);
    expect(props.onIncludeSnineClientChange).toHaveBeenCalledWith(false);
  });

  it("exposes Quilt with a remotely resolved compatible loader version", async () => {
    const props = formProps({ loaderType: "quilt", loaderVersion: "0.31.0-beta.4", loaderVersions: ["0.31.0-beta.4", "0.30.0"] });
    render(<ProfileCreationForm {...props} />);
    expect(screen.getByRole("combobox", { name: "Mod loader" }).textContent).toContain("Quilt");
    const versionPicker = screen.getByRole("combobox", { name: "Quilt Loader version" });
    expect(versionPicker).toHaveProperty("value", "0.31.0-beta.4");
    await userEvent.selectOptions(versionPicker, "0.30.0");
    expect(props.onLoaderVersionChange).toHaveBeenCalledWith("0.30.0");
    expect(screen.getByRole("button", { name: "Create profile" })).toHaveProperty("disabled", false);
  });

  it("only presents providers returned as available for the selected Minecraft version", () => {
    render(<ProfileCreationForm {...formProps({ availableLoaders: ["vanilla"] })} />);
    const choices = Array.from(screen.getByRole("combobox", { name: "Mod loader" }).querySelectorAll("option")).map(option => option.value);
    expect(choices).toEqual(["vanilla"]);
  });

  it("rejects SNineClient on Vanilla and offers a supported configuration", async () => {
    const props = formProps({ includeSnineClient: true });
    render(<ProfileCreationForm {...props} />);
    expect(screen.getByRole("alert").textContent).toContain("SNineClient requires Fabric");
    expect(screen.getByRole("button", { name: "Create profile" })).toHaveProperty("disabled", true);
    await userEvent.click(screen.getByRole("button", { name: "Switch to supported configuration" }));
    expect(props.onLoaderTypeChange).toHaveBeenCalledWith("fabric");
    expect(props.onMinecraftVersionChange).toHaveBeenCalledWith("1.21.11");
  });

  it("allows SNineClient on the verified Fabric 26.1.2 build", () => {
    render(<ProfileCreationForm {...formProps({ minecraftVersion: "26.1.2", loaderType: "fabric", loaderVersion: "0.19.5", includeSnineClient: true })} />);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Create profile" })).toHaveProperty("disabled", false);
  });

  it("validates optional memory and opens the version-aware icon picker", async () => {
    const props = formProps({ memory: "2049" });
    render(<ProfileCreationForm {...props} />);
    expect(screen.getByRole("button", { name: "Create profile" })).toHaveProperty("disabled", true);
    await userEvent.click(screen.getByRole("button", { name: "Choose block…" }));
    expect(props.onChooseIcon).toHaveBeenCalledOnce();
  });

  it("inserts the created profile into the Library selection immediately", () => {
    const existing = { id: "existing", selected: true } as Profile;
    const created = { id: "new-profile-id", name: "Fabric Test", selected: false } as Profile;
    const next = insertCreatedProfile([existing], created);
    expect(next).toHaveLength(2);
    expect(next[0].selected).toBe(false);
    expect(next[1]).toMatchObject({ id: "new-profile-id", name: "Fabric Test", selected: true });
  });
});

describe("latest-release block and item icon picker", () => {
  const entries: BlockIconEntry[] = [
    { id: "minecraft:stone", label: "Stone", kind: "block_and_item", sourceVersion: "latest", previewDataUrl: "data:image/png;base64,AA==" },
    { id: "minecraft:diamond_block", label: "Diamond Block", kind: "block", sourceVersion: "latest", previewDataUrl: null },
  ];
  beforeEach(() => {
    vi.mocked(ipc.getBlockIconCatalog).mockReset();
    vi.mocked(ipc.getBlockIconCatalog).mockResolvedValue(entries);
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  });
  it("searches the latest release catalog and returns the chosen icon identity", async () => {
    const onSelect = vi.fn();
    render(<BlockIconPicker open minecraftVersion="1.21.11" selectedIconId={null} selectedPreview={null} onSelect={onSelect} onClose={vi.fn()} />);
    expect(await screen.findByRole("button", { name: /Stone/ })).toBeTruthy();
    expect(ipc.getBlockIconCatalog).toHaveBeenCalledWith("latest", "", 0, 64);
    await userEvent.click(screen.getByTitle(/minecraft:diamond_block/));
    expect(onSelect).toHaveBeenCalledWith(entries[1]);
  });

  it("updates catalog query and supports paged loading", async () => {
    const next: BlockIconEntry[] = [{ id: "minecraft:oak_planks", label: "Oak Planks", kind: "block_and_item", sourceVersion: "latest", previewDataUrl: null }];
    vi.mocked(ipc.getBlockIconCatalog).mockImplementation(async (_version, query, offset) => offset ? next : query ? entries.filter(item => item.id.includes("diamond")) : entries);
    render(<BlockIconPicker open minecraftVersion="1.20.1" selectedIconId={null} selectedPreview={null} onSelect={vi.fn()} onClose={vi.fn()} />);
    await screen.findByRole("button", { name: /Stone/ });
    await userEvent.type(screen.getByRole("textbox", { name: "Search Minecraft blocks and items" }), "diamond_block");
    expect(await screen.findByTitle(/minecraft:diamond_block/)).toBeTruthy();
    await waitFor(() => expect(ipc.getBlockIconCatalog).toHaveBeenLastCalledWith("latest", "diamond_block", 0, 64));
    await userEvent.clear(screen.getByRole("textbox", { name: "Search Minecraft blocks and items" }));
    await screen.findByRole("button", { name: /Stone/ });
    await waitFor(() => expect(screen.getByRole("button", { name: "Load more icons" })).toHaveProperty("disabled", false));
    await userEvent.click(screen.getByRole("button", { name: "Load more icons" }));
    expect(await screen.findByTitle(/minecraft:oak_planks/)).toBeTruthy();
  });
});






