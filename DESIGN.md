---
name: Meetingly — Atelier Obsidian
description: Spacious, low-glare desktop AI meeting workbench with an obsidian navigation rail and dual-pane live workspace.
colors:
  studio-sky: "#38bdf8"
  obsidian-ink: "#090c11"
  obsidian-rail: "#0f131a"
  graphite-header: "#131720"
  graphite-canvas: "#161b24"
  slate-side: "#121720"
  elevated-card: "#1e2532"
  nav-active: "#1e293b"
  crisp-ivory: "#eef2f7"
  slate-muted: "#94a3b8"
  live-rose: "#f43f5e"
  ready-emerald: "#34d399"
  notice-amber: "#fbbf24"
  hairline-border: "rgba(255, 255, 255, 0.1)"
typography:
  headline:
    fontFamily: "Inter Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "-0.02em"
  title:
    fontFamily: "Inter Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "0.96875rem"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "-0.01em"
  body:
    fontFamily: "Inter Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: "0em"
  label:
    fontFamily: "Inter Variable, Segoe UI, system-ui, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "0.04em"
rounded:
  sm: "6px"
  md: "8px"
  lg: "10px"
  xl: "12px"
  studio: "16px"
  pill: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "20px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.studio-sky}"
    textColor: "{colors.obsidian-ink}"
    rounded: "{rounded.md}"
    padding: "8px 14px"
    height: "32px"
  button-secondary:
    backgroundColor: "#222a38"
    textColor: "{colors.crisp-ivory}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
    height: "32px"
  button-destructive:
    backgroundColor: "rgba(244, 63, 94, 0.2)"
    textColor: "#ffe4e6"
    rounded: "{rounded.md}"
    padding: "6px 12px"
    height: "28px"
  answer-card:
    backgroundColor: "{colors.elevated-card}"
    textColor: "{colors.crisp-ivory}"
    rounded: "{rounded.xl}"
    padding: "20px"
  input-field:
    backgroundColor: "{colors.obsidian-rail}"
    textColor: "{colors.crisp-ivory}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
    height: "36px"
---

# Design System: Meetingly — Atelier Obsidian

## Overview

**Creative North Star: "The Obsidian Briefing Desk"**

Atelier Obsidian transforms Meetingly from a cramped floating popup into a calm, spacious desktop studio built for engineers, founders, and candidates on live video calls. A deep matte obsidian navigation rail grounds the workspace, separating navigation and local-keychain status from the graphite live-reading canvas so answers and incoming speech stay scannable at a glance without visual glare.

Every surface earns its space: generous padding (20px–24px), a restrained single primary sky accent, crisp 1px hairlines instead of heavy drop shadows, and a dual-column live workspace that keeps real-time AI answers beside the live speaker transcript.

**Key Characteristics:**
- Deep tonal layering across matte obsidian rail (`#0f131a`), graphite canvas (`#161b24`), and elevated answer cards (`#1e2532`).
- Spacious dual-window architecture: a breathable 48px always-on-top toolbar paired with a 920×640 studio workspace.
- Single primary accent (`#38bdf8`) reserved strictly for primary actions, active navigation, and live focus rings.
- High-legibility 14px body prose constrained to 68ch measure with bold 15.5px lead-in answer lines.

## Colors

The palette relies on cool obsidian and graphite neutrals for calm low-glare reading alongside a single sky-blue accent and strict semantic status cues.

### Primary
- **Studio Sky** (`#38bdf8`): Primary CTAs (`Start listening`, `Send`), active rail highlight, focus rings, and inline interactive links. Paired with **Obsidian Ink** (`#090c11`) on solid primary buttons for high contrast.

### Neutral
- **Obsidian Rail** (`#0f131a`): Left navigation rail, floating toolbar surface, and recessed input wells.
- **Graphite Header** (`#131720`): Top 56px session control ribbon and composer dock.
- **Graphite Canvas** (`#161b24`): Main workspace background.
- **Slate Side** (`#121720`): Right-hand live transcript and quick-actions column.
- **Elevated Card** (`#1e2532`): Answer cards, provider configuration forms, and meeting summary cards.
- **Crisp Ivory** (`#eef2f7`): Primary body text and headings.
- **Slate Muted** (`#94a3b8`): Secondary hints, timestamps, inactive audio meters, and uppercase section labels.
- **Hairline Border** (`rgba(255, 255, 255, 0.1)`): Structural 1px dividers between rail, header ribbon, and panes.

### Named Rules
**The One-Accent Rule.** Studio Sky (`#38bdf8`) marks primary actions, active navigation, and focus indicators only; live capture states use Rose (`#f43f5e`), ready/saved states use Emerald (`#34d399`), and setup warnings use Amber (`#fbbf24`).

## Typography

**Display Font:** Inter Variable (fallback: `Segoe UI, system-ui, sans-serif`)
**Body Font:** Inter Variable (fallback: `Segoe UI, system-ui, sans-serif`)

**Character:** Single-family geometric grotesque with optical sizing (`cv11`, `ss03`) and `tabular-nums` on every elapsed timer, line count, duration, and character counter so numbers never jitter during live calls.

### Hierarchy
- **Headline** (`600`, `1.125rem`, `1.3` line-height, `-0.02em` tracking): Workspace header ribbon titles (`Live Workspace`, `Live Transcript`, `Providers & Keys`).
- **Title** (`600`, `0.96875rem`, `1.4` line-height): Lead-in answer sentence (`15.5px` bold) and card section headings.
- **Body** (`400`, `0.875rem`, `1.6` line-height): Streamed answers, transcript turns, and meeting reports, bounded to `68ch` measure in answers.
- **Label** (`600`, `0.75rem`, `1.3` line-height, `0.04em` tracking): Rail section headers, speaker names, and form field labels.

### Named Rules
**The Tabular Numerals Rule.** Every live clock, meeting duration, segment counter, and character counter uses `tabular-nums` so horizontal alignment never shifts while counting.

## Layout

The desktop studio separates control from reading across two synchronized windows: a compact, breathable `48px` always-on-top toolbar (`main`, `260px–960px` wide) and an attached `920×640` workspace (`chat`, `640×440` min, `1440×1000` max). Inside the workspace, a fixed `220px` left rail houses the 7 primary destinations and local-keychain status card, collapsing cleanly into a horizontal top strip below `768px`. In `Live Workspace`, viewports `840px` and wider divide `1.18fr : 0.82fr` between the Answers stream and the Live Transcript / Quick Actions side pane. Standard workspace padding is `20px–24px` (`1.25rem–1.5rem`).

## Elevation & Depth

Surfaces rely on tonal contrast and 1px hairline borders rather than stacked drop shadows. Window shells (`studio-shell`, `studio-toolbar`) carry a single deep ambient shadow (`0 24px 60px -16px rgba(0, 0, 0, 0.85)`) with `backdrop-blur-2xl` over transparent OS windows; interior cards and answer blocks sit flat against the canvas with 1px borders (`border-sky-400/20` or `border-white/[0.08]`) and no drop shadows.

### Named Rules
**The Single-Elevation Rule.** Declare interior card elevation with either a 1px border or a shadow, never both simultaneously (no ghost cards).

## Shapes

The studio uses consistent, calm rounding: `16px` (`rounded-2xl`) for outer window shells and the floating toolbar, `12px` (`rounded-xl`) for answer cards, provider cards, and alert dialogs, `8px` (`rounded-lg`) for inputs, buttons, and rail navigation items, and `9999px` (`rounded-full`) for live status dots and toggle pills.

## Components

### Buttons
- **Shape:** `8px` radius (`rounded-lg`), `32px` default height (`36px` in spacious forms).
- **Primary:** Studio Sky (`#38bdf8`) fill with Obsidian Ink (`#090c11`) semibold text (`hover:bg-primary/80`).
- **Hover / Focus:** Subtle luminance shift (`150ms`) and `3px` Sky focus ring (`focus-visible:ring-ring/50`).
- **Secondary / Ghost / Destructive:** Muted Slate (`#222a38`) secondary buttons, transparent ghost buttons with subtle hover wash (`hover:bg-muted`), and high-contrast Rose-tinted destructive buttons (`rgba(244, 63, 94, 0.2)` fill, `#ffe4e6` text, `rgba(251, 113, 133, 0.25)` border) for `Stop listening` and delete confirmations.

### Cards / Containers
- **Corner Style:** `12px` radius (`rounded-xl`).
- **Background:** Elevated Card (`#1e2532`) for answers and forms; Slate Side (`#121720`) for transcript turns.
- **Border:** `1px` solid `rgba(56, 189, 248, 0.2)` on answer cards, `rgba(255, 255, 255, 0.08)` on neutral cards.
- **Internal Padding:** `20px` (`p-5`) on answer cards and provider forms; `12px–16px` on list rows.

### Inputs / Fields
- **Style:** `1px` border (`rgba(255, 255, 255, 0.14)`), Obsidian Rail (`#0f131a`) fill, `8px` radius (`36px` height in settings).
- **Focus:** Sky ring (`#38bdf8`) on focus-visible; Rose border (`aria-invalid`) with inline field error text below.

### Navigation
- **Style:** `220px` Obsidian Rail (`#0f131a`) with `36px`-tall full-width nav items, `aria-current="page"` on the active item (`#1e293b` fill, `#38bdf8` text/icon), and collapsible horizontal pills below `768px`.

## Do's and Don'ts

### Do:
- **Do** keep `68ch` max prose measure and `20px` internal padding on streamed answer cards so answers remain readable while speaking.
- **Do** validate provider URLs, required API keys, prompt text, and duplicate shortcuts upfront with inline field errors before IPC submission.
- **Do** preserve `data-no-drag` on interactive header controls and `getCurrentWindow().startDragging()` on frameless window drag regions.
- **Do** require `AlertDialog` confirmation before discarding non-empty live transcripts, clearing chat, or deleting providers/prompts/meetings.

### Don't:
- **Don't** crowd toggles or secondary pills into the 56px live session header ribbon—keep it focused on view title, live state, session controls, and close.
- **Don't** combine a 1px border with a drop shadow on interior cards.
- **Don't** add decorative looping animations that ignore `prefers-reduced-motion: reduce`.
- **Don't** omit keyboard `focus-visible` rings on custom buttons or `aria-label` on icon-only controls.
