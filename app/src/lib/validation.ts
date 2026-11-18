import type { AudioDevices, ProviderKind, SettingsView, ShortcutAction, Shortcuts } from "@shared/types"

export interface ProviderFormValues {
  kind: ProviderKind
  label: string
  endpoint: string
  model: string
  apiKey: string
  clearKey: boolean
  hasExistingKey: boolean
}

export interface ProviderValidationResult {
  valid: boolean
  errors: Partial<Record<"label" | "endpoint" | "model" | "apiKey", string>>
  warning?: string
}

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"])

export function isLocalEndpoint(endpoint: string): boolean {
  try {
    const url = new URL(endpoint.trim())
    return LOCAL_HOSTS.has(url.hostname.toLowerCase()) || url.hostname === "::1"
  } catch {
    return false
  }
}

export function validateProviderInput(input: ProviderFormValues): ProviderValidationResult {
  const errors: ProviderValidationResult["errors"] = {}
  let warning: string | undefined

  const label = input.label.trim()
  if (!label) {
    errors.label = "Enter a provider name."
  } else if (label.length > 80) {
    errors.label = "Provider name must be 80 characters or fewer."
  }

  const endpoint = input.endpoint.trim()
  if (!endpoint) {
    errors.endpoint = "Enter an endpoint URL."
  } else {
    let parsed: URL | null = null
    try {
      parsed = new URL(endpoint)
    } catch {
      errors.endpoint = "Enter a valid URL."
    }
    if (parsed) {
      const protocol = parsed.protocol.toLowerCase()
      if (input.kind === "deepgram") {
        if (protocol !== "wss:" || !parsed.hostname) {
          errors.endpoint = "Deepgram endpoints must start with wss://."
        }
      } else {
        const isHttps = protocol === "https:" && Boolean(parsed.hostname)
        const isLocalHttp = protocol === "http:" && isLocalEndpoint(endpoint)
        if (!isHttps && !isLocalHttp) {
          errors.endpoint = "Use https://, or http:// only for localhost / 127.0.0.1."
        }
      }
    }
  }

  const model = input.model.trim()
  if (!model) {
    errors.model = "Enter a model ID."
  } else if (/[\r\n]/.test(model)) {
    errors.model = "Model ID cannot contain line breaks."
  }

  const typedKey = input.apiKey.trim()
  if (typedKey && input.clearKey) {
    errors.apiKey = "Choose either a new key or removing the saved key, not both."
  } else if (input.kind === "deepgram") {
    const hasKeyAfterSave = Boolean(typedKey) || (input.hasExistingKey && !input.clearKey)
    if (!hasKeyAfterSave) {
      errors.apiKey = "Deepgram requires an API key."
    }
  } else if (endpoint && !errors.endpoint && !isLocalEndpoint(endpoint)) {
    const hasKeyAfterSave = Boolean(typedKey) || (input.hasExistingKey && !input.clearKey)
    if (!hasKeyAfterSave) {
      warning = "Most hosted OpenAI-compatible providers require an API key."
    }
  }

  return { valid: Object.keys(errors).length === 0, errors, warning }
}

export interface PromptValidationResult {
  valid: boolean
  errors: Partial<Record<"title" | "content" | "notes", string>>
}

export function validatePromptInput(input: { title: string; content: string; notes: string }): PromptValidationResult {
  const errors: PromptValidationResult["errors"] = {}
  const title = input.title.trim()
  const content = input.content.trim()

  if (!title) {
    errors.title = "Enter a prompt title."
  } else if (title.length > 80) {
    errors.title = "Title must be 80 characters or fewer."
  }

  if (!content) {
    errors.content = "System instructions cannot be empty."
  } else if (input.content.length > 16000) {
    errors.content = "System instructions must be 16,000 characters or fewer."
  }

  if (input.notes.length > 32000) {
    errors.notes = "Background notes must be 32,000 characters or fewer."
  }

  return { valid: Object.keys(errors).length === 0, errors }
}

const VALID_MODIFIERS = new Set([
  "commandorcontrol",
  "cmdorctrl",
  "control",
  "ctrl",
  "command",
  "cmd",
  "super",
  "meta",
  "alt",
  "option",
  "shift",
  "altgr"
])

const VALID_NAMED_KEYS = new Set([
  "space",
  "enter",
  "return",
  "tab",
  "backspace",
  "delete",
  "insert",
  "escape",
  "esc",
  "up",
  "down",
  "left",
  "right",
  "arrowup",
  "arrowdown",
  "arrowleft",
  "arrowright",
  "home",
  "end",
  "pageup",
  "pagedown",
  "plus",
  "minus",
  "equal",
  "comma",
  "period",
  "slash",
  "backslash",
  "semicolon",
  "quote",
  "backquote",
  "bracketleft",
  "bracketright",
  "printscreen",
  "scrolllock",
  "pause",
  "capslock",
  "numlock",
  "mediaplaypause",
  "mediastop",
  "medianexttrack",
  "mediaprevioustrack",
  "volumeup",
  "volumedown",
  "volumemute"
])

function isValidShortcutKey(part: string): boolean {
  const lower = part.toLowerCase()
  if (/^[a-z0-9]$/i.test(part)) return true
  if (/^f([1-9]|1[0-9]|2[0-4])$/i.test(part)) return true
  if (/^key[a-z]$/i.test(part) || /^digit[0-9]$/i.test(part)) return true
  return VALID_NAMED_KEYS.has(lower)
}

export function normalizeShortcutKey(value: string): string {
  const parts = value
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((part) => {
      const lower = part.toLowerCase()
      if (lower === "commandorcontrol" || lower === "cmdorctrl") return "cmdorctrl"
      if (lower === "control") return "ctrl"
      if (lower === "command") return "cmd"
      if (lower === "return") return "enter"
      return lower
    })
  return parts.join("+")
}

export function validateAccelerator(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return "Shortcut cannot be empty."
  const parts = trimmed.split("+").map((p) => p.trim())
  if (parts.some((p) => !p)) return "Remove empty '+' segments."
  if (parts.length < 2) return "Include at least one modifier (e.g. CommandOrControl+L)."
  const modifiers = parts.slice(0, -1)
  const key = parts[parts.length - 1]
  const seenMods = new Set<string>()
  for (const mod of modifiers) {
    const lower = mod.toLowerCase()
    if (!VALID_MODIFIERS.has(lower)) return `Unknown modifier "${mod}".`
    const canon = lower === "commandorcontrol" || lower === "cmdorctrl" ? "cmdorctrl" : lower === "control" ? "ctrl" : lower === "command" ? "cmd" : lower
    if (seenMods.has(canon)) return `Duplicate modifier "${mod}".`
    seenMods.add(canon)
  }
  if (!isValidShortcutKey(key)) return `Unsupported key "${key}".`
  return null
}

export function validateShortcuts(shortcuts: Shortcuts): { valid: boolean; errors: Partial<Record<ShortcutAction, string>> } {
  const errors: Partial<Record<ShortcutAction, string>> = {}
  const byNormalized = new Map<string, ShortcutAction[]>()

  for (const action of Object.keys(shortcuts) as ShortcutAction[]) {
    const raw = shortcuts[action]
    const err = validateAccelerator(raw)
    if (err) {
      errors[action] = err
      continue
    }
    const norm = normalizeShortcutKey(raw)
    const list = byNormalized.get(norm) ?? []
    list.push(action)
    byNormalized.set(norm, list)
  }

  for (const actions of byNormalized.values()) {
    if (actions.length > 1) {
      for (const action of actions) {
        errors[action] = "Duplicates another shortcut."
      }
    }
  }

  return { valid: Object.keys(errors).length === 0, errors }
}

const KEY_CODE_MAP: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Minus: "Minus",
  Equal: "Equal",
  Comma: "Comma",
  Period: "Period",
  Slash: "Slash",
  Backslash: "Backslash",
  Semicolon: "Semicolon",
  Quote: "Quote",
  Backquote: "Backquote",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight"
}

const MODIFIER_ONLY_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "OS"])

export function keyEventToAccelerator(e: { key: string; code: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }): string | null {
  if (MODIFIER_ONLY_KEYS.has(e.key)) return null
  const mods: string[] = []
  if (e.ctrlKey || e.metaKey) mods.push("CommandOrControl")
  if (e.altKey) mods.push("Alt")
  if (e.shiftKey) mods.push("Shift")
  if (mods.length === 0) return null

  let keyPart: string | null = null
  if (/^Key([A-Z])$/.test(e.code)) {
    keyPart = e.code.slice(3)
  } else if (/^Digit([0-9])$/.test(e.code)) {
    keyPart = e.code.slice(5)
  } else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) {
    keyPart = e.key
  } else if (KEY_CODE_MAP[e.code]) {
    keyPart = KEY_CODE_MAP[e.code]
  } else if (/^[a-z0-9]$/i.test(e.key)) {
    keyPart = e.key.toUpperCase()
  }

  if (!keyPart) return null
  return [...mods, keyPart].join("+")
}

export const CHAT_MESSAGE_MAX_LENGTH = 12000

export function validateChatMessage(text: string): { valid: boolean; error: string | null } {
  const trimmed = text.trim()
  if (!trimmed) return { valid: false, error: null }
  if (text.length > CHAT_MESSAGE_MAX_LENGTH) {
    return { valid: false, error: `Message is too long (${text.length.toLocaleString()} / ${CHAT_MESSAGE_MAX_LENGTH.toLocaleString()} characters).` }
  }
  return { valid: true, error: null }
}

export interface SessionReadiness {
  hasTranscriber: boolean
  hasBrain: boolean
  needsMic: boolean
  needsMonitor: boolean
  micReady: boolean
  micConnected: boolean
  monitorReady: boolean
  monitorConnected: boolean
  canStart: boolean
  missingReasons: string[]
}

export function getSessionReadiness(
  settings: Pick<SettingsView, "transcriberId" | "brainId" | "audioSource" | "microphoneId" | "monitorId"> | null,
  devices: AudioDevices | null,
  devicesLoading: boolean
): SessionReadiness {
  const source = settings?.audioSource ?? "both"
  const hasTranscriber = Boolean(settings?.transcriberId)
  const hasBrain = Boolean(settings?.brainId)
  const needsMic = source !== "system"
  const needsMonitor = source !== "microphone"
  const micId = settings?.microphoneId ?? ""
  const monitorId = settings?.monitorId ?? ""

  const micReady = !needsMic || Boolean(micId)
  const monitorReady = !needsMonitor || Boolean(monitorId)
  const micConnected = !needsMic || !micId || !devices || devices.microphones.some((d) => d.id === micId)
  const monitorConnected = !needsMonitor || !monitorId || !devices || devices.monitors.some((d) => d.id === monitorId)

  const missingReasons: string[] = []
  if (!hasTranscriber) missingReasons.push("Configure a Deepgram transcription provider.")
  if (!micReady) missingReasons.push("Select a microphone.")
  else if (!micConnected) missingReasons.push("Selected microphone is currently disconnected.")
  if (!monitorReady) missingReasons.push("Select a system monitor.")
  else if (!monitorConnected) missingReasons.push("Selected system monitor is currently disconnected.")

  const canStart = settings !== null && hasTranscriber && micReady && monitorReady && !devicesLoading

  return {
    hasTranscriber,
    hasBrain,
    needsMic,
    needsMonitor,
    micReady,
    micConnected,
    monitorReady,
    monitorConnected,
    canStart,
    missingReasons
  }
}
