import { invoke as tauriInvoke } from "@tauri-apps/api/core"
import { listen as tauriListen } from "@tauri-apps/api/event"
import type { EventChannel, EventMap, InvokeChannel, InvokeMap } from "@shared/ipc"

type CommandArgs = Record<string, unknown> | undefined

function mapInvoke<K extends InvokeChannel>(channel: K, args: Parameters<InvokeMap[K]>): [string, CommandArgs] {
  switch (channel) {
    case "settings:get":
      return ["settings_get", undefined]
    case "settings:update":
      return ["settings_update", { patch: args[0] }]
    case "provider:save":
      return ["provider_save", { input: args[0] }]
    case "provider:remove":
      return ["provider_remove", { id: args[0] }]
    case "provider:select":
      return ["provider_select", { role: args[0], id: args[1] }]
    case "provider:models":
      return ["provider_models", { input: args[0] }]
    case "provider:test":
      return ["provider_test", { input: args[0] }]
    case "migration:status":
      return ["migration_status", undefined]
    case "audio:devices":
      return ["audio_devices", undefined]
    case "meetings:list":
      return ["meetings_list", undefined]
    case "meetings:get":
      return ["meetings_get", { id: args[0] }]
    case "meetings:delete":
      return ["meetings_delete", { id: args[0] }]
    case "meetings:export":
      return ["meetings_export", { id: args[0], reveal: args[1] }]
    case "reports:generate":
      return ["reports_generate", { id: args[0] }]
    case "windows:state":
      return ["windows_state", undefined]
    case "windows:show":
      return ["windows_show", { kind: args[0] }]
    case "windows:hide":
      return ["windows_hide", { kind: args[0] }]
    case "windows:toggle":
      return ["windows_toggle", { kind: args[0] }]
    case "windows:toggle-all":
      return ["windows_toggle_all", undefined]
    case "panel:show":
      return ["panel_show", { tab: args[0] }]
    case "panel:toggle":
      return ["panel_toggle", { tab: args[0] }]
    case "panel:set-tab":
      return ["panel_set_tab", { tab: args[0] }]
    case "windows:fit":
      return ["windows_fit", { size: args[0] }]
    case "shortcuts:status":
      return ["shortcuts_status", undefined]
    case "shortcuts:trigger":
      return ["shortcuts_trigger", { action: args[0] }]
    case "shortcuts:configure-system":
      return ["shortcuts_configure_system", undefined]
    case "transcripts:reveal":
      return ["transcripts_reveal", undefined]
    case "app:open-data-dir":
      return ["app_open_data_dir", undefined]
    case "app:quit":
      return ["app_quit", undefined]
    case "session:state":
      return ["session_state", undefined]
    case "session:start":
      return ["session_start", undefined]
    case "session:pause":
      return ["session_pause", undefined]
    case "session:resume":
      return ["session_resume", undefined]
    case "session:finish":
      return ["session_finish", undefined]
    case "session:cancel":
      return ["session_cancel", undefined]
    case "session:transcript":
      return ["session_transcript", undefined]
    case "session:segments":
      return ["session_segments", undefined]
    case "session:last-utterance":
      return ["session_last_utterance", undefined]
    case "chat:history":
      return ["chat_history", undefined]
    case "chat:send":
      return ["chat_send", { text: args[0] }]
    case "chat:answer-last":
      return ["chat_answer_last", undefined]
    case "chat:clear":
      return ["chat_clear", undefined]
    case "chat:cancel":
      return ["chat_cancel", undefined]
    case "chat:screenshot":
      return ["chat_screenshot", { prompt: args[0] }]
    case "suggestions:state":
      return ["suggestions_state", undefined]
    case "suggestions:refresh":
      return ["suggestions_refresh", undefined]
    case "suggestions:use":
      return ["suggestions_use", { id: args[0] }]
  }
}

export const api = {
  invoke<K extends InvokeChannel>(
    channel: K,
    ...args: Parameters<InvokeMap[K]>
  ): Promise<Awaited<ReturnType<InvokeMap[K]>>> {
    const [cmd, payload] = mapInvoke(channel, args)
    return tauriInvoke(cmd, payload)
  },
  on<K extends EventChannel>(channel: K, listener: (payload: EventMap[K]) => void): () => void {
    let active = true
    let dispose: (() => void) | undefined
    void tauriListen<EventMap[K]>(channel, (event) => {
      if (active) listener(event.payload)
    }).then((unlisten) => {
      if (active) dispose = unlisten
      else unlisten()
    })
    return () => {
      active = false
      dispose?.()
    }
  }
}

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform)
export const modKey = isMac ? "⌘" : "Ctrl"

export function shortcutLabel(accelerator: string): string {
  return accelerator
    .replace("CommandOrControl", modKey)
    .replace("CmdOrCtrl", modKey)
    .replace("Alt", isMac ? "⌥" : "Alt")
    .replace("Shift", isMac ? "⇧" : "Shift")
    .replace("Enter", "↵")
    .replace("Left", "←")
    .replace("Right", "→")
    .replace("Up", "↑")
    .replace("Down", "↓")
    .replace(/\+/g, isMac ? "" : "+")
}

export function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}
