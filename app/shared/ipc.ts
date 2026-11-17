// Typed Tauri IPC command and event contracts shared with the frontend.

import type {
  AudioDevices,
  AudioLevels,
  ChatChunk,
  ChatMessage,
  Meeting,
  MigrationOutcome,
  PanelTab,
  ProviderInput,
  ProviderKind,
  ProviderRole,
  ProviderTestResult,
  ScreenshotResult,
  SessionState,
  SettingsPatch,
  SettingsView,
  SuggestionsState,
  TranscriptEvent,
  Utterance,
  WindowKind,
  WindowsState
} from "./types"

export interface InvokeMap {
  "settings:get": () => SettingsView
  "settings:update": (patch: SettingsPatch) => SettingsView

  "provider:save": (input: ProviderInput) => SettingsView
  "provider:remove": (id: string) => SettingsView
  "provider:select": (role: ProviderRole, id: string | null) => SettingsView
  "provider:models": (input: { id?: string; endpoint: string; key?: string }) => string[]
  "provider:test": (input: { id?: string; kind: ProviderKind; endpoint: string; model: string; key?: string; language?: string }) => ProviderTestResult

  "migration:status": () => MigrationOutcome
  "audio:devices": () => AudioDevices

  "meetings:list": () => Meeting[]
  "meetings:get": (id: string) => Meeting | null
  "meetings:delete": (id: string) => boolean
  "meetings:export": (id: string, reveal?: boolean) => string
  "reports:generate": (id: string) => Meeting | null

  "windows:state": () => WindowsState
  "windows:show": (kind: WindowKind) => void
  "windows:hide": (kind: WindowKind) => void
  "windows:toggle": (kind: WindowKind) => boolean
  "windows:toggle-all": () => void
  "panel:show": (tab: PanelTab) => void
  "panel:toggle": (tab: PanelTab) => void
  "panel:set-tab": (tab: PanelTab) => void
  "windows:fit": (size: { width: number; height: number }) => void
  "windows:drag": (step: { phase: "start" | "move" | "end"; dx: number; dy: number }) => void
  "transcripts:reveal": () => void
  "app:open-data-dir": () => void
  "app:quit": () => void

  "session:state": () => SessionState
  "session:start": () => SessionState
  "session:pause": () => SessionState
  "session:resume": () => SessionState
  "session:finish": () => Meeting | null
  "session:cancel": () => SessionState
  "session:transcript": () => string
  "session:last-utterance": () => Utterance | null

  "chat:history": () => ChatMessage[]
  "chat:send": (text: string) => void
  "chat:answer-last": () => boolean
  "chat:clear": () => void
  "chat:cancel": () => boolean
  "chat:screenshot": (prompt?: string) => ScreenshotResult

  "suggestions:state": () => SuggestionsState
  "suggestions:refresh": () => void
  "suggestions:use": (id: string) => void
}

export interface EventMap {
  "session:state": SessionState
  "session:transcript": TranscriptEvent
  "session:utterance": Utterance
  "audio-levels": AudioLevels
  "windows:state": WindowsState
  "panel:tab": PanelTab
  "settings:changed": SettingsView
  "chat:message": ChatMessage
  "chat:chunk": ChatChunk
  "chat:busy": boolean
  "chat:cleared": undefined
  "suggestions:state": SuggestionsState
  "meetings:changed": undefined
}

export type InvokeChannel = keyof InvokeMap
export type EventChannel = keyof EventMap
