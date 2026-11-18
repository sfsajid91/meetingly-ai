---
version: 1
slug: "app-src-windows-panelwindow-tsx"
primary_target: "app/src/windows/PanelWindow.tsx"
related_targets: ["app/src/windows/MainWindow.tsx","app/src/components/SettingsPanels.tsx"]
---

# Meetingly Workspace Surface Brief

- **Mode:** Operate
- **Scope:** Complete redesign of the floating toolbar (`MainWindow`) and spacious floating studio workspace (`PanelWindow` + `SettingsPanels`).

## Direction contract

- **THESIS:** Replace the cramped 528×380 popup overlay with a spacious 920×640 **Atelier Obsidian** live-meeting workbench featuring a dedicated obsidian-slate navigation rail and side-by-side Answers + Live Transcript panes during calls.
- **OWN-WORLD:** Deep matte obsidian rail (`#0F131A`), graphite workspace canvas (`#161B24`), elevated answer cards (`#1E2532`), crisp 1px structural rules (`rgba(255,255,255,0.09)`), sky-blue primary accent (`#38BDF8`), rose live-capture signal (`#F43F5E`), emerald saved/ready cue (`#34D399`), amber setup notice (`#FBBF24`), and `Inter Variable` with tabular numerals and generous `20px–24px` workspace padding.
- **STORY:** Remote engineers, founders, and candidates keep a breathable, low-glare studio beside their video call or editor where live answers, incoming transcript turns, audio levels, and validated BYOK settings are immediately scannable without squinting or hunting through cramped menus.
- **FIRST VIEWPORT:** Breathable `48px` always-on-top floating toolbar (`main`) anchored above a `920×640` frameless workspace (`chat`): left `220px` obsidian navigation rail (`Live Workspace`, `Transcript`, `Meetings`, `Prompts`, `Providers`, `Shortcuts`, `General` plus bottom provider/stealth status card), top `56px` live-session ribbon (session title/state, live elapsed timer, Pause/Resume, Stop & Save, Discard, audio meters, Close), and a spacious dual-column live workspace (Answers stream + composer on left, Live Transcript rail on right during active sessions or wide viewports).
- **FORM:** Atelier Obsidian (IMPECCABLE'S PICK), Candidate 1 of grounded list, `seed:44b7a5ce289b20a4`. Chosen sketch: `.impeccable/mocks/decision/model-pick.html`.
- **FINISH:** unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
