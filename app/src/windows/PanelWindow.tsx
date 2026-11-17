import { useEffect, useRef, useState } from "react"
import {
  Camera,
  Check,
  CircleHelp,
  Copy,
  Eraser,
  FolderOpen,
  ListChecks,
  MessageSquareQuote,
  MessageSquareReply,
  Mic,
  PanelLeft,
  RefreshCw,
  SendHorizontal,
  Square,
  Volume2,
  X
} from "lucide-react"
import {
  ANSWER_LENGTHS,
  AUDIO_SOURCE_LABELS,
  AUTO_ANSWER_LABELS,
  LANGUAGES,
  QUICK_ACTIONS,
  hostOf,
  segmentLabel,
  suggestionsVisible,
  type AnswerLength,
  type AudioDevices,
  type AudioLevels,
  type AudioSource,
  type AutoAnswerMode,
  type ChatChunk,
  type ChatMessage,
  type LanguageCode,
  type PanelTab,
  type QuickActionId,
  type SettingsPatch,
  type SettingsView,
  type SuggestionsState
} from "@shared/types"
import { t } from "@shared/i18n"
import { api, errorText, shortcutLabel } from "@/lib/api"
import { useAudioLevels, useLiveTranscript, type Interim, type TranscriptLine } from "@/lib/capture"
import { useAudioDevices, useDragToolbar, useEvent, useSessionState, useSettings, useSuggestions } from "@/lib/hooks"
import { cn, copyText } from "@/lib/utils"
import { markdownToPlain } from "@/lib/plain-text"
import { Button } from "@/components/ui/button"
import { Select, Toggle } from "@/components/ui/fields"
import { Markdown } from "@/components/Markdown"
import { MeetingsHistory, PromptsSettings, ProvidersSettings, ShortcutsSettings } from "@/components/SettingsPanels"

type SettingsSubTab = "general" | "providers" | "prompts" | "shortcuts" | "meetings"

/** The panel attached under the toolbar: answers, live transcript, and local BYOK settings. */
export function PanelWindow() {
  const session = useSessionState()
  const [settings, update, setSettings] = useSettings()
  const [tab, setTab] = useState<PanelTab>("answers")
  const [settingsSubTab, setSettingsSubTab] = useState<SettingsSubTab>("general")
  const levels = useAudioLevels(session)
  const transcript = useLiveTranscript(session)
  const suggestions = useSuggestions()
  const { devices, loading: devicesLoading, error: devicesError, refresh: refreshDevices } = useAudioDevices()

  useEffect(() => {
    void api.invoke("windows:state").then((s) => setTab(s.panelTab))
  }, [])
  useEvent("panel:tab", setTab)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void api.invoke("windows:hide", "chat")
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  const choose = (next: PanelTab) => {
    setTab(next)
    void api.invoke("panel:set-tab", next)
  }

  const openProviders = () => {
    setSettingsSubTab("providers")
    choose("settings")
  }

  const handsFree = settings ? settings.autoAnswer !== "off" : false
  const rail = suggestionsVisible(settings, session.status)
  const dragToolbar = useDragToolbar()

  return (
    <div className="flex h-screen w-full flex-col p-1">
      <div className="glass flex h-full w-full flex-col overflow-hidden">
        <header {...dragToolbar} className="flex h-9 shrink-0 cursor-grab select-none items-center gap-1 border-b border-white/10 pl-1 pr-1">
          <div className="no-drag flex items-center rounded-md bg-white/[0.04] p-0.5">
            <TabButton active={tab === "answers"} onClick={() => choose("answers")}>{t("Answers")}</TabButton>
            <TabButton active={tab === "transcript"} onClick={() => choose("transcript")}>
              {t("Transcript")}
              {session.status === "recording" && <span className="ml-1 inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-rose-400" />}
            </TabButton>
            <TabButton active={tab === "settings"} onClick={() => choose("settings")}>{t("Settings")}</TabButton>
          </div>
          <div className="no-drag ml-auto flex items-center gap-0.5">
            {session.status !== "idle" && settings && (
              <Button
                variant={rail ? "secondary" : "ghost"}
                size="icon-sm"
                className="h-7 w-7"
                onClick={() => void update({ suggestions: !settings.suggestions })}
                title={rail ? t("Hide suggested questions") : t("Show suggested questions")}
              >
                <PanelLeft className="h-3.5 w-3.5" />
              </Button>
            )}
            {tab === "answers" ? (
              <button
                type="button"
                onClick={() => void update({ autoAnswer: handsFree ? "off" : "questions" })}
                title={handsFree ? t("Questions heard in the meeting are answered automatically. Click to turn off.") : t("Click to answer questions from the meeting automatically.")}
                className={cn(
                  "h-6 rounded-full px-2 text-[11px] font-medium transition-colors",
                  handsFree ? "bg-sky-500/80 text-white hover:bg-sky-400/80" : "text-white/55 hover:bg-white/10 hover:text-white"
                )}
              >
                {handsFree ? t("Auto-answer on") : t("Auto-answer off")}
              </button>
            ) : tab === "transcript" ? (
              <Levels levels={levels} />
            ) : null}
            <Button variant="ghost" size="icon-sm" className="h-7 w-7" onClick={() => void api.invoke("windows:hide", "chat")} title={t("Close (Esc)")}>
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        </header>

        {settings && settings.shortcutConflicts.length > 0 && (
          <div className="flex items-center gap-2 border-b border-amber-400/20 bg-amber-500/10 px-2.5 py-1 text-[11px] text-amber-200">
            <span className="min-w-0 flex-1 truncate">
              {t("Shortcut conflict:")} {settings.shortcutConflicts.map((a) => shortcutLabel(settings.shortcuts[a])).join(", ")}
            </span>
            <button
              type="button"
              className="no-drag shrink-0 font-medium text-amber-100 hover:text-white"
              onClick={() => {
                setSettingsSubTab("shortcuts")
                choose("settings")
              }}
            >
              {t("Edit")}
            </button>
          </div>
        )}

        <div className="flex min-h-0 flex-1">
          {rail && <Suggestions state={suggestions} onOff={() => void update({ suggestions: false })} />}
          <div className="flex min-w-0 flex-1 flex-col">
            <div className={cn("min-h-0 flex-1 flex-col", tab === "answers" ? "flex" : "hidden")}>
              <Answers
                listening={session.status !== "idle"}
                handsFree={handsFree}
                settings={settings}
                onOpenProviders={openProviders}
              />
            </div>
            <div className={cn("min-h-0 flex-1 flex-col", tab === "transcript" ? "flex" : "hidden")}>
              <Transcript
                status={session.status}
                connected={session.connected}
                settings={settings}
                devices={devices}
                devicesLoading={devicesLoading}
                devicesError={devicesError}
                onRefreshDevices={() => void refreshDevices()}
                notice={session.notice ?? null}
                lines={transcript.lines}
                interim={transcript.interim}
                update={update}
                onOpenProviders={openProviders}
              />
            </div>
            {tab === "settings" && settings && (
              <SettingsTab
                settings={settings}
                update={update}
                onSettingsChange={setSettings}
                subTab={settingsSubTab}
                setSubTab={setSettingsSubTab}
                devices={devices}
                devicesLoading={devicesLoading}
                devicesError={devicesError}
                onRefreshDevices={() => void refreshDevices()}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function TabButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-6 items-center rounded px-2.5 text-[12px] font-medium transition-colors",
        active ? "bg-white/[0.12] text-white shadow-sm" : "text-white/50 hover:text-white/80"
      )}
    >
      {children}
    </button>
  )
}

const ACTION_ICONS: Record<QuickActionId, React.ReactNode> = {
  say: <MessageSquareQuote className="h-3 w-3" />,
  ask: <CircleHelp className="h-3 w-3" />,
  recap: <ListChecks className="h-3 w-3" />
}

function Suggestions({ state, onOff }: { state: SuggestionsState; onOff: () => void }) {
  const [busy, setBusy] = useState(false)
  useEvent("chat:busy", setBusy)
  const use = (id: string) => void api.invoke("suggestions:use", id).catch(() => {})

  return (
    <aside className="flex w-[200px] shrink-0 flex-col border-r border-white/10 bg-white/[0.02]">
      <div className="flex h-7 shrink-0 items-center gap-1 pl-2.5 pr-1">
        <span className="flex-1 text-[10.5px] font-medium uppercase tracking-wide text-white/35">{t("Suggested")}</span>
        <span className="flex items-center px-1" title={t("Turn off suggested questions: nothing is sent to the AI for them.")}>
          <Toggle size="sm" checked onChange={onOff} label={t("Suggested questions")} />
        </span>
        <button
          type="button"
          onClick={() => void api.invoke("suggestions:refresh").catch(() => {})}
          disabled={state.updating}
          title={t("Refresh from the conversation")}
          className="rounded p-1 text-white/35 transition-colors hover:bg-white/10 hover:text-white/80 disabled:hover:bg-transparent"
        >
          <RefreshCw className={cn("h-3 w-3", state.updating && "animate-spin")} />
        </button>
      </div>
      {state.topic && (
        <p className="truncate px-2.5 pb-1 text-[11px] text-sky-200/55" title={state.topic}>
          {state.topic}
        </p>
      )}
      <div className="scroll min-h-0 flex-1 space-y-0.5 px-1 pb-1">
        {state.items.length === 0 ? (
          <p className="px-1.5 py-1 text-[11px] leading-snug text-white/35">
            {state.updating ? t("Reading the conversation…") : t("Questions show up here as the conversation goes.")}
          </p>
        ) : (
          state.items.map((s) => (
            <button
              key={s.id}
              type="button"
              disabled={busy}
              onClick={() => use(s.id)}
              title={s.text}
              className="line-clamp-3 w-full animate-fade-in rounded-md px-1.5 py-1 text-left text-[12px] leading-snug text-white/85 transition-colors hover:bg-white/[0.07] hover:text-white disabled:opacity-45"
            >
              {s.text}
            </button>
          ))
        )}
      </div>
      <div className="space-y-px border-t border-white/10 p-1">
        {QUICK_ACTIONS.map((a) => (
          <button
            key={a.id}
            type="button"
            disabled={busy}
            onClick={() => use(a.id)}
            className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left text-[11.5px] text-white/60 transition-colors hover:bg-white/[0.07] hover:text-white disabled:opacity-45"
          >
            <span className="text-white/40">{ACTION_ICONS[a.id]}</span>
            {t(a.label)}
          </button>
        ))}
      </div>
    </aside>
  )
}

function Answers({
  listening,
  handsFree,
  settings,
  onOpenProviders
}: {
  listening: boolean
  handsFree: boolean
  settings: SettingsView | null
  onOpenProviders: () => void
}) {
  const [history, setHistory] = useState<ChatMessage[]>([])
  const [stream, setStream] = useState<ChatChunk | null>(null)
  const [busy, setBusy] = useState(false)
  const [input, setInput] = useState("")
  const [heard, setHeard] = useState<string | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    void api.invoke("chat:history").then(setHistory)
    void api.invoke("session:last-utterance").then((u) => setHeard(u?.text ?? null))
    inputRef.current?.focus()
  }, [])
  useEvent("chat:message", (m) => {
    setHistory((h) => [...h, m])
    setStream(null)
  })
  useEvent("chat:chunk", (c) => setStream((s) => (s && s.id === c.id ? { id: c.id, text: s.text + c.text } : c)))
  useEvent("chat:busy", setBusy)
  useEvent("chat:cleared", () => {
    setHistory([])
    setStream(null)
  })
  useEvent("session:utterance", (u) => setHeard(u.text))
  useEvent("session:state", (s) => {
    if (s.status === "idle") setHeard(null)
  })
  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight })
  }, [history.length, stream?.text.length])

  const brain = settings?.providers.find((p) => p.id === settings.brainId) ?? null
  const missingBrain = settings !== null && !brain

  const send = (text: string) => {
    if (!text.trim() || busy || missingBrain) return
    setInput("")
    void api.invoke("chat:send", text)
    inputRef.current?.focus()
  }

  return (
    <>
      {missingBrain && (
        <div className="flex items-center gap-2 border-b border-amber-400/20 bg-amber-500/10 px-2.5 py-1.5 text-[11px] text-amber-100">
          <span className="min-w-0 flex-1">Configure and select a brain provider to ask questions or get auto-answers.</span>
          <Button size="sm" variant="secondary" className="h-6 px-2 text-[11px]" onClick={onOpenProviders}>
            Configure
          </Button>
        </div>
      )}

      <div ref={listRef} className="scroll min-h-0 flex-1 space-y-1.5 px-2.5 py-2">
        {history.length === 0 && !stream && (
          <p className="px-4 py-6 text-center text-[11.5px] leading-relaxed text-white/45">
            {handsFree
              ? t("Ask anything, or press Listen: questions from the meeting are answered here.")
              : t("Ask anything. Auto-answer is off.")}
          </p>
        )}
        {history.map((m) => <Message key={m.id} message={m} />)}
        {stream && (
          <div className={ANSWER}>
            <Markdown text={stream.text} />
            <span className="ml-0.5 inline-block h-3.5 w-[2px] animate-pulse bg-white/70 align-middle" />
          </div>
        )}
        {busy && !stream && <p className="animate-pulse px-1 text-[11px] text-white/45">{t("Thinking…")}</p>}
      </div>

      {listening && heard && (
        <div className="flex items-center gap-1.5 border-t border-white/10 px-2 py-1">
          <span className="min-w-0 flex-1 truncate text-[11px] text-white/50" title={heard}>
            <span className="mr-1 text-white/35">{t("Heard")}</span>
            {heard}
          </span>
          <Button variant="ghost" size="sm" className="h-6 px-2 text-[11px]" disabled={busy || missingBrain} onClick={() => void api.invoke("chat:answer-last")} title={t("Answer the last thing heard")}>
            <MessageSquareReply className="h-3 w-3" /> {t("Answer")}
          </Button>
        </div>
      )}

      {brain && (
        <div className="flex items-center gap-1.5 border-t border-white/10 px-2.5 py-1 text-[10.5px] text-white/45">
          <span className="min-w-0 flex-1 truncate">
            {brain.label} · {brain.model} ({hostOf(brain.endpoint)})
          </span>
          <button type="button" className="font-medium text-sky-300 hover:text-sky-200" onClick={onOpenProviders}>
            Providers
          </button>
        </div>
      )}

      <form
        className="flex items-center gap-1 border-t border-white/10 p-1.5"
        onSubmit={(e) => {
          e.preventDefault()
          send(input)
        }}
      >
        <Button type="button" variant="ghost" size="icon-sm" title={t("Analyze my screen")} disabled={busy || missingBrain} onClick={() => void api.invoke("chat:screenshot").catch(() => {})}>
          <Camera className="h-3.5 w-3.5" />
        </Button>
        <input
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          disabled={busy || missingBrain}
          placeholder={missingBrain ? "Select a brain provider first" : busy ? t("Waiting for reply…") : t("Type a message")}
          className="h-7 min-w-0 flex-1 rounded-md border border-white/[0.12] bg-white/5 px-2.5 text-[13px] text-white placeholder:text-white/35 focus:border-white/35 focus:outline-none disabled:opacity-50"
        />
        <Button type="button" variant="ghost" size="icon-sm" onClick={() => void api.invoke("chat:clear")} title={t("Clear conversation")} disabled={history.length === 0 && !busy}>
          <Eraser className="h-3.5 w-3.5" />
        </Button>
        {busy ? (
          <Button type="button" variant="danger" size="icon-sm" title="Stop reply" onClick={() => void api.invoke("chat:cancel")}>
            <Square className="h-3 w-3" />
          </Button>
        ) : (
          <Button type="submit" variant="primary" size="icon-sm" title={t("Send (Enter)")} disabled={missingBrain || !input.trim()}>
            <SendHorizontal className="h-3.5 w-3.5" />
          </Button>
        )}
      </form>
    </>
  )
}

const ANSWER = "rounded-lg bg-[#132238] px-3 py-2 ring-1 ring-inset ring-sky-300/10"

function Message({ message }: { message: ChatMessage }) {
  const [copied, setCopied] = useState(false)
  if (message.role === "user") {
    if (message.kind === "auto") {
      return (
        <p className="selectable px-1 pt-1 text-[11.5px] leading-snug text-white/55">
          <span className="mr-1.5 text-[10.5px] font-medium text-white/35">{t("Heard")}</span>
          {message.text}
        </p>
      )
    }
    return <p className="selectable ml-10 rounded-lg bg-white/[0.08] px-3 py-1.5 text-[12.5px] text-white/90">{message.text}</p>
  }
  const copy = async () => {
    if (await copyText(markdownToPlain(message.text))) {
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    }
  }
  return (
    <div className={cn("group relative", message.kind === "error" ? "rounded-lg bg-rose-500/15 px-3 py-2 text-rose-200" : ANSWER)}>
      {message.kind === "screenshot" && <div className="mb-0.5 flex items-center gap-1 text-[10.5px] text-sky-200/50"><Camera className="h-3 w-3" /> {t("Screen")}</div>}
      {message.kind === "error" ? <p className="selectable text-[12.5px]">{message.text}</p> : <Markdown text={message.text} />}
      {message.kind !== "error" && (
        <button
          type="button"
          onClick={copy}
          title={t("Copy")}
          className="absolute right-1 top-1 rounded p-1 text-white/40 opacity-0 transition-opacity hover:bg-white/10 hover:text-white group-hover:opacity-100"
        >
          {copied ? <Check className="h-3 w-3 text-emerald-300" /> : <Copy className="h-3 w-3" />}
        </button>
      )}
    </div>
  )
}

interface Turn {
  side: "you" | "them" | "other"
  label: string
  texts: string[]
}

function speakerText(label: string): string {
  const numbered = label.match(/^Speaker (\d+)$/)
  return numbered ? t("Speaker {n}", { n: numbered[1] }) : t(label)
}

function toTurns(lines: TranscriptLine[]): Turn[] {
  const turns: Turn[] = []
  for (const l of lines) {
    const side = l.channel ?? "other"
    const label = segmentLabel(l) || ""
    const last = turns[turns.length - 1]
    if (last && last.side === side && last.label === label) last.texts.push(l.text)
    else turns.push({ side, label, texts: [l.text] })
  }
  return turns
}

function Transcript(props: {
  status: string
  connected: boolean
  settings: SettingsView | null
  devices: AudioDevices | null
  devicesLoading: boolean
  devicesError: string | null
  onRefreshDevices: () => void
  notice: string | null
  lines: TranscriptLine[]
  interim: Interim
  update: (patch: SettingsPatch) => Promise<void>
  onOpenProviders: () => void
}) {
  const { status, connected, settings, devices, devicesLoading, devicesError, onRefreshDevices, notice, lines, interim, update, onOpenProviders } = props
  const ref = useRef<HTMLDivElement>(null)
  const [copied, setCopied] = useState(false)
  const [startError, setStartError] = useState<string | null>(null)
  const live = [interim.them, interim.you, interim.other].filter(Boolean).join(" ")
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight })
  }, [lines.length, live])

  const source = settings?.audioSource ?? "both"
  const micId = settings?.microphoneId ?? ""
  const monitorId = settings?.monitorId ?? ""
  const missingTranscriber = settings !== null && !settings.transcriberId
  const missingMic = source !== "system" && !micId
  const missingMonitor = source !== "microphone" && !monitorId
  const canStart = !missingTranscriber && !missingMic && !missingMonitor

  const turns = toTurns(lines)
  const text = turns.map((turn) => (turn.label ? `${speakerText(turn.label)}: ${turn.texts.join(" ")}` : turn.texts.join(" "))).join("\n")
  const copy = async () => {
    if (await copyText(text)) {
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    }
  }

  const startListening = async () => {
    setStartError(null)
    try {
      await api.invoke("session:start")
    } catch (err) {
      setStartError(errorText(err))
    }
  }

  const banner = startError ?? devicesError ?? notice

  return (
    <>
      {banner && <div className="mx-2 mt-1.5 rounded-md border border-amber-400/25 bg-amber-500/10 px-2.5 py-1 text-[11px] text-amber-200">{banner}</div>}
      {missingTranscriber && (
        <div className="mx-2 mt-1.5 flex items-center gap-2 rounded-md border border-amber-400/25 bg-amber-500/10 px-2.5 py-1 text-[11px] text-amber-100">
          <span className="min-w-0 flex-1">Configure a Deepgram transcription provider in Settings before listening.</span>
          <Button size="sm" variant="secondary" className="h-6 px-2 text-[11px]" onClick={onOpenProviders}>
            Providers
          </Button>
        </div>
      )}

      {status === "idle" ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center">
          <p className="text-[11.5px] text-white/50">{t("The live transcript shows here while listening.")}</p>
          <Button variant="primary" size="sm" disabled={!canStart} onClick={() => void startListening()}>
            <Mic className="h-3.5 w-3.5" /> {t("Start listening")}
          </Button>
          {!canStart && !missingTranscriber && (
            <p className="text-[11px] text-amber-200/80">
              {missingMic && missingMonitor ? "Select a microphone and system monitor below." : missingMic ? "Select a microphone below." : "Select a system monitor below."}
            </p>
          )}
        </div>
      ) : (
        <div ref={ref} className="scroll selectable min-h-0 flex-1 space-y-2 px-2.5 py-2 text-[12.5px] leading-relaxed">
          {turns.map((turn, i) => <TurnView key={i} turn={turn} />)}
          {(["them", "other", "you"] as const).map((side) => {
            const words = interim[side]
            return words ? <TurnView key={`live-${side}`} turn={{ side, label: "", texts: [words] }} live /> : null
          })}
          {!text && !live && <p className="text-white/40">{connected ? t("Listening…") : t("Starting…")}</p>}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-1 border-t border-white/10 p-1.5">
        <Select value={source} onChange={(e) => void update({ audioSource: e.target.value as AudioSource })} className="h-7 min-w-[120px] flex-1 bg-black/30 px-2 py-0 text-[11px]" title={t("What to transcribe")}>
          {(Object.keys(AUDIO_SOURCE_LABELS) as AudioSource[]).map((s) => <option key={s} value={s}>{t(AUDIO_SOURCE_LABELS[s])}</option>)}
        </Select>
        {source !== "system" && (
          <Select
            value={micId}
            onChange={(e) => void update({ microphoneId: e.target.value })}
            disabled={devicesLoading}
            className="h-7 min-w-[120px] flex-1 bg-black/30 px-2 py-0 text-[11px]"
            title="Microphone"
          >
            <option value="">Select microphone</option>
            {devices?.microphones.map((d) => <option key={d.id} value={d.id}>{d.label}</option>)}
            {micId && !devices?.microphones.some((d) => d.id === micId) && <option value={micId}>{micId} (missing)</option>}
          </Select>
        )}
        {source !== "microphone" && (
          <Select
            value={monitorId}
            onChange={(e) => void update({ monitorId: e.target.value })}
            disabled={devicesLoading}
            className="h-7 min-w-[120px] flex-1 bg-black/30 px-2 py-0 text-[11px]"
            title="System monitor"
          >
            <option value="">Select system monitor</option>
            {devices?.monitors.map((d) => <option key={d.id} value={d.id}>{d.label}</option>)}
            {monitorId && !devices?.monitors.some((d) => d.id === monitorId) && <option value={monitorId}>{monitorId} (missing)</option>}
          </Select>
        )}
        <Button variant="ghost" size="icon-sm" onClick={onRefreshDevices} disabled={devicesLoading} title="Refresh audio devices">
          <RefreshCw className={cn("h-3.5 w-3.5", devicesLoading && "animate-spin")} />
        </Button>
        <Button variant="ghost" size="icon-sm" onClick={copy} disabled={!text} title={t("Copy transcript")}>
          {copied ? <Check className="h-3.5 w-3.5 text-emerald-300" /> : <Copy className="h-3.5 w-3.5" />}
        </Button>
      </div>
    </>
  )
}

const PICK = "h-7 w-[190px] shrink-0 bg-black/30 px-2 py-0 text-[11.5px]"

function SettingsTab({
  settings,
  update,
  onSettingsChange,
  subTab,
  setSubTab,
  devices,
  devicesLoading,
  devicesError,
  onRefreshDevices
}: {
  settings: SettingsView
  update: (patch: SettingsPatch) => Promise<void>
  onSettingsChange: (next: SettingsView) => void
  subTab: SettingsSubTab
  setSubTab: (t: SettingsSubTab) => void
  devices: AudioDevices | null
  devicesLoading: boolean
  devicesError: string | null
  onRefreshDevices: () => void
}) {
  const nav: [SettingsSubTab, string][] = [
    ["general", "General"],
    ["providers", "Providers"],
    ["prompts", "Prompts"],
    ["shortcuts", "Shortcuts"],
    ["meetings", "Meetings"]
  ]

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-1 border-b border-white/10 px-2.5 py-1">
        {nav.map(([id, label]) => (
          <button
            key={id}
            type="button"
            onClick={() => setSubTab(id)}
            className={cn(
              "rounded px-2 py-0.5 text-[11px] font-medium transition-colors",
              subTab === id ? "bg-white/15 text-white" : "text-white/50 hover:text-white/80"
            )}
          >
            {label}
          </button>
        ))}
      </div>

      {settings.readOnlyReason && (
        <div className="border-b border-rose-400/20 bg-rose-500/10 px-2.5 py-1 text-[11px] text-rose-200">
          {settings.readOnlyReason}
        </div>
      )}

      <div className="scroll min-h-0 flex-1 px-2.5">
        {subTab === "general" && (
          <div className="divide-y divide-white/[0.06]">
            <SettingRow label={t("Meeting language")} hint={t("The language spoken in the meeting.")}>
              <Select value={settings.audioLanguage} onChange={(e) => void update({ audioLanguage: e.target.value as LanguageCode })} className={PICK} aria-label={t("Meeting language")}>
                {LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
              </Select>
            </SettingRow>
            <SettingRow label={t("Answer language")} hint={t("For answers and the meeting report.")}>
              <Select value={settings.outputLanguage} onChange={(e) => void update({ outputLanguage: e.target.value as LanguageCode })} className={PICK} aria-label={t("Answer language")}>
                {LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.name}</option>)}
              </Select>
            </SettingRow>
            <SettingRow label={t("Audio source")} hint={t("System audio is the call; the microphone is you.")}>
              <Select value={settings.audioSource} onChange={(e) => void update({ audioSource: e.target.value as AudioSource })} className={PICK} aria-label={t("Audio source")}>
                {(Object.keys(AUDIO_SOURCE_LABELS) as AudioSource[]).map((s) => <option key={s} value={s}>{t(AUDIO_SOURCE_LABELS[s])}</option>)}
              </Select>
            </SettingRow>
            {settings.audioSource !== "system" && (
              <SettingRow label={t("Microphone")} hint={devicesError ?? undefined}>
                <div className="flex w-[190px] shrink-0 gap-1">
                  <Select value={settings.microphoneId} onChange={(e) => void update({ microphoneId: e.target.value })} disabled={devicesLoading} className="h-7 min-w-0 flex-1 bg-black/30 px-2 py-0 text-[11.5px]">
                    <option value="">Select microphone</option>
                    {devices?.microphones.map((d) => <option key={d.id} value={d.id}>{d.label}</option>)}
                    {settings.microphoneId && !devices?.microphones.some((d) => d.id === settings.microphoneId) && (
                      <option value={settings.microphoneId}>{settings.microphoneId} (missing)</option>
                    )}
                  </Select>
                  <Button variant="ghost" size="icon-sm" className="h-7 w-7 shrink-0" onClick={onRefreshDevices} disabled={devicesLoading}>
                    <RefreshCw className={cn("h-3 w-3", devicesLoading && "animate-spin")} />
                  </Button>
                </div>
              </SettingRow>
            )}
            {settings.audioSource !== "microphone" && (
              <SettingRow label="System monitor" hint="PulseAudio / PipeWire monitor for remote participants.">
                <Select value={settings.monitorId} onChange={(e) => void update({ monitorId: e.target.value })} disabled={devicesLoading} className={PICK}>
                  <option value="">Select system monitor</option>
                  {devices?.monitors.map((d) => <option key={d.id} value={d.id}>{d.label}</option>)}
                  {settings.monitorId && !devices?.monitors.some((d) => d.id === settings.monitorId) && (
                    <option value={settings.monitorId}>{settings.monitorId} (missing)</option>
                  )}
                </Select>
              </SettingRow>
            )}
            <SettingRow label="Microphone noise suppression" hint="Applies RNNoise on the microphone channel only.">
              <Toggle checked={settings.denoiseMic} onChange={(on) => void update({ denoiseMic: on })} label="Microphone noise suppression" />
            </SettingRow>
            <SettingRow label={t("Auto-answer")} hint={t("Answers questions heard in the meeting by itself.")}>
              <Select value={settings.autoAnswer} onChange={(e) => void update({ autoAnswer: e.target.value as AutoAnswerMode })} className={PICK} aria-label={t("Auto-answer")}>
                {(Object.keys(AUTO_ANSWER_LABELS) as AutoAnswerMode[]).map((m) => <option key={m} value={m}>{t(AUTO_ANSWER_LABELS[m])}</option>)}
              </Select>
            </SettingRow>
            <SettingRow label={t("Answer length")}>
              <Select value={settings.answerLength} onChange={(e) => void update({ answerLength: e.target.value as AnswerLength })} className={PICK} aria-label={t("Answer length")}>
                {(Object.keys(ANSWER_LENGTHS) as AnswerLength[]).map((l) => <option key={l} value={l}>{t(ANSWER_LENGTHS[l].label)}</option>)}
              </Select>
            </SettingRow>
            <SettingRow label={t("Suggested questions")} hint={t("Off: just the transcript, nothing is sent to the AI for them.")}>
              <Toggle checked={settings.suggestions} onChange={(on) => void update({ suggestions: on })} label={t("Suggested questions")} />
            </SettingRow>
            <SettingRow label={t("Stealth mode")} hint={t("Hidden from screen sharing, recordings and screenshots.")}>
              <Toggle checked={settings.stealth} onChange={(on) => void update({ stealth: on })} label={t("Stealth mode")} />
            </SettingRow>
          </div>
        )}
        {subTab === "providers" && <ProvidersSettings settings={settings} onSettingsChange={onSettingsChange} />}
        {subTab === "prompts" && <PromptsSettings settings={settings} update={update} />}
        {subTab === "shortcuts" && <ShortcutsSettings settings={settings} update={update} />}
        {subTab === "meetings" && <MeetingsHistory />}
      </div>

      <div className="flex items-center gap-1 border-t border-white/10 p-1.5">
        <Button variant="ghost" size="sm" className="h-6 px-2 text-[11px]" onClick={() => void api.invoke("transcripts:reveal")} title="Saved .txt transcripts">
          <FolderOpen className="h-3 w-3" /> {t("Saved transcripts")}
        </Button>
        <span className="min-w-0 flex-1 truncate text-center text-[10.5px] text-white/30">Local BYOK</span>
        <Button variant="ghost" size="sm" className="h-6 px-2 text-[11px]" onClick={() => void api.invoke("app:open-data-dir")}>
          App data
        </Button>
      </div>
    </div>
  )
}

function SettingRow({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex min-h-[40px] items-center gap-3 py-1.5">
      <div className="min-w-0 flex-1">
        <div className="truncate text-[12px] font-medium text-white/90" title={label}>{label}</div>
        {hint && <p className="line-clamp-2 text-[10.5px] leading-snug text-white/40">{hint}</p>}
      </div>
      {children}
    </div>
  )
}

function TurnView({ turn, live }: { turn: Turn; live?: boolean }) {
  const mine = turn.side === "you"
  return (
    <div className={cn("flex flex-col", mine ? "items-end" : "items-start")}>
      {turn.label && <span className="mb-0.5 px-1 text-[10.5px] font-medium text-white/40">{speakerText(turn.label)}</span>}
      <div
        className={cn(
          "max-w-[88%] space-y-1 rounded-lg px-2.5 py-1.5",
          mine ? "rounded-br-sm bg-sky-500/15 text-sky-50/90" : "rounded-bl-sm bg-white/[0.06] text-white/90",
          live && "italic opacity-70"
        )}
      >
        {turn.texts.map((line, i) => <p key={i}>{line}</p>)}
      </div>
    </div>
  )
}

function Levels({ levels }: { levels: AudioLevels }) {
  return (
    <div className="mr-1 flex items-center gap-2">
      <Meter icon={<Mic className="h-3 w-3" />} level={levels.microphone} title={t("Microphone")} />
      <Meter icon={<Volume2 className="h-3 w-3" />} level={levels.system} title={t("System audio")} />
    </div>
  )
}

function Meter({ icon, level, title }: { icon: React.ReactNode; level: number | null; title: string }) {
  const active = level !== null
  const pct = Math.round((level ?? 0) * 100)
  return (
    <div className={cn("flex items-center gap-1", active ? "text-white/70" : "text-white/25")} title={active ? `${title}: ${pct}%` : t("{title}: not captured", { title })}>
      {icon}
      <div className="h-1 w-9 overflow-hidden rounded-full bg-white/10">
        <div className={cn("h-full rounded-full transition-[width] duration-100", pct > 70 ? "bg-rose-400" : "bg-emerald-400")} style={{ width: `${active ? Math.max(pct, 2) : 0}%` }} />
      </div>
    </div>
  )
}
