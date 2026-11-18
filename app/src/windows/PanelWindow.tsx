import { Suspense, lazy, memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  AlertCircle,
  ArrowDown,
  Camera,
  Check,
  CircleHelp,
  Copy,
  Cpu,
  Eraser,
  FileText,
  FolderOpen,
  History,
  Keyboard,
  ListChecks,
  Loader2,
  MessageSquare,
  MessageSquareQuote,
  MessageSquareReply,
  Mic,
  Pause,
  Play,
  RefreshCw,
  SendHorizontal,
  Shield,
  Sliders,
  Sparkles,
  Square,
  Trash2,
  Volume2,
  X
} from "lucide-react"
import {
  ANSWER_LENGTHS,
  AUDIO_SOURCE_LABELS,
  AUTO_ANSWER_LABELS,
  DEFAULT_SHORTCUTS,
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
  type SessionState,
  type SettingsPatch,
  type SettingsView,
  type ShortcutStatusView,
  type SuggestionsState,
  type Utterance
} from "@shared/types"
import { t } from "@shared/i18n"
import { api, errorText, shortcutLabel } from "@/lib/api"
import { useAudioLevels, useLiveTranscript, type Interim, type TranscriptLine } from "@/lib/capture"
import { useAudioDevices, useDragToolbar, useElapsed, useEvent, useInAppShortcuts, useSessionState, useSettings, useShortcutStatus, useSuggestions } from "@/lib/hooks"
import { actionTrigger } from "@/lib/in-app-shortcuts"
import { buildAnswerTurns, computeAnswerScrollTop, selectHeardUtterance, type AnswerTurn } from "@/lib/turns"
import { CHAT_MESSAGE_MAX_LENGTH, getSessionReadiness, validateChatMessage } from "@/lib/validation"
import { cn, copyText } from "@/lib/utils"
import { markdownToPlain } from "@/lib/plain-text"
import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger
} from "@/components/ui/alert-dialog"
import { Badge, Dot } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { Markdown } from "@/components/Markdown"

const loadSettingsPanels = () => import("@/components/SettingsPanels")
const ProvidersSettings = lazy(() => loadSettingsPanels().then((m) => ({ default: m.ProvidersSettings })))
const PromptsSettings = lazy(() => loadSettingsPanels().then((m) => ({ default: m.PromptsSettings })))
const ShortcutsSettings = lazy(() => loadSettingsPanels().then((m) => ({ default: m.ShortcutsSettings })))
const MeetingsHistory = lazy(() => loadSettingsPanels().then((m) => ({ default: m.MeetingsHistory })))

type SettingsSubTab = "general" | "providers" | "prompts" | "shortcuts" | "meetings"
type SessionAction = "start" | "stop" | "pause" | "resume" | "discard"

const EMPTY_DEVICE_VALUE = "__unselected__"

const VIEW_TITLES: Record<PanelTab | SettingsSubTab, string> = {
  answers: "Live Workspace",
  transcript: "Live Transcript",
  settings: "Settings",
  general: "General Settings",
  providers: "Providers & Keys",
  prompts: "System Prompts",
  shortcuts: "Global Shortcuts",
  meetings: "Meeting History"
}

/** Spacious Atelier Obsidian floating studio attached below the toolbar. */
export function PanelWindow() {
  const session = useSessionState()
  const [settings, update, setSettings] = useSettings()
  const shortcutStatus = useShortcutStatus()
  useInAppShortcuts(shortcutStatus)
  const [tab, setTab] = useState<PanelTab>("answers")
  const [settingsSubTab, setSettingsSubTab] = useState<SettingsSubTab>("general")
  const [busyAction, setBusyAction] = useState<SessionAction | null>(null)
  const [sessionError, setSessionError] = useState<string | null>(null)
  const [sessionNotice, setSessionNotice] = useState<string | null>(null)

  const transcript = useLiveTranscript(session)
  const suggestions = useSuggestions()
  const { devices, loading: devicesLoading, error: devicesError, refresh: refreshDevices } = useAudioDevices()
  const dragToolbar = useDragToolbar()

  useEffect(() => {
    void api.invoke("windows:state").then((s) => setTab(s.panelTab))
  }, [])
  useEvent("panel:tab", setTab)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) void api.invoke("windows:hide", "chat")
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  const choose = (next: PanelTab, sub?: SettingsSubTab) => {
    if (sub) setSettingsSubTab(sub)
    setTab(next)
    void api.invoke("panel:set-tab", next)
  }

  const openProviders = () => choose("settings", "providers")

  const readiness = useMemo(
    () => getSessionReadiness(settings, devices, devicesLoading),
    [settings, devices, devicesLoading]
  )
  const handsFree = settings ? settings.autoAnswer !== "off" : false
  const railSuggestions = suggestionsVisible(settings, session.status)
  const needsSetup = settings !== null && (!settings.transcriberId || !settings.brainId)
  const needsPortalApproval =
    shortcutStatus?.authority.kind === "desktopPortal" && shortcutStatus.authority.session === "needsApproval"
  const conflictActions =
    shortcutStatus?.authority.kind === "appManaged"
      ? shortcutStatus.actions.filter((a) => a.binding.kind === "inAppFallback" && a.binding.reason === "globalConflict")
      : []
  const hasShortcutNotice = needsPortalApproval || conflictActions.length > 0

  const activeTranscriber = settings?.providers.find((p) => p.id === settings.transcriberId) ?? null
  const activeBrain = settings?.providers.find((p) => p.id === settings.brainId) ?? null

  const startListening = async () => {
    if (busyAction) return
    setSessionError(null)
    setSessionNotice(null)
    if (!readiness.canStart) {
      if (!readiness.hasTranscriber) openProviders()
      else choose("transcript")
      setSessionError(readiness.missingReasons[0] ?? "Complete audio setup before listening.")
      return
    }
    setBusyAction("start")
    try {
      await api.invoke("session:start")
    } catch (err) {
      setSessionError(errorText(err))
    } finally {
      setBusyAction(null)
    }
  }

  const stopListening = async () => {
    if (busyAction) return
    setSessionError(null)
    setSessionNotice(null)
    setBusyAction("stop")
    try {
      const meeting = await api.invoke("session:finish")
      if (!meeting) {
        setSessionNotice("Listening stopped. No speech was captured, so nothing was saved.")
      } else {
        setSessionNotice(`Saved "${meeting.title || "Meeting"}" to Meeting History.`)
      }
    } catch (err) {
      setSessionError(errorText(err))
    } finally {
      setBusyAction(null)
    }
  }

  const togglePause = async () => {
    if (busyAction) return
    setSessionError(null)
    setBusyAction(session.status === "paused" ? "resume" : "pause")
    try {
      await api.invoke(session.status === "paused" ? "session:resume" : "session:pause")
    } catch (err) {
      setSessionError(errorText(err))
    } finally {
      setBusyAction(null)
    }
  }

  const discardSession = async () => {
    if (busyAction) return
    setSessionError(null)
    setSessionNotice(null)
    setBusyAction("discard")
    try {
      await api.invoke("session:cancel")
    } catch (err) {
      setSessionError(errorText(err))
    } finally {
      setBusyAction(null)
    }
  }

  const currentHeading = tab === "settings" ? VIEW_TITLES[settingsSubTab] : VIEW_TITLES[tab]

  return (
    <TooltipProvider delay={300}>
      <div className="flex h-screen w-full flex-col p-1.5">
        <div className="studio-shell grid h-full w-full grid-cols-1 overflow-hidden md:grid-cols-[220px_1fr]">
          <aside className="hidden flex-col justify-between border-r border-white/[0.08] bg-[#0f131a] p-3.5 md:flex">
            <div className="space-y-5">
              <div {...dragToolbar} className="flex cursor-grab items-center justify-between px-2 pt-1 active:cursor-grabbing">
                <span className="text-lg font-bold tracking-[-0.02em] text-white">Meetingly</span>
                <Badge variant="outline" className="border-white/15 bg-white/[0.04] text-[10.5px] text-muted-foreground">
                  BYOK
                </Badge>
              </div>

              <nav aria-label="Workspace navigation" className="space-y-1">
                <NavItem
                  active={tab === "answers"}
                  icon={<MessageSquare className="h-4 w-4" />}
                  label={t("Live Workspace")}
                  onClick={() => choose("answers")}
                />
                <NavItem
                  active={tab === "transcript"}
                  icon={<Mic className="h-4 w-4" />}
                  label={t("Transcript")}
                  badge={
                    session.status === "recording" ? (
                      <Dot className="h-2 w-2 animate-pulse bg-rose-400" />
                    ) : session.status === "paused" ? (
                      <Dot className="h-2 w-2 bg-amber-400" />
                    ) : transcript.lines.length > 0 ? (
                      <span className="text-[11px] tabular-nums text-muted-foreground">{transcript.lines.length}</span>
                    ) : undefined
                  }
                  onClick={() => choose("transcript")}
                />

                <div className="px-2.5 pb-1 pt-4 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/75">
                  Library & Setup
                </div>

                <NavItem
                  active={tab === "settings" && settingsSubTab === "meetings"}
                  icon={<History className="h-4 w-4" />}
                  label="Meetings"
                  onClick={() => choose("settings", "meetings")}
                />
                <NavItem
                  active={tab === "settings" && settingsSubTab === "prompts"}
                  icon={<FileText className="h-4 w-4" />}
                  label="Prompts"
                  onClick={() => choose("settings", "prompts")}
                />
                <NavItem
                  active={tab === "settings" && settingsSubTab === "providers"}
                  icon={<Cpu className="h-4 w-4" />}
                  label="Providers"
                  badge={needsSetup ? <Dot className="h-2 w-2 bg-amber-400" /> : undefined}
                  onClick={() => choose("settings", "providers")}
                />
                <NavItem
                  active={tab === "settings" && settingsSubTab === "shortcuts"}
                  icon={<Keyboard className="h-4 w-4" />}
                  label="Shortcuts"
                  badge={hasShortcutNotice ? <Dot className="h-2 w-2 bg-amber-400" /> : undefined}
                  onClick={() => choose("settings", "shortcuts")}
                />
                <NavItem
                  active={tab === "settings" && settingsSubTab === "general"}
                  icon={<Sliders className="h-4 w-4" />}
                  label="General"
                  onClick={() => choose("settings", "general")}
                />
              </nav>
            </div>

            <div className="space-y-2.5">
              <button
                type="button"
                onClick={openProviders}
                className="w-full rounded-xl border border-white/[0.08] bg-white/[0.03] p-3 text-left transition-colors hover:border-white/15 hover:bg-white/[0.06] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <div className="flex items-center justify-between text-[11.5px] font-medium text-foreground/90">
                  <span>{needsSetup ? "Setup required" : "Local Keychain"}</span>
                  <Shield className={cn("h-3.5 w-3.5", settings?.stealth ? "text-emerald-400" : "text-muted-foreground")} />
                </div>
                <div className="mt-1 truncate text-[11px] text-muted-foreground">
                  {activeTranscriber ? activeTranscriber.label : "No transcriber"} · {activeBrain ? activeBrain.label : "No brain"}
                </div>
                <div className="mt-0.5 text-[11px] text-muted-foreground/80">
                  {settings?.stealth ? "Stealth mode active" : "Stealth mode off"}
                </div>
              </button>

              <div className="flex items-center gap-1 px-1">
                <Button variant="ghost" size="xs" className="flex-1 justify-start text-muted-foreground" onClick={() => void api.invoke("transcripts:reveal")}>
                  <FolderOpen className="h-3.5 w-3.5" /> Transcripts
                </Button>
                <Button variant="ghost" size="xs" className="text-muted-foreground" onClick={() => void api.invoke("app:open-data-dir")}>
                  Data
                </Button>
              </div>
            </div>
          </aside>

          <div className="flex min-h-0 min-w-0 flex-col">
            <header
              {...dragToolbar}
              className="flex h-14 shrink-0 cursor-grab items-center gap-3 border-b border-white/[0.08] bg-[#131720] px-5 active:cursor-grabbing"
            >
              <div className="flex min-w-0 items-center gap-3">
                <h1 className="truncate text-[18px] font-semibold tracking-[-0.02em] text-white">{currentHeading}</h1>

                {session.status !== "idle" && (
                  <Badge
                    variant="outline"
                    className={cn(
                      "gap-1.5 px-2.5 py-0.5 text-xs tabular-nums",
                      session.status === "recording" && "border-rose-400/35 bg-rose-500/15 text-rose-200",
                      session.status === "paused" && "border-amber-400/35 bg-amber-500/15 text-amber-200",
                      session.status === "connecting" && "border-sky-400/35 bg-sky-500/15 text-sky-200"
                    )}
                  >
                    <Dot
                      className={cn(
                        "h-2 w-2",
                        session.status === "recording" && "animate-pulse bg-rose-400",
                        session.status === "paused" && "bg-amber-400",
                        session.status === "connecting" && "animate-pulse bg-sky-400"
                      )}
                    />
                    <span>{session.status === "recording" ? "Live" : session.status === "paused" ? "Paused" : "Connecting"}</span>
                    <span>·</span>
                    <ElapsedText startedAt={session.startedAt} />
                  </Badge>
                )}
              </div>

              <div data-no-drag className="ml-auto flex shrink-0 items-center gap-2">
                {session.status !== "idle" && <LiveLevels session={session} />}

                {session.status === "idle" && tab !== "settings" && (
                  <Button
                    variant="default"
                    size="sm"
                    disabled={busyAction !== null}
                    onClick={() => void startListening()}
                    className="gap-1.5 px-3 font-semibold"
                  >
                    {busyAction === "start" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Mic className="h-3.5 w-3.5" />}
                    <span>{busyAction === "start" ? t("Starting…") : t("Start listening")}</span>
                  </Button>
                )}

                {session.status !== "idle" && (
                  <>
                    {(session.status === "recording" || session.status === "paused") && (
                      <Button variant="outline" size="sm" disabled={busyAction !== null} onClick={() => void togglePause()}>
                        {session.status === "paused" ? (
                          <>
                            <Play className="h-3.5 w-3.5 text-emerald-300" />
                            <span>{t("Resume")}</span>
                          </>
                        ) : (
                          <>
                            <Pause className="h-3.5 w-3.5" />
                            <span>{t("Pause")}</span>
                          </>
                        )}
                      </Button>
                    )}

                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={busyAction !== null}
                      onClick={() => void stopListening()}
                      title="Stop listening and save meeting transcript"
                    >
                      {busyAction === "stop" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Square className="h-3 w-3 fill-current" />}
                      <span>{busyAction === "stop" ? "Stopping…" : "Stop listening"}</span>
                    </Button>

                    {transcript.lines.length > 0 ? (
                      <AlertDialog>
                        <Tooltip>
                          <TooltipTrigger
                            render={
                              <AlertDialogTrigger
                                render={<Button variant="ghost" size="icon-sm" disabled={busyAction !== null} aria-label={t("Discard session")} />}
                              />
                            }
                          >
                            <Trash2 className="h-4 w-4 text-muted-foreground hover:text-rose-300" />
                          </TooltipTrigger>
                          <TooltipContent side="bottom">{t("Discard session")}</TooltipContent>
                        </Tooltip>
                        <AlertDialogContent size="sm">
                          <AlertDialogHeader>
                            <AlertDialogTitle>Discard live transcript?</AlertDialogTitle>
                            <AlertDialogDescription>
                              This stops listening and discards {transcript.lines.length} captured transcript{" "}
                              {transcript.lines.length === 1 ? "segment" : "segments"} without saving.
                            </AlertDialogDescription>
                          </AlertDialogHeader>
                          <AlertDialogFooter>
                            <AlertDialogCancel>Keep listening</AlertDialogCancel>
                            <AlertDialogAction variant="destructive" onClick={() => void discardSession()}>
                              Discard
                            </AlertDialogAction>
                          </AlertDialogFooter>
                        </AlertDialogContent>
                      </AlertDialog>
                    ) : (
                      <Tooltip>
                        <TooltipTrigger
                          render={
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              disabled={busyAction !== null}
                              onClick={() => void discardSession()}
                              aria-label={t("Discard session")}
                            />
                          }
                        >
                          <X className="h-4 w-4 text-muted-foreground" />
                        </TooltipTrigger>
                        <TooltipContent side="bottom">{t("Discard session")}</TooltipContent>
                      </Tooltip>
                    )}
                  </>
                )}

                <Tooltip>
                  <TooltipTrigger
                    render={
                      <Button variant="ghost" size="icon-sm" onClick={() => void api.invoke("windows:hide", "chat")} aria-label={t("Close (Esc)")} />
                    }
                  >
                    <X className="h-4 w-4" />
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    <span>{t("Close")}</span>
                    <Kbd>Esc</Kbd>
                  </TooltipContent>
                </Tooltip>
              </div>
            </header>

            <div className="flex items-center gap-1 overflow-x-auto border-b border-white/[0.08] bg-[#0f131a] px-3 py-1.5 md:hidden">
              <Button size="xs" variant={tab === "answers" ? "secondary" : "ghost"} onClick={() => choose("answers")}>
                Workspace
              </Button>
              <Button size="xs" variant={tab === "transcript" ? "secondary" : "ghost"} onClick={() => choose("transcript")}>
                Transcript
              </Button>
              <Button size="xs" variant={tab === "settings" && settingsSubTab === "meetings" ? "secondary" : "ghost"} onClick={() => choose("settings", "meetings")}>
                Meetings
              </Button>
              <Button size="xs" variant={tab === "settings" && settingsSubTab === "providers" ? "secondary" : "ghost"} onClick={() => choose("settings", "providers")}>
                Providers
              </Button>
              <Button size="xs" variant={tab === "settings" && settingsSubTab === "prompts" ? "secondary" : "ghost"} onClick={() => choose("settings", "prompts")}>
                Prompts
              </Button>
              <Button size="xs" variant={tab === "settings" && settingsSubTab === "shortcuts" ? "secondary" : "ghost"} onClick={() => choose("settings", "shortcuts")}>
                Shortcuts
              </Button>
              <Button size="xs" variant={tab === "settings" && settingsSubTab === "general" ? "secondary" : "ghost"} onClick={() => choose("settings", "general")}>
                General
              </Button>
            </div>

            {needsPortalApproval && (
              <div className="flex items-center gap-2 border-b border-amber-400/20 bg-amber-500/10 px-5 py-2 text-xs text-amber-100">
                <AlertCircle className="h-4 w-4 shrink-0 text-amber-300" />
                <span className="min-w-0 flex-1 truncate">
                  Global shortcuts need system approval on Wayland. In-app shortcuts still work while focused.
                </span>
                <Button
                  variant="secondary"
                  size="xs"
                  onClick={() => void api.invoke("shortcuts:configure-system").catch((err) => setSessionError(errorText(err)))}
                >
                  Enable global shortcuts
                </Button>
                <Button
                  variant="ghost"
                  size="xs"
                  className="text-amber-100 hover:bg-amber-400/20"
                  onClick={() => choose("settings", "shortcuts")}
                >
                  {t("Settings")}
                </Button>
              </div>
            )}

            {!needsPortalApproval && conflictActions.length > 0 && (
              <div className="flex items-center gap-2 border-b border-amber-400/20 bg-amber-500/10 px-5 py-2 text-xs text-amber-100">
                <AlertCircle className="h-4 w-4 shrink-0 text-amber-300" />
                <span className="min-w-0 flex-1 truncate">
                  {t("Shortcut conflict:")} {conflictActions.map((a) => shortcutLabel(a.configured)).join(", ")}
                </span>
                <Button
                  variant="ghost"
                  size="xs"
                  className="text-amber-100 hover:bg-amber-400/20"
                  onClick={() => choose("settings", "shortcuts")}
                >
                  {t("Edit")}
                </Button>
              </div>
            )}

            {(sessionError || devicesError || session.notice) && (
              <div className="px-5 pt-3">
                <Alert className="border-amber-400/30 bg-amber-500/10 py-2 text-amber-100">
                  <AlertCircle className="h-4 w-4 text-amber-300" />
                  <AlertDescription className="flex items-center justify-between gap-2 text-xs text-amber-100">
                    <span>{sessionError ?? devicesError ?? session.notice}</span>
                    {sessionError && (
                      <button
                        type="button"
                        onClick={() => setSessionError(null)}
                        aria-label="Dismiss error"
                        className="shrink-0 rounded p-0.5 text-amber-200 hover:bg-white/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      >
                        <X className="h-3.5 w-3.5" />
                      </button>
                    )}
                  </AlertDescription>
                </Alert>
              </div>
            )}

            {sessionNotice && !sessionError && (
              <div className="px-5 pt-3">
                <Alert className="border-emerald-400/30 bg-emerald-500/10 py-2 text-emerald-100">
                  <Check className="h-4 w-4 text-emerald-300" />
                  <AlertDescription className="flex items-center justify-between gap-2 text-xs text-emerald-100">
                    <span>{sessionNotice}</span>
                    <button
                      type="button"
                      onClick={() => setSessionNotice(null)}
                      aria-label="Dismiss notice"
                      className="shrink-0 rounded p-0.5 text-emerald-200 hover:bg-white/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <X className="h-3.5 w-3.5" />
                    </button>
                  </AlertDescription>
                </Alert>
              </div>
            )}

            <main className="flex min-h-0 min-w-0 flex-1 flex-col">
              {tab === "answers" && (
                <div className="grid min-h-0 min-w-0 flex-1 grid-cols-1 min-[840px]:grid-cols-[minmax(0,1.28fr)_minmax(0,0.82fr)]">
                  <div className="flex min-h-0 min-w-0 flex-col">
                    <Answers
                      listening={session.status !== "idle"}
                      handsFree={handsFree}
                      settings={settings}
                      shortcutStatus={shortcutStatus}
                      onToggleAutoAnswer={() => void update({ autoAnswer: handsFree ? "off" : "questions" })}
                      onAnswerLengthChange={(answerLength) => void update({ answerLength })}
                      onOpenProviders={openProviders}
                    />
                  </div>
                  <aside className="hidden min-h-0 min-w-0 flex-col overflow-hidden border-l border-white/[0.08] bg-[#121720] min-[840px]:flex">
                    <WorkspaceSidePane
                      status={session.status}
                      connected={session.connected}
                      lines={transcript.lines}
                      interim={transcript.interim}
                      suggestions={suggestions}
                      suggestionsEnabled={Boolean(settings?.suggestions)}
                      showSuggestions={railSuggestions}
                      readiness={readiness}
                      onStart={() => void startListening()}
                      onOpenTranscript={() => choose("transcript")}
                      onOpenProviders={openProviders}
                      onToggleSuggestions={() => settings && void update({ suggestions: !settings.suggestions })}
                    />
                  </aside>
                </div>
              )}

              {tab === "transcript" && (
                <TranscriptStudio
                  status={session.status}
                  connected={session.connected}
                  settings={settings}
                  devices={devices}
                  devicesLoading={devicesLoading}
                  readiness={readiness}
                  busyAction={busyAction}
                  lines={transcript.lines}
                  interim={transcript.interim}
                  onRefreshDevices={() => void refreshDevices()}
                  onStart={() => void startListening()}
                  update={update}
                  onOpenProviders={openProviders}
                />
              )}

              {tab === "settings" && settings && (
                <SettingsWorkspace
                  settings={settings}
                  shortcutStatus={shortcutStatus}
                  update={update}
                  onSettingsChange={setSettings}
                  subTab={settingsSubTab}
                  devices={devices}
                  devicesLoading={devicesLoading}
                  devicesError={devicesError}
                  onRefreshDevices={() => void refreshDevices()}
                />
              )}
            </main>
          </div>
        </div>
      </div>
    </TooltipProvider>
  )
}

function NavItem({
  active,
  icon,
  label,
  badge,
  onClick
}: {
  active: boolean
  icon: React.ReactNode
  label: string
  badge?: React.ReactNode
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-[13.5px] font-medium transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        active ? "bg-[#1e293b] text-sky-300 shadow-sm" : "text-muted-foreground hover:bg-white/[0.05] hover:text-foreground"
      )}
    >
      <span className={cn("shrink-0", active ? "text-sky-300" : "text-muted-foreground")}>{icon}</span>
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      {badge}
    </button>
  )
}

const ACTION_ICONS: Record<QuickActionId, React.ReactNode> = {
  say: <MessageSquareQuote className="h-3.5 w-3.5" />,
  ask: <CircleHelp className="h-3.5 w-3.5" />,
  recap: <ListChecks className="h-3.5 w-3.5" />
}

function WorkspaceSidePane({
  status,
  connected,
  lines,
  interim,
  suggestions,
  suggestionsEnabled,
  showSuggestions,
  readiness,
  onStart,
  onOpenTranscript,
  onOpenProviders,
  onToggleSuggestions
}: {
  status: string
  connected: boolean
  lines: TranscriptLine[]
  interim: Interim
  suggestions: SuggestionsState
  suggestionsEnabled: boolean
  showSuggestions: boolean
  readiness: ReturnType<typeof getSessionReadiness>
  onStart: () => void
  onOpenTranscript: () => void
  onOpenProviders: () => void
  onToggleSuggestions: () => void
}) {
  const [busy, setBusy] = useState(false)
  const listRef = useRef<HTMLDivElement>(null)
  useEvent("chat:busy", setBusy)

  const live = [interim.them, interim.you, interim.other].filter(Boolean).join(" ")
  const turns = useMemo(() => toTurns(lines), [lines])

  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight })
  }, [lines.length, live])

  const useSuggestion = (id: string) => void api.invoke("suggestions:use", id).catch(() => {})

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-b border-white/[0.08] px-4">
        <span className="min-w-0 truncate text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          {status === "idle" ? "Call & Quick Actions" : "Live Transcript"}
        </span>
        <div className="flex shrink-0 items-center gap-2.5">
          {status !== "idle" && (
            <Tooltip>
              <TooltipTrigger
                render={
                  <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                    <span>Suggest</span>
                    <Switch size="sm" checked={suggestionsEnabled} onCheckedChange={onToggleSuggestions} aria-label={t("Suggested questions")} />
                  </span>
                }
              />
              <TooltipContent side="bottom">
                {suggestionsEnabled ? t("Turn off suggested questions: nothing is sent to the AI for them.") : t("Suggested questions")}
              </TooltipContent>
            </Tooltip>
          )}
          <button
            type="button"
            onClick={onOpenTranscript}
            className="rounded text-xs font-medium text-sky-300 hover:text-sky-200 hover:underline underline-offset-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {status === "idle" ? "Audio setup" : "Expand"}
          </button>
        </div>
      </div>

      {showSuggestions && (
        <div className="border-b border-white/[0.08] bg-white/[0.02] px-4 py-3">
          <div className="mb-2 flex items-center gap-2">
            <span className="flex-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
              {suggestions.topic || t("Suggested")}
            </span>
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => void api.invoke("suggestions:refresh").catch(() => {})}
              disabled={suggestions.updating}
              aria-label={t("Refresh from the conversation")}
            >
              <RefreshCw className={cn("h-3 w-3", suggestions.updating && "animate-spin")} />
            </Button>
          </div>
          {suggestions.items.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {suggestions.updating ? t("Reading the conversation…") : t("Questions show up here as the conversation goes.")}
            </p>
          ) : (
            <div className="space-y-1">
              {suggestions.items.slice(0, 3).map((s) => (
                <button
                  key={s.id}
                  type="button"
                  disabled={busy}
                  onClick={() => useSuggestion(s.id)}
                  className="line-clamp-2 w-full rounded-lg border border-white/[0.08] bg-[#1c222e] px-2.5 py-1.5 text-left text-xs text-foreground/90 transition-colors hover:border-sky-400/35 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-45"
                >
                  {s.text}
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {status === "idle" ? (
        <div className="flex flex-1 flex-col justify-between p-5">
          <div className="space-y-3 rounded-xl border border-white/[0.08] bg-[#161b24] p-4">
            <div className="text-sm font-semibold text-white">Ready for your next call</div>
            <p className="text-xs leading-relaxed text-muted-foreground">
              Start listening to capture live microphone and system audio here while answers stream on the left.
            </p>
            {readiness.missingReasons.length > 0 ? (
              <div className="space-y-1.5 pt-1">
                {readiness.missingReasons.map((r) => (
                  <div key={r} className="flex items-center gap-2 text-xs text-amber-200">
                    <AlertCircle className="h-3.5 w-3.5 shrink-0 text-amber-400" />
                    <span>{r}</span>
                  </div>
                ))}
                <Button size="sm" variant="outline" className="mt-2 w-full" onClick={readiness.hasTranscriber ? onOpenTranscript : onOpenProviders}>
                  {readiness.hasTranscriber ? "Select audio devices" : "Configure Deepgram"}
                </Button>
              </div>
            ) : (
              <Button size="sm" variant="default" className="w-full font-semibold" onClick={onStart}>
                <Mic className="h-3.5 w-3.5" /> {t("Start listening")}
              </Button>
            )}
          </div>

          <div className="space-y-1.5">
            <div className="px-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Quick prompts</div>
            {QUICK_ACTIONS.map((a) => (
              <Button
                key={a.id}
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => useSuggestion(a.id)}
                className="h-9 w-full justify-start gap-2 border-white/[0.08] bg-[#161b24] text-xs text-foreground/85 hover:bg-[#1e2532]"
              >
                <span className="text-sky-300">{ACTION_ICONS[a.id]}</span>
                <span>{t(a.label)}</span>
              </Button>
            ))}
          </div>
        </div>
      ) : (
        <>
          <div ref={listRef} className="scroll selectable min-h-0 flex-1 space-y-2.5 p-4 text-[13px] leading-relaxed">
            {turns.map((turn, i) => (
              <TurnView key={i} turn={turn} />
            ))}
            {(["them", "other", "you"] as const).map((side) => {
              const words = interim[side]
              return words ? <TurnView key={`live-${side}`} turn={{ side, label: "", texts: [words] }} live /> : null
            })}
            {lines.length === 0 && !live && (
              <p className="text-xs text-muted-foreground">
                {status === "paused" ? "Listening is paused." : connected ? t("Listening…") : t("Starting…")}
              </p>
            )}
          </div>
          <div className="space-y-1 border-t border-white/[0.08] p-3">
            {QUICK_ACTIONS.map((a) => (
              <Button
                key={a.id}
                variant="ghost"
                size="xs"
                disabled={busy}
                onClick={() => useSuggestion(a.id)}
                className="h-7 w-full justify-start gap-2 text-xs text-foreground/80 hover:bg-white/[0.06] hover:text-white"
              >
                <span className="text-sky-300">{ACTION_ICONS[a.id]}</span>
                <span>{t(a.label)}</span>
              </Button>
            ))}
          </div>
        </>
      )}
    </div>
  )
}

const FOLLOW_UPS: { label: string; prompt: string }[] = [
  {
    label: "Shorter",
    prompt: "Make that answer shorter: one crisp spoken sentence and 2 short talking points."
  },
  {
    label: "Example",
    prompt: "Give one concrete example I can say out loud to back that up."
  },
  {
    label: "Deeper",
    prompt: "Go one level deeper on key tradeoffs and likely interviewer follow-ups."
  }
]

function Answers({
  listening,
  handsFree,
  settings,
  shortcutStatus,
  onToggleAutoAnswer,
  onAnswerLengthChange,
  onOpenProviders
}: {
  listening: boolean
  handsFree: boolean
  settings: SettingsView | null
  shortcutStatus: ShortcutStatusView | null
  onToggleAutoAnswer: () => void
  onAnswerLengthChange: (answerLength: AnswerLength) => void
  onOpenProviders: () => void
}) {
  const [history, setHistory] = useState<ChatMessage[]>([])
  const [stream, setStream] = useState<ChatChunk | null>(null)
  const [busy, setBusy] = useState(false)
  const [input, setInput] = useState("")
  const [inputError, setInputError] = useState<string | null>(null)
  const [heardUtterance, setHeardUtterance] = useState<Utterance | null>(null)
  const [userScrolledUp, setUserScrolledUp] = useState(false)

  const listRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const activeTurnRef = useRef<HTMLDivElement | null>(null)
  const turnRefs = useRef<(HTMLDivElement | null)[]>([])
  const pendingChunkRef = useRef<ChatChunk | null>(null)
  const rafRef = useRef<number | null>(null)
  const lastTargetScrollRef = useRef(0)
  const programmaticScrollRef = useRef(false)

  const KEYS = settings?.shortcuts ?? DEFAULT_SHORTCUTS
  const audioSource = settings?.audioSource ?? "both"

  const cancelPendingFrame = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
  }, [])

  const flushChunk = useCallback(() => {
    rafRef.current = null
    if (pendingChunkRef.current) {
      setStream(pendingChunkRef.current)
    }
  }, [])

  useEffect(() => {
    void api.invoke("chat:history").then(setHistory)
    void api.invoke("session:last-utterance").then((u) => setHeardUtterance(u ?? null))
    inputRef.current?.focus()
    return () => cancelPendingFrame()
  }, [cancelPendingFrame])

  useEvent("chat:message", (m) => {
    setHistory((h) => [...h, m])
    if (m.role === "assistant") {
      pendingChunkRef.current = null
      cancelPendingFrame()
      setStream(null)
    }
  })

  useEvent("chat:chunk", (c) => {
    const prev = pendingChunkRef.current
    pendingChunkRef.current =
      prev && prev.id === c.id ? { id: c.id, text: prev.text + c.text, kind: c.kind ?? prev.kind } : c
    if (rafRef.current === null) {
      rafRef.current = requestAnimationFrame(flushChunk)
    }
  })

  useEvent("chat:busy", (next) => {
    setBusy(next)
    if (!next) {
      pendingChunkRef.current = null
      cancelPendingFrame()
      setStream(null)
    }
  })

  useEvent("chat:cleared", () => {
    pendingChunkRef.current = null
    cancelPendingFrame()
    setHistory([])
    setStream(null)
    setUserScrolledUp(false)
  })

  useEvent("session:utterance", (u) => {
    setHeardUtterance((prev) => selectHeardUtterance(prev, u, audioSource))
  })

  useEvent("session:state", (s) => {
    if (s.status === "idle") setHeardUtterance(null)
  })

  const turns = useMemo(() => buildAnswerTurns(history, stream, busy), [history, stream, busy])
  const latestTurn = useMemo(() => turns.find((item) => item.isLatest) ?? turns[turns.length - 1] ?? null, [turns])
  const latestTurnKey = latestTurn?.id ?? null

  turnRefs.current.length = turns.length
  if (!turns.some((item) => item.isLatest)) {
    activeTurnRef.current = null
  }

  const registerTurnElement = useCallback((index: number, isLatest: boolean, el: HTMLDivElement | null) => {
    turnRefs.current[index] = el
    if (isLatest) activeTurnRef.current = el
  }, [])

  useEffect(() => {
    setUserScrolledUp(false)
  }, [latestTurnKey])

  useEffect(() => {
    const listEl = listRef.current
    if (!listEl || userScrolledUp) return
    const activeEl = activeTurnRef.current
    const activeTurnTop = activeEl
      ? activeEl.getBoundingClientRect().top - listEl.getBoundingClientRect().top + listEl.scrollTop
      : null
    const target = computeAnswerScrollTop(
      {
        scrollTop: listEl.scrollTop,
        clientHeight: listEl.clientHeight,
        scrollHeight: listEl.scrollHeight,
        activeTurnTop
      },
      userScrolledUp
    )
    if (target === null || Math.abs(listEl.scrollTop - target) < 2) return
    lastTargetScrollRef.current = target
    programmaticScrollRef.current = true
    listEl.scrollTo({ top: target })
    requestAnimationFrame(() => {
      programmaticScrollRef.current = false
    })
  }, [turns.length, latestTurnKey, stream?.text.length, busy, userScrolledUp])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return
      const listEl = listRef.current
      const items = turnRefs.current.filter((el): el is HTMLDivElement => el !== null)
      if (!listEl || items.length === 0) return
      e.preventDefault()
      const listTop = listEl.getBoundingClientRect().top
      const currentIdx = items.findIndex((el) => el.getBoundingClientRect().bottom > listTop + 24)
      const base = currentIdx === -1 ? items.length - 1 : currentIdx
      const nextIdx = e.key === "ArrowUp" ? Math.max(0, base - 1) : Math.min(items.length - 1, base + 1)
      setUserScrolledUp(nextIdx < items.length - 1)
      items[nextIdx]?.scrollIntoView({ block: "start", behavior: "smooth" })
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  const handleScroll = () => {
    if (programmaticScrollRef.current) return
    const el = listRef.current
    if (!el) return
    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    const nearTarget = lastTargetScrollRef.current > 0 && Math.abs(el.scrollTop - lastTargetScrollRef.current) <= 28
    setUserScrolledUp(distFromBottom > 32 && !nearTarget)
  }

  const jumpToLatest = () => {
    const el = listRef.current
    if (!el) return
    setUserScrolledUp(false)
    const activeEl = activeTurnRef.current
    const activeTurnTop = activeEl
      ? activeEl.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop
      : null
    const target = computeAnswerScrollTop(
      {
        scrollTop: el.scrollTop,
        clientHeight: el.clientHeight,
        scrollHeight: el.scrollHeight,
        activeTurnTop
      },
      false
    )
    if (target === null) return
    lastTargetScrollRef.current = target
    programmaticScrollRef.current = true
    el.scrollTo({ top: target, behavior: "smooth" })
    requestAnimationFrame(() => {
      programmaticScrollRef.current = false
    })
  }

  const brain = settings?.providers.find((p) => p.id === settings.brainId) ?? null
  const missingBrain = settings !== null && !brain
  const validation = validateChatMessage(input)

  const send = useCallback(
    (text: string) => {
      const check = validateChatMessage(text)
      if (check.error) {
        setInputError(check.error)
        return
      }
      if (!check.valid || busy || missingBrain) return
      setInput("")
      setInputError(null)
      setUserScrolledUp(false)
      void api.invoke("chat:send", text.trim())
      inputRef.current?.focus()
    },
    [busy, missingBrain]
  )

  return (
    <>
      {missingBrain && (
        <div className="px-5 pt-4">
          <Alert className="border-amber-400/30 bg-amber-500/10 py-2.5 text-amber-100">
            <AlertCircle className="h-4 w-4 text-amber-300" />
            <AlertDescription className="flex items-center justify-between gap-3 text-xs text-amber-100">
              <span>Configure and select a brain provider to ask questions or get auto-answers.</span>
              <Button size="xs" variant="secondary" className="shrink-0" onClick={onOpenProviders}>
                Configure
              </Button>
            </AlertDescription>
          </Alert>
        </div>
      )}

      <div className="relative flex min-h-0 flex-1 flex-col">
        <div ref={listRef} onScroll={handleScroll} className="scroll min-h-0 flex-1 space-y-5 px-6 py-5">
          {turns.length === 0 && (
            <div className="flex h-full flex-col items-center justify-center gap-4 px-6 py-10 text-center">
              <div className="space-y-1.5">
                <h2 className="text-base font-semibold text-white">Real-time meeting answers</h2>
                <p className="max-w-md text-[13.5px] leading-relaxed text-muted-foreground">
                  {handsFree
                    ? t("Ask anything, or press Listen: questions from the meeting are answered here.")
                    : t("Ask anything. Auto-answer is off.")}
                </p>
              </div>
              <div className="flex flex-wrap items-center justify-center gap-3 text-xs text-muted-foreground">
                <span className="inline-flex items-center gap-1.5">
                  <Kbd>{shortcutLabel(actionTrigger(shortcutStatus, "listen", KEYS.listen))}</Kbd>
                  <span>{t("Listen")}</span>
                </span>
                <span className="inline-flex items-center gap-1.5">
                  <Kbd>{shortcutLabel(actionTrigger(shortcutStatus, "screenshot", KEYS.screenshot))}</Kbd>
                  <span>{t("Screen")}</span>
                </span>
              </div>
            </div>
          )}
          {turns.map((turn, index) => (
            <AnswerTurnCard
              key={turn.id}
              turn={turn}
              index={index}
              busy={busy}
              onFollowUp={send}
              onTurnElement={registerTurnElement}
            />
          ))}
        </div>

        {userScrolledUp && turns.length > 0 && (
          <div className="pointer-events-none absolute inset-x-0 bottom-3 flex justify-center">
            <button
              type="button"
              onClick={jumpToLatest}
              className="pointer-events-auto inline-flex items-center gap-1.5 rounded-full border border-sky-400/45 bg-[#161c26] px-3 py-1.5 text-xs font-medium text-sky-200 transition-colors hover:border-sky-400/65 hover:bg-[#1e2532] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <ArrowDown className="h-3.5 w-3.5" />
              <span>{t("Jump to latest answer")}</span>
            </button>
          </div>
        )}
      </div>

      {listening && heardUtterance && (
        <div className="flex items-center gap-3 border-t border-white/[0.08] bg-white/[0.02] px-5 py-2.5">
          <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={heardUtterance.text}>
            <span className="mr-2 rounded bg-white/[0.07] px-1.5 py-0.5 text-[10.5px] font-semibold uppercase tracking-wide text-sky-300">
              {heardUtterance.channel === "you" ? t("You") : t("Interviewer")}
            </span>
            {heardUtterance.text}
          </span>
          <Button
            variant="secondary"
            size="sm"
            disabled={busy || missingBrain}
            onClick={() => void api.invoke("chat:answer-last")}
            title={t("Answer the last thing heard")}
          >
            <MessageSquareReply className="h-3.5 w-3.5" /> {t("Answer")}
          </Button>
        </div>
      )}

      <div className="flex flex-wrap items-center justify-between gap-2 border-t border-white/[0.08] px-5 py-1.5 text-[11.5px] text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">
          {brain ? (
            <>
              {brain.label} · {brain.model} ({hostOf(brain.endpoint)}){" "}
              <button
                type="button"
                className="ml-1.5 rounded font-medium text-sky-300 hover:text-sky-200 hover:underline underline-offset-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                onClick={onOpenProviders}
              >
                Providers
              </button>
            </>
          ) : (
            "No brain selected"
          )}
        </span>

        <div className="flex shrink-0 items-center gap-2">
          {settings && (
            <div role="group" aria-label={t("Answer length")} className="inline-flex items-center rounded-full border border-white/10 bg-[#0f131a] p-0.5">
              {(["short", "auto", "medium"] as AnswerLength[]).map((len) => {
                const active = settings.answerLength === len
                return (
                  <button
                    key={len}
                    type="button"
                    aria-pressed={active}
                    onClick={() => void onAnswerLengthChange(len)}
                    className={cn(
                      "rounded-full px-2 py-0.5 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      active ? "bg-sky-500/20 text-sky-200" : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {len === "auto" ? t("Auto") : t(ANSWER_LENGTHS[len].label)}
                  </button>
                )
              })}
            </div>
          )}

          <Tooltip>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  onClick={onToggleAutoAnswer}
                  className={cn(
                    "inline-flex shrink-0 items-center gap-1 rounded-full border px-2.5 py-0.5 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    handsFree
                      ? "border-sky-400/35 bg-sky-500/15 text-sky-200 hover:bg-sky-500/25"
                      : "border-white/10 bg-white/[0.03] text-muted-foreground hover:text-foreground"
                  )}
                />
              }
            >
              <Sparkles className="h-3 w-3" />
              <span>{handsFree ? t("Auto-answer on") : t("Auto-answer off")}</span>
            </TooltipTrigger>
            <TooltipContent className="max-w-[260px]">
              {handsFree
                ? t("Questions heard in the meeting are answered automatically. Click to turn off.")
                : t("Click to answer questions from the meeting automatically.")}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>

      <form
        className="space-y-1.5 border-t border-white/[0.08] bg-[#131720] p-3.5"
        onSubmit={(e) => {
          e.preventDefault()
          send(input)
        }}
      >
        {(inputError || validation.error) && <p className="px-1 text-xs text-rose-300">{inputError ?? validation.error}</p>}
        <div className="flex items-end gap-2">
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  disabled={busy || missingBrain}
                  onClick={() => void api.invoke("chat:screenshot").catch(() => {})}
                  aria-label={t("Analyze my screen")}
                />
              }
            >
              <Camera className="h-4 w-4" />
            </TooltipTrigger>
            <TooltipContent>{t("Analyze my screen")}</TooltipContent>
          </Tooltip>

          <Textarea
            ref={inputRef}
            rows={1}
            value={input}
            aria-label={t("Message")}
            onChange={(e) => {
              setInput(e.target.value)
              if (inputError) setInputError(null)
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
                e.preventDefault()
                send(input)
              }
            }}
            aria-invalid={Boolean(inputError || validation.error)}
            disabled={busy || missingBrain}
            placeholder={
              missingBrain
                ? "Select a brain provider first"
                : busy
                  ? t("Waiting for reply…")
                  : `${t("Type a message")} (Enter to send, Shift+Enter for newline)`
            }
            className="scroll min-h-9 max-h-28 flex-1 resize-none bg-[#0f131a] px-3 py-2 text-[13.5px] leading-snug"
          />

          <AlertDialog>
            <Tooltip>
              <TooltipTrigger
                render={
                  <AlertDialogTrigger
                    render={
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        disabled={history.length === 0 || busy}
                        aria-label={t("Clear conversation")}
                      />
                    }
                  />
                }
              >
                <Eraser className="h-4 w-4" />
              </TooltipTrigger>
              <TooltipContent>{t("Clear conversation")}</TooltipContent>
            </Tooltip>
            <AlertDialogContent size="sm">
              <AlertDialogHeader>
                <AlertDialogTitle>Clear conversation?</AlertDialogTitle>
                <AlertDialogDescription>
                  This removes the current chat messages from this workspace. Saved meeting transcripts are not affected.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => void api.invoke("chat:clear")}>
                  Clear
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>

          {busy ? (
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button type="button" variant="destructive" size="icon" onClick={() => void api.invoke("chat:cancel")} aria-label="Stop reply" />
                }
              >
                <Square className="h-3.5 w-3.5 fill-current" />
              </TooltipTrigger>
              <TooltipContent>Stop reply</TooltipContent>
            </Tooltip>
          ) : (
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button type="submit" variant="default" size="icon" disabled={missingBrain || !validation.valid} aria-label={t("Send (Enter)")} />
                }
              >
                <SendHorizontal className="h-4 w-4" />
              </TooltipTrigger>
              <TooltipContent>{t("Send (Enter)")}</TooltipContent>
            </Tooltip>
          )}
        </div>
        {input.length > CHAT_MESSAGE_MAX_LENGTH * 0.75 && (
          <div className="flex justify-end px-1 text-[11px] tabular-nums text-muted-foreground">
            {input.length.toLocaleString()} / {CHAT_MESSAGE_MAX_LENGTH.toLocaleString()}
          </div>
        )}
      </form>
    </>
  )
}

const AnswerTurnCard = memo(
  function AnswerTurnCard({
    turn,
    index,
    busy,
    onFollowUp,
    onTurnElement
  }: {
    turn: AnswerTurn
    index: number
    busy: boolean
    onFollowUp: (prompt: string) => void
    onTurnElement: (index: number, isLatest: boolean, el: HTMLDivElement | null) => void
  }) {
    const [copied, setCopied] = useState(false)
    const copyTimerRef = useRef<number | null>(null)

    useEffect(() => {
      return () => {
        if (copyTimerRef.current) clearTimeout(copyTimerRef.current)
      }
    }, [])

    const setRootRef = useCallback(
      (el: HTMLDivElement | null) => {
        onTurnElement(index, turn.isLatest, el)
      },
      [onTurnElement, index, turn.isLatest]
    )

    const bodyText = turn.response?.text ?? turn.streamingText
    const isError = turn.response?.kind === "error"
    const showCard = turn.thinking || bodyText !== null

    const copy = async () => {
      if (!turn.response) return
      if (await copyText(markdownToPlain(turn.response.text))) {
        setCopied(true)
        if (copyTimerRef.current) clearTimeout(copyTimerRef.current)
        copyTimerRef.current = window.setTimeout(() => setCopied(false), 1200)
      }
    }

    return (
      <div ref={setRootRef} className="space-y-2">
      {turn.prompt?.kind === "heard" && (
        <div className="flex items-baseline gap-2 px-1 text-[13px] leading-snug text-muted-foreground">
          <span className="shrink-0 rounded bg-white/[0.07] px-1.5 py-0.5 text-[10.5px] font-semibold uppercase tracking-wider text-sky-300">
            {t("Interviewer")}
          </span>
          <span className="selectable min-w-0 text-foreground/80">{turn.prompt.text}</span>
        </div>
      )}

      {turn.prompt?.kind === "asked" && (
        <div className="flex justify-end">
          <p className="selectable max-w-[82%] rounded-xl bg-white/[0.09] px-4 py-2 text-[13.5px] leading-relaxed text-foreground">
            {turn.prompt.text}
          </p>
        </div>
      )}

      {turn.prompt?.kind === "screen" && (
        <div className="flex items-center gap-1.5 px-1 text-[11px] font-medium text-sky-300/85">
          <Camera className="h-3.5 w-3.5" />
          <span>{t("Screen analysis")}</span>
        </div>
      )}

      {showCard && (
        <div
          className={cn(
            "group relative max-w-[68ch]",
            isError
              ? "rounded-xl border border-rose-400/25 bg-rose-500/15 px-4 py-3 text-rose-100"
              : cn(
                  "rounded-xl border p-5 transition-colors",
                  turn.isLatest ? "border-sky-400/25 bg-[#1e2532]" : "border-white/[0.08] bg-[#1e2532]/75"
                )
          )}
        >
          {turn.thinking && (
            <div className="space-y-3" aria-busy="true" aria-label={t("Thinking…")}>
              <div className="h-4 w-4/5 rounded bg-white/[0.09] animate-pulse" />
              <div className="space-y-2 pt-1">
                <div className="h-3 w-3/5 rounded bg-white/[0.06] animate-pulse" />
                <div className="h-3 w-2/3 rounded bg-white/[0.06] animate-pulse" />
                <div className="h-3 w-1/2 rounded bg-white/[0.06] animate-pulse" />
              </div>
            </div>
          )}

          {!turn.thinking && isError && turn.response && (
            <p className="selectable text-[13.5px]">{turn.response.text}</p>
          )}

          {!turn.thinking && !isError && turn.streamingText !== null && (
            <div>
              <Markdown text={turn.streamingText} streaming />
              <span className="ml-0.5 inline-block h-4 w-[2px] animate-pulse bg-sky-300 align-middle" />
            </div>
          )}

          {!turn.thinking && !isError && turn.streamingText === null && turn.response && (
            <>
              <div className="pr-7">
                <Markdown text={turn.response.text} />
              </div>
              <button
                type="button"
                onClick={() => void copy()}
                title={t("Copy")}
                aria-label={t("Copy")}
                className={cn(
                  "absolute right-2.5 top-2.5 rounded-md p-1.5 text-muted-foreground transition-opacity hover:bg-white/10 hover:text-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  turn.isLatest ? "opacity-70 hover:opacity-100" : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
                )}
              >
                {copied ? <Check className="h-3.5 w-3.5 text-emerald-300" /> : <Copy className="h-3.5 w-3.5" />}
              </button>

              {turn.isLatest && !busy && (
                <div className="mt-3.5 flex flex-wrap items-center gap-1.5 border-t border-white/[0.07] pt-3">
                  {FOLLOW_UPS.map((f) => (
                    <button
                      key={f.label}
                      type="button"
                      onClick={() => onFollowUp(f.prompt)}
                      className="rounded-md border border-white/[0.08] bg-white/[0.03] px-2.5 py-1 text-[11.5px] font-medium text-foreground/80 transition-colors hover:border-sky-400/35 hover:bg-sky-500/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      {t(f.label)}
                    </button>
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
    )
  },
  (prev, next) =>
    prev.index === next.index &&
    prev.onFollowUp === next.onFollowUp &&
    prev.onTurnElement === next.onTurnElement &&
    prev.turn.id === next.turn.id &&
    prev.turn.isLatest === next.turn.isLatest &&
    prev.turn.thinking === next.turn.thinking &&
    prev.turn.streamingText === next.turn.streamingText &&
    prev.turn.streamingKind === next.turn.streamingKind &&
    prev.turn.response === next.turn.response &&
    (!next.turn.isLatest || prev.busy === next.busy)
)

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

function TranscriptStudio(props: {
  status: string
  connected: boolean
  settings: SettingsView | null
  devices: AudioDevices | null
  devicesLoading: boolean
  readiness: ReturnType<typeof getSessionReadiness>
  busyAction: SessionAction | null
  lines: TranscriptLine[]
  interim: Interim
  onRefreshDevices: () => void
  onStart: () => void
  update: (patch: SettingsPatch) => Promise<void>
  onOpenProviders: () => void
}) {
  const {
    status,
    connected,
    settings,
    devices,
    devicesLoading,
    readiness,
    busyAction,
    lines,
    interim,
    onRefreshDevices,
    onStart,
    update,
    onOpenProviders
  } = props
  const ref = useRef<HTMLDivElement>(null)
  const [copied, setCopied] = useState(false)

  const live = [interim.them, interim.you, interim.other].filter(Boolean).join(" ")
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight })
  }, [lines.length, live])

  const source = settings?.audioSource ?? "both"
  const micId = settings?.microphoneId ?? ""
  const monitorId = settings?.monitorId ?? ""

  const turns = useMemo(() => toTurns(lines), [lines])
  const text = turns.map((turn) => (turn.label ? `${speakerText(turn.label)}: ${turn.texts.join(" ")}` : turn.texts.join(" "))).join("\n")
  const copy = async () => {
    if (await copyText(text)) {
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    }
  }

  const micLabel = (id: string) => {
    if (!id) return "Select microphone"
    return devices?.microphones.find((d) => d.id === id)?.label ?? `${id} (missing)`
  }

  const monitorLabel = (id: string) => {
    if (!id) return "Select system monitor"
    return devices?.monitors.find((d) => d.id === id)?.label ?? `${id} (missing)`
  }

  return (
    <>
      {!readiness.hasTranscriber && (
        <div className="px-6 pt-4">
          <Alert className="border-amber-400/30 bg-amber-500/10 py-2.5 text-amber-100">
            <AlertCircle className="h-4 w-4 text-amber-300" />
            <AlertDescription className="flex items-center justify-between gap-3 text-xs text-amber-100">
              <span>Configure a Deepgram transcription provider in Settings before listening.</span>
              <Button size="xs" variant="secondary" onClick={onOpenProviders}>
                Providers
              </Button>
            </AlertDescription>
          </Alert>
        </div>
      )}

      {status === "idle" ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-4 p-8 text-center">
          <div className="space-y-1.5">
            <h2 className="text-base font-semibold text-white">Live Audio Transcription</h2>
            <p className="max-w-md text-[13.5px] leading-relaxed text-muted-foreground">
              {t("The live transcript shows here while listening.")}
            </p>
          </div>
          <Button variant="default" size="default" disabled={!readiness.canStart || busyAction !== null} onClick={onStart} className="gap-2 px-5 font-semibold">
            {busyAction === "start" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Mic className="h-4 w-4" />}
            <span>{busyAction === "start" ? t("Starting…") : t("Start listening")}</span>
          </Button>
          {readiness.missingReasons.length > 0 && (
            <ul className="space-y-1 text-xs text-amber-200/90">
              {readiness.missingReasons.map((reason) => (
                <li key={reason}>{reason}</li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <div ref={ref} className="scroll selectable min-h-0 flex-1 space-y-3 px-6 py-5 text-[14px] leading-relaxed">
          {turns.map((turn, i) => (
            <TurnView key={i} turn={turn} />
          ))}
          {(["them", "other", "you"] as const).map((side) => {
            const words = interim[side]
            return words ? <TurnView key={`live-${side}`} turn={{ side, label: "", texts: [words] }} live /> : null
          })}
          {!text && !live && (
            <p className="text-sm text-muted-foreground">
              {status === "paused" ? "Listening is paused. Resume from the top bar to continue transcribing." : connected ? t("Listening…") : t("Starting…")}
            </p>
          )}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2 border-t border-white/[0.08] bg-[#131720] px-5 py-3">
        <Select value={source} onValueChange={(v) => v && void update({ audioSource: v as AudioSource })}>
          <SelectTrigger size="default" className="h-9 min-w-[160px] flex-1 bg-[#0f131a] text-xs" aria-label={t("What to transcribe")}>
            <SelectValue>{(v: AudioSource | null) => (v ? t(AUDIO_SOURCE_LABELS[v]) : t("Audio source"))}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {(Object.keys(AUDIO_SOURCE_LABELS) as AudioSource[]).map((s) => (
              <SelectItem key={s} value={s} className="text-xs">
                {t(AUDIO_SOURCE_LABELS[s])}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {source !== "system" && (
          <Select
            value={micId || EMPTY_DEVICE_VALUE}
            disabled={devicesLoading}
            onValueChange={(v) => void update({ microphoneId: !v || v === EMPTY_DEVICE_VALUE ? "" : v })}
          >
            <SelectTrigger
              size="default"
              aria-invalid={!readiness.micReady || !readiness.micConnected}
              className="h-9 min-w-[170px] flex-1 bg-[#0f131a] text-xs"
              aria-label={t("Microphone")}
            >
              <SelectValue>{(v: string | null) => micLabel(!v || v === EMPTY_DEVICE_VALUE ? "" : v)}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={EMPTY_DEVICE_VALUE} className="text-xs text-muted-foreground">
                Select microphone
              </SelectItem>
              {devices?.microphones.map((d) => (
                <SelectItem key={d.id} value={d.id} className="text-xs">
                  {d.label}
                </SelectItem>
              ))}
              {micId && !devices?.microphones.some((d) => d.id === micId) && (
                <SelectItem value={micId} className="text-xs text-amber-200">
                  {micId} (missing)
                </SelectItem>
              )}
            </SelectContent>
          </Select>
        )}

        {source !== "microphone" && (
          <Select
            value={monitorId || EMPTY_DEVICE_VALUE}
            disabled={devicesLoading}
            onValueChange={(v) => void update({ monitorId: !v || v === EMPTY_DEVICE_VALUE ? "" : v })}
          >
            <SelectTrigger
              size="default"
              aria-invalid={!readiness.monitorReady || !readiness.monitorConnected}
              className="h-9 min-w-[170px] flex-1 bg-[#0f131a] text-xs"
              aria-label="System monitor"
            >
              <SelectValue>{(v: string | null) => monitorLabel(!v || v === EMPTY_DEVICE_VALUE ? "" : v)}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={EMPTY_DEVICE_VALUE} className="text-xs text-muted-foreground">
                Select system monitor
              </SelectItem>
              {devices?.monitors.map((d) => (
                <SelectItem key={d.id} value={d.id} className="text-xs">
                  {d.label}
                </SelectItem>
              ))}
              {monitorId && !devices?.monitors.some((d) => d.id === monitorId) && (
                <SelectItem value={monitorId} className="text-xs text-amber-200">
                  {monitorId} (missing)
                </SelectItem>
              )}
            </SelectContent>
          </Select>
        )}

        <Tooltip>
          <TooltipTrigger
            render={
              <Button variant="outline" size="icon" className="h-9 w-9" onClick={onRefreshDevices} disabled={devicesLoading} aria-label="Refresh audio devices" />
            }
          >
            <RefreshCw className={cn("h-4 w-4", devicesLoading && "animate-spin")} />
          </TooltipTrigger>
          <TooltipContent>Refresh audio devices</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger
            render={<Button variant="outline" size="icon" className="h-9 w-9" onClick={copy} disabled={!text} aria-label={t("Copy transcript")} />}
          >
            {copied ? <Check className="h-4 w-4 text-emerald-300" /> : <Copy className="h-4 w-4" />}
          </TooltipTrigger>
          <TooltipContent>{copied ? "Copied" : t("Copy transcript")}</TooltipContent>
        </Tooltip>
      </div>
    </>
  )
}

const PICK = "h-9 w-[220px] shrink-0 bg-[#0f131a] text-[13px]"

function SettingsWorkspace({
  settings,
  shortcutStatus,
  update,
  onSettingsChange,
  subTab,
  devices,
  devicesLoading,
  devicesError,
  onRefreshDevices
}: {
  settings: SettingsView
  shortcutStatus: ShortcutStatusView | null
  update: (patch: SettingsPatch) => Promise<void>
  onSettingsChange: (next: SettingsView) => void
  subTab: SettingsSubTab
  devices: AudioDevices | null
  devicesLoading: boolean
  devicesError: string | null
  onRefreshDevices: () => void
}) {
  const readiness = useMemo(
    () => getSessionReadiness(settings, devices, devicesLoading),
    [settings, devices, devicesLoading]
  )
  const langName = (code: LanguageCode | null) => LANGUAGES.find((l) => l.code === code)?.name ?? "English"
  const micLabel = (id: string) => {
    if (!id) return "Select microphone"
    return devices?.microphones.find((d) => d.id === id)?.label ?? `${id} (missing)`
  }
  const monitorLabel = (id: string) => {
    if (!id) return "Select system monitor"
    return devices?.monitors.find((d) => d.id === id)?.label ?? `${id} (missing)`
  }

  return (
    <div className="scroll min-h-0 flex-1 px-6 py-5">
      {settings.readOnlyReason && (
        <Alert variant="destructive" className="mb-4 border-rose-400/30 bg-rose-500/12 py-2.5 text-rose-100">
          <AlertCircle className="h-4 w-4 text-rose-300" />
          <AlertDescription className="text-xs text-rose-100">{settings.readOnlyReason}</AlertDescription>
        </Alert>
      )}

      <div className="mx-auto max-w-[680px]">
        {subTab === "general" && (
          <div className="divide-y divide-white/[0.07]">
            <SettingRow label={t("Meeting language")} hint={t("The language spoken in the meeting.")}>
              <Select value={settings.audioLanguage} onValueChange={(v) => v && void update({ audioLanguage: v as LanguageCode })}>
                <SelectTrigger className={PICK} aria-label={t("Meeting language")}>
                  <SelectValue>{(v: LanguageCode | null) => langName(v)}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {LANGUAGES.map((l) => (
                    <SelectItem key={l.code} value={l.code} className="text-xs">
                      {l.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>

            <SettingRow label={t("Answer language")} hint={t("For answers and the meeting report.")}>
              <Select value={settings.outputLanguage} onValueChange={(v) => v && void update({ outputLanguage: v as LanguageCode })}>
                <SelectTrigger className={PICK} aria-label={t("Answer language")}>
                  <SelectValue>{(v: LanguageCode | null) => langName(v)}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {LANGUAGES.map((l) => (
                    <SelectItem key={l.code} value={l.code} className="text-xs">
                      {l.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>

            <SettingRow label={t("Audio source")} hint={t("System audio is the call; the microphone is you.")}>
              <Select value={settings.audioSource} onValueChange={(v) => v && void update({ audioSource: v as AudioSource })}>
                <SelectTrigger className={PICK} aria-label={t("Audio source")}>
                  <SelectValue>{(v: AudioSource | null) => (v ? t(AUDIO_SOURCE_LABELS[v]) : "")}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {(Object.keys(AUDIO_SOURCE_LABELS) as AudioSource[]).map((s) => (
                    <SelectItem key={s} value={s} className="text-xs">
                      {t(AUDIO_SOURCE_LABELS[s])}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>

            {settings.audioSource !== "system" && (
              <SettingRow
                label={t("Microphone")}
                hint={devicesError ?? (!readiness.micConnected ? "Saved microphone is not currently connected." : undefined)}
              >
                <div className="flex w-[220px] shrink-0 gap-1.5">
                  <Select
                    value={settings.microphoneId || EMPTY_DEVICE_VALUE}
                    disabled={devicesLoading}
                    onValueChange={(v) => void update({ microphoneId: !v || v === EMPTY_DEVICE_VALUE ? "" : v })}
                  >
                    <SelectTrigger
                      aria-invalid={!readiness.micConnected}
                      className="h-9 min-w-0 flex-1 bg-[#0f131a] text-[13px]"
                      aria-label={t("Microphone")}
                    >
                      <SelectValue>{(v: string | null) => micLabel(!v || v === EMPTY_DEVICE_VALUE ? "" : v)}</SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={EMPTY_DEVICE_VALUE} className="text-xs text-muted-foreground">
                        Select microphone
                      </SelectItem>
                      {devices?.microphones.map((d) => (
                        <SelectItem key={d.id} value={d.id} className="text-xs">
                          {d.label}
                        </SelectItem>
                      ))}
                      {settings.microphoneId && !devices?.microphones.some((d) => d.id === settings.microphoneId) && (
                        <SelectItem value={settings.microphoneId} className="text-xs text-amber-200">
                          {settings.microphoneId} (missing)
                        </SelectItem>
                      )}
                    </SelectContent>
                  </Select>
                  <Button variant="outline" size="icon" className="h-9 w-9 shrink-0" onClick={onRefreshDevices} disabled={devicesLoading} aria-label="Refresh audio devices">
                    <RefreshCw className={cn("h-3.5 w-3.5", devicesLoading && "animate-spin")} />
                  </Button>
                </div>
              </SettingRow>
            )}

            {settings.audioSource !== "microphone" && (
              <SettingRow
                label="System monitor"
                hint={!readiness.monitorConnected ? "Saved system monitor is not currently connected." : "PulseAudio / PipeWire monitor for remote participants."}
              >
                <Select
                  value={settings.monitorId || EMPTY_DEVICE_VALUE}
                  disabled={devicesLoading}
                  onValueChange={(v) => void update({ monitorId: !v || v === EMPTY_DEVICE_VALUE ? "" : v })}
                >
                  <SelectTrigger aria-invalid={!readiness.monitorConnected} className={PICK} aria-label="System monitor">
                    <SelectValue>{(v: string | null) => monitorLabel(!v || v === EMPTY_DEVICE_VALUE ? "" : v)}</SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={EMPTY_DEVICE_VALUE} className="text-xs text-muted-foreground">
                      Select system monitor
                    </SelectItem>
                    {devices?.monitors.map((d) => (
                      <SelectItem key={d.id} value={d.id} className="text-xs">
                        {d.label}
                      </SelectItem>
                    ))}
                    {settings.monitorId && !devices?.monitors.some((d) => d.id === settings.monitorId) && (
                      <SelectItem value={settings.monitorId} className="text-xs text-amber-200">
                        {settings.monitorId} (missing)
                      </SelectItem>
                    )}
                  </SelectContent>
                </Select>
              </SettingRow>
            )}

            <SettingRow label="Microphone noise suppression" hint="Applies RNNoise on the microphone channel only.">
              <Switch checked={settings.denoiseMic} onCheckedChange={(on) => void update({ denoiseMic: on })} aria-label="Microphone noise suppression" />
            </SettingRow>

            <SettingRow label={t("Auto-answer")} hint={t("Answers questions heard in the meeting by itself.")}>
              <Select value={settings.autoAnswer} onValueChange={(v) => v && void update({ autoAnswer: v as AutoAnswerMode })}>
                <SelectTrigger className={PICK} aria-label={t("Auto-answer")}>
                  <SelectValue>{(v: AutoAnswerMode | null) => (v ? t(AUTO_ANSWER_LABELS[v]) : "")}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {(Object.keys(AUTO_ANSWER_LABELS) as AutoAnswerMode[]).map((m) => (
                    <SelectItem key={m} value={m} className="text-xs">
                      {t(AUTO_ANSWER_LABELS[m])}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>

            <SettingRow label={t("Answer length")}>
              <Select value={settings.answerLength} onValueChange={(v) => v && void update({ answerLength: v as AnswerLength })}>
                <SelectTrigger className={PICK} aria-label={t("Answer length")}>
                  <SelectValue>{(v: AnswerLength | null) => (v ? t(ANSWER_LENGTHS[v].label) : "")}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {(Object.keys(ANSWER_LENGTHS) as AnswerLength[]).map((l) => (
                    <SelectItem key={l} value={l} className="text-xs">
                      {t(ANSWER_LENGTHS[l].label)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>

            <SettingRow label={t("Suggested questions")} hint={t("Off: just the transcript, nothing is sent to the AI for them.")}>
              <Switch checked={settings.suggestions} onCheckedChange={(on) => void update({ suggestions: on })} aria-label={t("Suggested questions")} />
            </SettingRow>

            <SettingRow label={t("Stealth mode")} hint={t("Hidden from screen sharing, recordings and screenshots.")}>
              <Switch checked={settings.stealth} onCheckedChange={(on) => void update({ stealth: on })} aria-label={t("Stealth mode")} />
            </SettingRow>
          </div>
        )}
        <Suspense fallback={<SettingsSkeleton />}>
          {subTab === "providers" && <ProvidersSettings settings={settings} onSettingsChange={onSettingsChange} />}
          {subTab === "prompts" && <PromptsSettings settings={settings} update={update} />}
          {subTab === "shortcuts" && <ShortcutsSettings settings={settings} shortcutStatus={shortcutStatus} update={update} />}
          {subTab === "meetings" && <MeetingsHistory />}
        </Suspense>
      </div>
    </div>
  )
}

function SettingsSkeleton() {
  return (
    <div className="space-y-3" aria-busy="true" aria-label="Loading settings">
      <div className="h-14 rounded-xl border border-white/[0.08] bg-[#1e2532] animate-pulse" />
      <div className="h-24 rounded-xl border border-white/[0.08] bg-[#1e2532] animate-pulse" />
    </div>
  )
}

function SettingRow({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex min-h-[56px] items-center gap-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium text-foreground">{label}</div>
        {hint && <p className="mt-0.5 text-xs leading-snug text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </div>
  )
}

const TurnView = memo(function TurnView({ turn, live }: { turn: Turn; live?: boolean }) {
  const mine = turn.side === "you"
  return (
    <div className={cn("flex flex-col", mine ? "items-end" : "items-start")}>
      {turn.label && <span className="mb-1 px-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{speakerText(turn.label)}</span>}
      <div
        className={cn(
          "max-w-[90%] space-y-1 rounded-xl px-3.5 py-2.5",
          mine ? "rounded-br-sm bg-sky-500/18 text-sky-50" : "rounded-bl-sm bg-[#1c222e] text-foreground",
          live && "italic opacity-75"
        )}
      >
        {turn.texts.map((line, i) => (
          <p key={i}>{line}</p>
        ))}
      </div>
    </div>
  )
})

function ElapsedText({ startedAt }: { startedAt: number | null }) {
  return <span>{useElapsed(startedAt)}</span>
}

function LiveLevels({ session }: { session: SessionState }) {
  const levels = useAudioLevels(session)
  return <Levels levels={levels} />
}

function Levels({ levels }: { levels: AudioLevels }) {
  return (
    <div className="mr-1 hidden items-center gap-3 lg:flex">
      <Meter icon={<Mic className="h-3.5 w-3.5" />} level={levels.microphone} title={t("Microphone")} />
      <Meter icon={<Volume2 className="h-3.5 w-3.5" />} level={levels.system} title={t("System audio")} />
    </div>
  )
}

function Meter({ icon, level, title }: { icon: React.ReactNode; level: number | null; title: string }) {
  const active = level !== null
  const pct = Math.round((level ?? 0) * 100)
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            tabIndex={0}
            aria-label={active ? `${title}: ${pct}%` : t("{title}: not captured", { title })}
            className={cn(
              "flex items-center gap-1.5 rounded px-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              active ? "text-foreground/90" : "text-muted-foreground/80"
            )}
          />
        }
      >
        {icon}
        <div className="h-1.5 w-10 overflow-hidden rounded-full bg-white/10">
          <div
            className={cn("h-full rounded-full transition-[width] duration-100", pct > 70 ? "bg-rose-400" : "bg-emerald-400")}
            style={{ width: `${active ? Math.max(pct, 3) : 0}%` }}
          />
        </div>
      </TooltipTrigger>
      <TooltipContent side="bottom">{active ? `${title}: ${pct}%` : t("{title}: not captured", { title })}</TooltipContent>
    </Tooltip>
  )
}
