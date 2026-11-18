// Types shared by the Tauri frontend and the Rust backend contract.

export type WindowKind = "main" | "chat"

export type PanelTab = "answers" | "transcript" | "settings"

export type LanguageCode = "en" | "ar" | "bg" | "zh" | "hr" | "cs" | "da" | "nl" | "et" | "fi" | "fr" | "de" | "hi" | "hu" | "it" | "ja" | "ko" | "no" | "pl" | "pt" | "ro" | "ru" | "sk" | "es" | "sv" | "tr" | "uk" | "vi"

export interface Language {
  code: LanguageCode
  name: string
  instruction: string
}

/** English first, then alphabetical. */
export const LANGUAGES: Language[] = [
  { code: "en", name: "English", instruction: "Respond in English." },
  { code: "ar", name: "Arabic (العربية)", instruction: "Respond in Arabic (العربية)." },
  { code: "bg", name: "Bulgarian (Български)", instruction: "Respond in Bulgarian (Български)." },
  { code: "zh", name: "Chinese (中文)", instruction: "Respond in Chinese (中文)." },
  { code: "hr", name: "Croatian (Hrvatski)", instruction: "Respond in Croatian (Hrvatski)." },
  { code: "cs", name: "Czech (Čeština)", instruction: "Respond in Czech (Čeština)." },
  { code: "da", name: "Danish (Dansk)", instruction: "Respond in Danish (Dansk)." },
  { code: "nl", name: "Dutch (Nederlands)", instruction: "Respond in Dutch (Nederlands)." },
  { code: "et", name: "Estonian (Eesti)", instruction: "Respond in Estonian (Eesti)." },
  { code: "fi", name: "Finnish (Suomi)", instruction: "Respond in Finnish (Suomi)." },
  { code: "fr", name: "French (Français)", instruction: "Respond in French (Français)." },
  { code: "de", name: "German (Deutsch)", instruction: "Respond in German (Deutsch)." },
  { code: "hi", name: "Hindi (हिन्दी)", instruction: "Respond in Hindi (हिन्दी)." },
  { code: "hu", name: "Hungarian (Magyar)", instruction: "Respond in Hungarian (Magyar)." },
  { code: "it", name: "Italian (Italiano)", instruction: "Respond in Italian (Italiano)." },
  { code: "ja", name: "Japanese (日本語)", instruction: "Respond in Japanese (日本語)." },
  { code: "ko", name: "Korean (한국어)", instruction: "Respond in Korean (한국어)." },
  { code: "no", name: "Norwegian (Norsk)", instruction: "Respond in Norwegian Bokmål (Norsk)." },
  { code: "pl", name: "Polish (Polski)", instruction: "Respond in Polish (Polski)." },
  { code: "pt", name: "Portuguese (Português)", instruction: "Respond in Portuguese (Português)." },
  { code: "ro", name: "Romanian (Română)", instruction: "Respond in Romanian (Română)." },
  { code: "ru", name: "Russian (Русский)", instruction: "Respond in Russian (Русский язык)." },
  { code: "sk", name: "Slovak (Slovenčina)", instruction: "Respond in Slovak (Slovenčina)." },
  { code: "es", name: "Spanish (Español)", instruction: "Respond in Spanish (Español)." },
  { code: "sv", name: "Swedish (Svenska)", instruction: "Respond in Swedish (Svenska)." },
  { code: "tr", name: "Turkish (Türkçe)", instruction: "Respond in Turkish (Türkçe)." },
  { code: "uk", name: "Ukrainian (Українська)", instruction: "Respond in Ukrainian (Українська)." },
  { code: "vi", name: "Vietnamese (Tiếng Việt)", instruction: "Respond in Vietnamese (Tiếng Việt)." }
]

export function getLanguage(code: string): Language {
  return LANGUAGES.find((l) => l.code === code) ?? LANGUAGES[0]
}

export type AudioSource = "both" | "microphone" | "system"

export const AUDIO_SOURCE_LABELS: Record<AudioSource, string> = {
  both: "Microphone + system audio",
  system: "System audio only",
  microphone: "Microphone only"
}

export interface AudioDevice {
  id: string
  label: string
}

export interface AudioDevices {
  microphones: AudioDevice[]
  monitors: AudioDevice[]
}

export interface AudioLevels {
  microphone: number | null
  system: number | null
}

export type AutoAnswerMode = "off" | "questions" | "always"

export const AUTO_ANSWER_LABELS: Record<AutoAnswerMode, string> = {
  off: "Off",
  questions: "When a question is asked",
  always: "After every pause"
}

export type AnswerLength = "short" | "medium" | "auto"

export const ANSWER_FORMAT =
  "Lay out every answer to be read at a glance while speaking: first the answer itself as one bold sentence, then 2 to 4 catch points as bullets the user can expand on out loud, each a fragment of 3 to 8 words, not a sentence. No headings."

export const ANSWER_LENGTHS: Record<AnswerLength, { label: string; instruction: string }> = {
  short: {
    label: "Short",
    instruction: "Stop after the catch points."
  },
  medium: {
    label: "Medium",
    instruction: "After the catch points, add one short paragraph with the key details."
  },
  auto: {
    label: "Automatic",
    instruction: "Usually stop after the catch points; add code or a few steps after them only when the question needs it."
  }
}

export interface Prompt {
  id: string
  title: string
  content: string
  notes?: string
}

export type ShortcutAction =
  | "showToolbar"
  | "toggleAll"
  | "listen"
  | "chat"
  | "screenshot"
  | "moveLeft"
  | "moveRight"
  | "moveUp"
  | "moveDown"

export type Shortcuts = Record<ShortcutAction, string>

export const DEFAULT_SHORTCUTS: Shortcuts = {
  showToolbar: "CommandOrControl+Space",
  toggleAll: "CommandOrControl+B",
  listen: "CommandOrControl+L",
  chat: "CommandOrControl+Shift+C",
  screenshot: "CommandOrControl+Enter",
  moveLeft: "CommandOrControl+Left",
  moveRight: "CommandOrControl+Right",
  moveUp: "CommandOrControl+Up",
  moveDown: "CommandOrControl+Down"
}

export const SHORTCUT_LABELS: Record<ShortcutAction, string> = {
  showToolbar: "Show and center the toolbar",
  toggleAll: "Hide / show all windows",
  listen: "Start / pause / resume listening",
  chat: "Toggle chat",
  screenshot: "Analyze the screen in chat",
  moveLeft: "Move toolbar left",
  moveRight: "Move toolbar right",
  moveUp: "Move toolbar up",
  moveDown: "Move toolbar down"
}

export type ProviderRole = "transcription" | "brain"
export type ProviderKind = "deepgram" | "openai-compatible"

export interface ProviderView {
  id: string
  kind: ProviderKind
  role: ProviderRole
  label: string
  endpoint: string
  model: string
  hasKey: boolean
}

export interface ProviderInput {
  id?: string
  kind: ProviderKind
  label: string
  endpoint: string
  model: string
  key?: string
  clearKey?: boolean
}

export interface ProviderTestResult {
  ok: boolean
  role: ProviderRole
  streaming: boolean | null
  vision: boolean | null
  detail: string
}

export const TRANSCRIBER_PRESETS = [
  { label: "Deepgram", kind: "deepgram" as const, endpoint: "wss://api.deepgram.com/v1/listen", model: "nova-3" }
] as const

export const BRAIN_PRESETS = [
  { label: "Gemini AI Studio", kind: "openai-compatible" as const, endpoint: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.5-flash" },
  { label: "OpenAI", kind: "openai-compatible" as const, endpoint: "https://api.openai.com/v1", model: "gpt-4.1-mini" },
  { label: "OpenRouter", kind: "openai-compatible" as const, endpoint: "https://openrouter.ai/api/v1", model: "openai/gpt-4.1-mini" },
  { label: "Ollama", kind: "openai-compatible" as const, endpoint: "http://localhost:11434/v1", model: "llama3.2" }
] as const

export interface SettingsView {
  outputLanguage: LanguageCode
  audioLanguage: LanguageCode
  audioSource: AudioSource
  microphoneId: string
  monitorId: string
  denoiseMic: boolean
  autoAnswer: AutoAnswerMode
  answerLength: AnswerLength
  suggestions: boolean
  stealth: boolean
  autoLaunch: boolean
  prompts: Prompt[]
  activePromptId: string | null
  shortcuts: Shortcuts
  providers: ProviderView[]
  transcriberId: string | null
  brainId: string | null
  shortcutConflicts: ShortcutAction[]
  readOnlyReason: string | null
}

export type SettingsPatch = Partial<
  Pick<
    SettingsView,
    | "outputLanguage"
    | "audioLanguage"
    | "audioSource"
    | "microphoneId"
    | "monitorId"
    | "denoiseMic"
    | "autoAnswer"
    | "answerLength"
    | "suggestions"
    | "stealth"
    | "autoLaunch"
    | "prompts"
    | "activePromptId"
    | "shortcuts"
  >
>

export type SessionStatus = "idle" | "connecting" | "recording" | "paused"

export interface SessionState {
  status: SessionStatus
  startedAt: number | null
  notice: string | null
  connected: boolean
  audioSource: AudioSource
  microphoneId: string
  monitorId: string
}

export type Channel = "you" | "them"

export interface TranscriptSegment {
  text: string
  timestamp: number
  confidence: number
  speaker: number | null
  channel?: Channel | null
}

export interface TranscriptEvent {
  text: string
  isFinal: boolean
  speechFinal: boolean
  confidence: number
  speaker: number | null
  channel?: Channel | null
}

export interface Utterance {
  text: string
  speaker: number | null
  channel?: Channel | null
  timestamp: number
}

export type ChatRole = "user" | "assistant"
export type ChatKind = "text" | "screenshot" | "error" | "auto"

export interface ChatMessage {
  id: string
  role: ChatRole
  kind: ChatKind
  text: string
  timestamp: number
}

export interface ChatChunk {
  id: string
  text: string
  kind?: ChatKind
}

export interface Suggestion {
  id: string
  text: string
}

export interface SuggestionsState {
  topic: string
  items: Suggestion[]
  updating: boolean
}

export type QuickActionId = "say" | "ask" | "recap"

export const QUICK_ACTIONS: { id: QuickActionId; label: string; prompt: string }[] = [
  {
    id: "say",
    label: "What should I say?",
    prompt: "Based on the conversation so far, what should I say next? Give the exact words to say, ready to say out loud, not advice about what to say."
  },
  {
    id: "ask",
    label: "Questions to ask",
    prompt: "Suggest three sharp questions I could ask the other side right now, specific to this conversation."
  },
  {
    id: "recap",
    label: "Recap so far",
    prompt: "Recap the conversation so far: what was discussed, what was decided, what is still open."
  }
]

export function suggestionsVisible(settings: Pick<SettingsView, "suggestions"> | null, status: SessionStatus): boolean {
  return Boolean(settings?.suggestions) && status !== "idle"
}

export interface MeetingSummary {
  title: string
  overview: string
  keyPoints: string[]
  decisions: string[]
  actionItems: string[]
  nextSteps: string[]
  followUpQuestions: string[]
  topics: string[]
  generatedAt: number
  language: LanguageCode
}

export type MeetingStatus = "processing" | "completed" | "failed"

export interface Meeting {
  id: string
  title: string
  startTime: number
  endTime: number
  duration: number
  transcript: TranscriptSegment[]
  fullTranscriptText: string
  language: LanguageCode
  status: MeetingStatus
  summary?: MeetingSummary | null
  createdAt: number
  updatedAt: number
}

export type MigrationOutcome =
  | { state: "noLegacyData" }
  | { state: "alreadyDone"; marker: { meetingsImported: number; meetingsSkipped: number; settingsImported: boolean } }
  | { state: "imported"; marker: { meetingsImported: number; meetingsSkipped: number; settingsImported: boolean } }
  | { state: "failed"; error: string }

export interface WindowsState {
  chat: boolean
  panelTab: PanelTab
}

export interface ScreenshotResult {
  text: string
  tokens: number
  dimensions: string
}

export function speakerLabel(speaker: number | null): string {
  return speaker === null ? "" : `Speaker ${speaker + 1}`
}

export const CHANNEL_LABELS: Record<Channel, string> = { you: "You", them: "Them" }

export function segmentLabel(s: { speaker: number | null; channel?: Channel | null }): string {
  return s.channel ? CHANNEL_LABELS[s.channel] : speakerLabel(s.speaker)
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host
  } catch {
    return url
  }
}
