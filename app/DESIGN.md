# choco.video UI system

A warm, compact macOS interface built with Svelte 5 and native system type. The top toolbar is a Tauri drag region; controls opt out of dragging.

## Tokens

- `:root` defines the brand: chocolate `#3B2420`, cocoa ink `#1E1411`, cream `#F7EFE6`, caramel `#D08A57`, and success `#2F7D4A`.
- Semantic surface, border, text, accent, focus, hover, pressed, and media overlay tokens have light defaults and `prefers-color-scheme: dark` overrides.
- UI uses the system font at 13px; headings use a compact 14/16/24px scale. Time uses tabular numerals; code and timecodes use SF Mono.
- Controls have 8px corners, cards 12px corners, hairline borders, and restrained raised-surface shadows.

## Components and views

- Toolbar: brand mark, contextual New video action, and Settings.
- Home: source drop zone becomes a selected file card; topic, count, segmented caption/size/framing choices, output folder, and primary Find clips action.
- Onboarding: centered card, four-step indicator, privacy message, provider setup, speech readiness, and finish state.
- Job: overall progress and elapsed time, ordered stage states, live detail, and Cancel job.
- Results: responsive 9:16 poster cards, score/rank/duration, inline playback, Finder/caption/restyle actions, and empty state.
- Settings: provider, defaults, speech, and app information with a persistent save action.

## Interaction states

- All controls show hover, pressed, disabled, and caramel keyboard focus states. Segmented controls expose `aria-pressed`.
- Drag-over highlights the source target. Video posters play on hover or click. Progress uses native `progressbar` semantics and a live status detail.
- Layout collapses for narrow windows. `prefers-reduced-motion` removes nonessential transitions and animation.
