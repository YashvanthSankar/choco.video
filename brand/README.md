# choco.video app icon

A frosted 9:16 glass card, the shape of every clip choco.video makes, floating over warm orange and caption-yellow light. One yellow caption bar is the only detail: it is the active word in the app's real output.

| File | Use |
|---|---|
| `app-icon.svg` | Source of the macOS app icon and the site favicon (824 px tile at 100, macOS squircle) |
| `app-icon-1024.png` | Rendered master; `pnpm tauri icon` builds every size from it |
| `app-icon-dark-alt.svg` | The dark variant, kept for a possible dark-mode icon |

Construction: the glass card is 340 × 604 (9:16) with 72 px corners; its frost is the background light blurred 150 px and lifted 34% toward white, with a specular top edge. Palette: orange `#FF7A2A`, caption yellow `#FFD84A`, warm white `#FFFAF2` → `#F3E7D8`.
