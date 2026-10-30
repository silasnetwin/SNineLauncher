# Epic Launcher / SNine visual comparison

Reference basis: the locally installed Epic Games Launcher Library screenshot (1282×722), plus the supplied card-hover and context-menu reference screenshots. Values are approximate screenshot measurements. SNine baseline refers to the presentation before this comparison pass.

| Area | Epic reference | SNine before | Difference | Change made |
|---|---|---|---|---|
| Outer frame | Restrained charcoal frame, ~10px corners | Similar shell and corner treatment | Close | Kept current frame and surfaces |
| Toolbar | ~54–57px, brand/menu, history, filled selected Library control, window buttons | 54px and filled selected control | Library glyph was a generic grid | Replaced the Library nav glyph with Lucide `LibraryBig`; retained canonical SNine icon and native window controls |
| Navigation | Compact ~38px filled active button with icon/label | Similar size and hierarchy | Close | Kept geometry and coherent Lucide glyph family |
| Library tabs | ~14px text tabs, thin underline, first row starts ~38px below workspace top | 14px text tabs with ~29px gaps | Close | Kept measured spacing and underline |
| Search | ~420×40px rounded field | 41px field, could grow to 480px | Too wide at desktop widths | Capped at 420px and retained 41px control height |
| View/filter controls | Installed-only circle (~41px), view (~74×41px), filter/sort (~192×41px), divider | View dropdown (70px) and sort (176px); no installed-only control | Missing control and group was undersized | Added working installed-only toggle, divider, 74px view selector and 192px sort control |
| Cards | ~182×243px cover, ~0.75 ratio, ~15–16px gaps | 180px card, ~243px cover | Very close | Kept portrait cover scale and spacing; no extra row filling |
| Hover state | Corner actions (~40px); bottom action (~40px), ~15–16px inset | Similar behavior, controls inset closer to edges and artwork dimmed more | Slightly tight/dim | Set 40px corner/action dimensions, ~14–16px insets, reduced whole-image dimming |
| Context menu | ~238px wide, ~41px rows, separators and restrained shadow | 246px, 39px rows | Slightly oversized/short rows | Changed to 238px and 41px rows |
| Sidebar | ~382px / 30% of inner window; tabs align vertically with main tabs | 27vw, capped at 350px; tab text too high and inset too tight | Narrow and vertically misaligned | Changed to 30vw (clamped 300–390px), aligned tab/content insets and matched panel base surface to main workspace |
| Bottom bar | ~48px total frame footprint, centered Downloads, low-contrast utility/status items | 42px strip plus margins (~48px total) | Geometry close; recent icon less specific | Kept dimensions and changed to a history glyph; raised version label to readable 12px |

## Current intentional SNine differences

- Lime is the SNine accent; Epic uses a blue primary action.
- Activity and Downloads replace Epic's social sidebar.
- Profile artwork, names and available actions are SNine-specific.
- Unsupported Play/Install/uninstall/collection operations stay disabled or unavailable instead of pretending to work.
