# Local Epic Games Launcher UI and installation analysis

## Discovery

The installed launcher was located through three independent read-only sources:

- Running process executable: `C:\Users\silas\AppData\Local\Epic Games\Launcher\EpicGamesLauncher.exe`
- Uninstall registry entry, `Epic Games Launcher`: install location `C:\Users\silas\AppData\Local\Epic Games\Launcher`
- Start Menu shortcut: `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Epic Games Launcher.lnk`, targeting the same executable

```text
EPIC_LAUNCHER_EXE=C:\Users\silas\AppData\Local\Epic Games\Launcher\EpicGamesLauncher.exe
EPIC_LAUNCHER_ROOT=C:\Users\silas\AppData\Local\Epic Games\Launcher
```

The executable reports version `0.95.2+gfa9ad49` and is approximately 81 MB. The live Library window was observed at 1282×722 logical screenshot pixels, localized in German. The application was not modified. No account controls or settings were intentionally changed.

## What is present on disk

The install root contains the compiled `EpicGamesLauncher.exe`, updater, setup and migration executables, EOS SDK DLL, an `Overlay` tree and a `resources` directory containing third-party notices. The inspected tree contains 253 files: 58 `.pak`, 12 `.dll`, 6 `.exe`, 3 `.json`, 11 log files and small state/cache files. No plain launcher `.html`, `.js`, `.css`, `.tsx`, `.ts`, `.svg`, `.png`, `.webp`, `.ico` or font source files were found in the launcher install tree.

`Overlay\Win64` includes Chromium/CEF runtime files such as `libcef.dll`, `chrome_elf.dll`, `resources.pak`, locale packs and graphics libraries. This is the EOS overlay runtime; its presence is not evidence that the main launcher's UI source is present. `EpicGamesLauncherMigrate.exe.WebView2\EBWebView` is a WebView2 profile/cache directory associated with the migration executable, not readable launcher source.

### Source, bundle, binary and runtime distinctions

- **Source:** No readable frontend source or unminified launcher components were located.
- **Bundled frontend output:** No standalone JS/CSS/HTML bundles were found in the install tree. Binary `.pak` resources exist, but were not unpacked or inspected.
- **Compiled native code:** The launcher EXE, updater, setup, migration executable and DLLs are compiled binaries. They were not decompiled or reverse engineered.
- **Assets/configuration:** The inspected install exposes third-party notices, overlay runtime resources, localization packs and cache directories. No branded assets were copied.
- **Live shell evidence:** The visible window's accessibility tree exposed `TAURI_DRAG_RESIZE_WINDOW` and a document URL at `tauri.localhost/library`. This is consistent with a Tauri/webview shell, but does not reveal its implementation source.

## Live geometry observations

Measurements below are taken from the visible installed Library screenshot (1282×722). They are approximate where the screenshot's anti-aliasing and window borders make a one-pixel edge ambiguous. Color values are described visually; no pixel sampling was performed.

| Element | Observed measurement |
|---|---:|
| Outer content frame | x≈9–1273, y≈0–721; restrained ~10px outer corner |
| Toolbar | bottom edge y≈57; about 54–57px high |
| Workspace panels | x≈9–1273, y≈57–673 |
| Main panel | x≈9–882; about 873px |
| Sidebar | x≈891–1273; about 382px, 30% of the inner width |
| Panel gap | about 8px |
| Main content left inset | x≈58 from panel edge; about 49px |
| Library category row | text begins y≈96; compact 14px-class type and thin underline |
| Search field | x≈58–478; about 420×40px, pill radius about 20px |
| Installed filter control | about 41×41px, circular |
| View selector | about 74×41px |
| Filter/sort control | about 192×41px |
| Card covers | about 182×243px; portrait ratio ~0.75; 15–16px horizontal gaps |
| Cover grid start | y≈210px |
| Sidebar tab row | aligned near the main category text; content inset about 24px |
| Bottom strip | y≈673–721; about 48px including its margin/frame |

The visual surface hierarchy is charcoal outer chrome, near-black inner workspaces, subtle gray borders, and softly raised control/menu surfaces. The main workspace and social panel are close in base brightness. Text is compact but comfortably readable. The active top navigation control is a filled rounded rectangle, while library categories use text tabs and a thin underline.

## Card interactions

The installed Library accessibility tree exposes game cards and contextual actions. The supplied card hover and menu reference images were also used for measuring the interaction-specific surfaces:

- Card hover controls occupy the cover's upper corners; the primary action sits inset along the bottom edge.
- The supplied hover reference uses roughly 40px square controls and a 40px primary action, inset about 15–16px.
- The supplied context menu is roughly 238px wide, uses approximately 41px rows, restrained separators, a rounded charcoal surface and a subtle shadow.
- The live app's Library structure exposes search, installed-only filtering, a view-mode menu, filter/sort, card actions, a recent-games footer and Downloads.

The context-menu and hover screenshots are references, not recovered Epic implementation files. No proprietary code or artwork was copied.

## Icon system used in SNine

SNine already depends on `lucide-react` and uses one coherent Lucide outline family. The implementation keeps that family and uses semantic equivalents: `LibraryBig`, `Sparkles`, `ArrowLeft`, `ArrowRight`, `Search`, `Check`, `Grid2X2`, `List`, `Settings2`, `Heart`, `MoreHorizontal`, `Play`, `Download`, `History`, `ChevronDown`, `CircleHelp`, and the native-window control glyphs. SNine's supplied icon remains unchanged and separate from generic controls.
