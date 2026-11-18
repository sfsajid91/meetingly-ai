# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Remote engineers, founders, and candidates using Meetingly during live calls, technical interviews, and meetings.

## Product Purpose

Meetingly is a local-first, bring-your-own-key (BYOK) desktop AI meeting assistant. It captures microphone and system audio for live transcription, answers meeting questions in real time (manually or automatically), analyzes screen content, and saves structured meeting transcripts and reports locally.

## Positioning

Unlike cloud-hosted subscription bots that join calls as visible participants or lock users into hosted accounts, Meetingly runs locally on the user's machine with their own Deepgram and OpenAI-compatible API keys stored in the OS keychain, optional stealth content protection, and zero external relay dependency.

## Operating Context

Users operate Meetingly alongside active video calls, coding editors, browsers, and slide decks. The interface stays accessible via a breathable always-on-top floating toolbar (`main` window) and a spacious floating workspace (`chat` panel, ~860×620 default) that supports stealth mode, quick glanceability while speaking, and distraction-free configuration and review.

## Capabilities and Constraints

- **Two-window architecture**: Always-on-top frameless floating toolbar (`main`) paired with a spacious attached floating workspace (`chat`), both draggable and stealth-capable (`set_content_protected`).
- **Live transcription**: Deepgram WebSocket streaming over microphone, system audio monitor, or both (with optional RNNoise mic denoising), live interim/final speaker turns, pause/resume, stop-and-save, and discard controls.
- **Real-time AI assistance**: Streaming answers, optional hands-free auto-answering on detected questions, suggested contextual follow-ups, quick actions, and primary-display screenshot analysis via OpenAI-compatible providers.
- **Local storage & security**: API keys stored strictly in the OS keychain (`keyring`); settings, prompts, global shortcuts, and saved meeting histories/reports stored locally on disk.
- **Frontend stack**: React 19, TypeScript, Tailwind CSS v4, `shadcn` (`base-nova` / `@base-ui/react`), `cn`, and Tauri v2 IPC.

## Brand Commitments

- Name: **Meetingly**
- Tone: Calm, direct, trustworthy, local-first utility without cloud hype or clutter.

## Evidence on Hand

- Existing Tauri v2 + React codebase in `app/`, Rust backend in `app/src-tauri/src/`, and bundled `Inter Variable` font.
- No customer testimonials, pricing tiers, or hosted benchmarks apply to the local BYOK desktop app.

## Product Principles

- **Glanceable under live pressure**: Answers and live transcripts must be legible and scannable while the user is actively speaking on a call.
- **Local-first transparency**: Active providers, audio sources, capture state, and privacy protections (stealth, keychain storage) are always clear and controllable.
- **Breathable workspace over cramped chrome**: Generous spacing, readable type, and clean hierarchy replace tiny cramped controls while preserving fast keyboard and toolbar access.
- **Safe, validated configuration**: Forms validate URLs, keys, prompts, and shortcuts upfront and confirm destructive actions before discarding data.

## Accessibility & Inclusion

- Keyboard-navigable controls across tabs, forms, dialogs, and shortcuts, visible focus rings, WCAG AA contrast on dark/translucent surfaces, and reduced-motion support.
