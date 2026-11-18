import { useEffect, useRef, useState } from "react"
import {
  Camera,
  Captions,
  Check,
  EyeOff,
  GripVertical,
  Loader2,
  MessageSquare,
  Mic,
  Pause,
  Play,
  Power,
  Settings,
  Square,
  X
} from "lucide-react"
import { DEFAULT_SHORTCUTS, type PanelTab } from "@shared/types"
import { t } from "@shared/i18n"
import { api, errorText, shortcutLabel } from "@/lib/api"
import { useDragToolbar, useElapsed, useFitWindow, useInAppShortcuts, useSessionState, useSettings, useShortcutStatus, useWindowsState } from "@/lib/hooks"
import { actionTrigger } from "@/lib/in-app-shortcuts"
import { Button } from "@/components/ui/button"
import { Dot } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import { cn } from "@/lib/utils"

type ToolbarAction = "listen" | "pause" | "resume" | "finish" | "cancel" | "screenshot"

/** The always-on-top breathable floating toolbar. Everything else hangs off it. */
export function MainWindow() {
  const ref = useFitWindow<HTMLDivElement>()
  const session = useSessionState()
  const windows = useWindowsState()
  const [settings] = useSettings()
  const shortcutStatus = useShortcutStatus()
  useInAppShortcuts(shortcutStatus)
  const KEYS = settings?.shortcuts ?? DEFAULT_SHORTCUTS
  const toggleAllTrigger = actionTrigger(shortcutStatus, "toggleAll", KEYS.toggleAll)
  const listenTrigger = actionTrigger(shortcutStatus, "listen", KEYS.listen)
  const chatTrigger = actionTrigger(shortcutStatus, "chat", KEYS.chat)
  const screenshotTrigger = actionTrigger(shortcutStatus, "screenshot", KEYS.screenshot)
  const dragToolbar = useDragToolbar()

  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<ToolbarAction | null>(null)
  const [confirmDiscard, setConfirmDiscard] = useState(false)
  const errorTimer = useRef<number | null>(null)
  const discardTimer = useRef<number | null>(null)

  useEffect(() => {
    return () => {
      if (errorTimer.current) clearTimeout(errorTimer.current)
      if (discardTimer.current) clearTimeout(discardTimer.current)
    }
  }, [])

  useEffect(() => {
    if (session.status === "idle") setConfirmDiscard(false)
  }, [session.status])

  const showError = (message: string) => {
    setError(message)
    if (errorTimer.current) clearTimeout(errorTimer.current)
    errorTimer.current = window.setTimeout(() => setError(null), 6500)
  }

  const run = async (actionName: ToolbarAction, action: () => Promise<unknown>) => {
    if (busy) return
    setBusy(actionName)
    setError(null)
    try {
      await action()
    } catch (err) {
      showError(errorText(err))
    } finally {
      setBusy(null)
    }
  }

  const listen = () => {
    if (settings && !settings.transcriberId) {
      void api.invoke("panel:show", "transcript")
      showError("Add a Deepgram transcription provider in Settings before listening.")
      return
    }
    void run("listen", async () => {
      await api.invoke("panel:show", "transcript")
      await api.invoke("session:start")
    })
  }

  const finish = () =>
    void run("finish", async () => {
      const meeting = await api.invoke("session:finish")
      if (!meeting) {
        showError("No speech was captured yet — session ended.")
        return
      }
      await api.invoke("transcripts:reveal")
    })

  const discard = () => {
    if (!confirmDiscard) {
      setConfirmDiscard(true)
      if (discardTimer.current) clearTimeout(discardTimer.current)
      discardTimer.current = window.setTimeout(() => setConfirmDiscard(false), 3500)
      return
    }
    setConfirmDiscard(false)
    void run("cancel", () => api.invoke("session:cancel"))
  }

  const screenshot = () => {
    if (settings && !settings.brainId) {
      void api.invoke("panel:show", "answers")
      showError("Select a brain provider in Settings to analyze your screen.")
      return
    }
    void run("screenshot", async () => {
      await api.invoke("panel:show", "answers")
      await api.invoke("chat:screenshot")
    })
  }

  const showing = (tab: PanelTab) => windows.chat && windows.panelTab === tab
  const needsTranscriber = settings !== null && !settings.transcriberId
  const needsBrain = settings !== null && !settings.brainId
  const needsSetup = needsTranscriber || needsBrain

  const live = session.status === "recording"
  const paused = session.status === "paused"
  const connecting = session.status === "connecting"
  const bannerText = error ?? session.notice

  return (
    <div ref={ref} className="inline-block p-1.5">
      <div
        {...dragToolbar}
        className="studio-toolbar flex h-12 cursor-grab items-center gap-1.5 px-2.5 active:cursor-grabbing"
      >
        <DragHandle />

        <Button
          variant="ghost"
          size="icon"
          onClick={() => void api.invoke("windows:toggle-all")}
          aria-label={t("Hide all windows ({shortcut})", { shortcut: shortcutLabel(toggleAllTrigger) })}
          title={t("Hide all windows ({shortcut})", { shortcut: shortcutLabel(toggleAllTrigger) })}
        >
          <EyeOff className="h-4 w-4" />
        </Button>

        <Separator orientation="vertical" className="mx-0.5 h-5" />

        {session.status === "idle" ? (
          <Button
            variant={needsTranscriber ? "outline" : "default"}
            size="default"
            disabled={busy !== null}
            onClick={listen}
            className="gap-2 px-3.5 text-[13px] font-semibold"
            title={t("Start listening ({shortcut})", { shortcut: shortcutLabel(listenTrigger) })}
          >
            {busy === "listen" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Mic className="h-4 w-4" />}
            <span>{t("Listen")}</span>
          </Button>
        ) : (
          <div className="flex items-center gap-1">
            <Button
              variant={showing("transcript") ? "secondary" : "ghost"}
              size="default"
              className={cn(
                "gap-2 px-3 text-[13px] font-medium tabular-nums",
                live ? "text-rose-300" : paused ? "text-amber-300" : "text-sky-300"
              )}
              onClick={() => void api.invoke("panel:toggle", "transcript")}
              title={showing("transcript") ? t("Hide the live transcript") : t("Show the live transcript")}
            >
              <Dot className={cn("h-2 w-2", live && "animate-pulse bg-rose-400", paused && "bg-amber-400", connecting && "animate-pulse bg-sky-400")} />
              <ElapsedText startedAt={session.startedAt} />
              <Captions className="h-4 w-4 opacity-80" />
            </Button>

            {live && (
              <Button
                variant="ghost"
                size="icon"
                disabled={busy !== null}
                onClick={() => void run("pause", () => api.invoke("session:pause"))}
                aria-label={t("Pause")}
                title={t("Pause")}
              >
                <Pause className="h-4 w-4" />
              </Button>
            )}
            {paused && (
              <Button
                variant="ghost"
                size="icon"
                disabled={busy !== null}
                onClick={() => void run("resume", () => api.invoke("session:resume"))}
                aria-label={t("Resume")}
                title={t("Resume")}
              >
                <Play className="h-4 w-4 text-emerald-300" />
              </Button>
            )}

            <Button
              variant="ghost"
              size="icon"
              disabled={busy !== null}
              className="text-emerald-300 hover:bg-emerald-500/15 hover:text-emerald-200"
              onClick={finish}
              aria-label={t("Finish and save meeting")}
              title={t("Finish and save meeting")}
            >
              {busy === "finish" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Square className="h-3.5 w-3.5 fill-current" />}
            </Button>

            <Button
              variant={confirmDiscard ? "destructive" : "ghost"}
              size={confirmDiscard ? "sm" : "icon"}
              disabled={busy !== null}
              className={cn(!confirmDiscard && "text-muted-foreground hover:text-rose-200")}
              onClick={discard}
              aria-label={confirmDiscard ? "Confirm discard session" : t("Discard session")}
              title={confirmDiscard ? "Click again to discard without saving" : t("Discard session")}
            >
              {confirmDiscard ? (
                <>
                  <X className="h-3.5 w-3.5" />
                  <span>Discard?</span>
                </>
              ) : (
                <X className="h-4 w-4" />
              )}
            </Button>
          </div>
        )}

        <Separator orientation="vertical" className="mx-0.5 h-5" />

        <Button
          variant={showing("answers") ? "secondary" : "ghost"}
          size="default"
          onClick={() => void api.invoke("panel:toggle", "answers")}
          className="gap-1.5 px-3 text-[13px]"
          title={t("Answers and chat ({shortcut})", { shortcut: shortcutLabel(chatTrigger) })}
        >
          <MessageSquare className="h-4 w-4" />
          <span>Workspace</span>
        </Button>

        <Button
          variant="ghost"
          size="icon"
          disabled={busy !== null}
          onClick={screenshot}
          aria-label={t("Analyze screen ({shortcut})", { shortcut: shortcutLabel(screenshotTrigger) })}
          title={t("Analyze screen ({shortcut})", { shortcut: shortcutLabel(screenshotTrigger) })}
        >
          {busy === "screenshot" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Camera className="h-4 w-4" />}
        </Button>

        <Button
          variant={showing("settings") ? "secondary" : "ghost"}
          size="icon"
          className="relative"
          onClick={() => void api.invoke("panel:toggle", "settings")}
          aria-label={needsSetup ? `${t("Settings")} — setup required` : t("Settings")}
          title={needsSetup ? `${t("Settings")} (setup required)` : t("Settings")}
        >
          <Settings className="h-4 w-4" />
          {needsSetup && <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-amber-400 ring-2 ring-[#0f131a]" />}
        </Button>

        <Separator orientation="vertical" className="mx-0.5 h-5" />

        <Button
          variant="ghost"
          size="icon"
          className="text-rose-300/85 hover:bg-rose-500/15 hover:text-rose-200"
          onClick={() => void api.invoke("app:quit")}
          aria-label={t("Quit Meetingly")}
          title={t("Quit Meetingly")}
        >
          <Power className="h-4 w-4" />
        </Button>

        <DragHandle />
      </div>

      {bannerText && (
        <div className="mt-1.5 flex max-w-[540px] items-center gap-2 rounded-xl border border-amber-400/30 bg-[#0f131a]/95 px-3 py-2 text-xs text-amber-100 shadow-lg backdrop-blur-xl">
          <span className="min-w-0 flex-1 truncate" title={bannerText}>
            {bannerText}
          </span>
          {needsSetup && (
            <button
              type="button"
              className="shrink-0 rounded-md bg-amber-400/20 px-2 py-1 text-[11.5px] font-semibold text-amber-100 hover:bg-amber-400/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() => {
                setError(null)
                void api.invoke("panel:show", "settings")
              }}
            >
              Open settings
            </button>
          )}
          <button
            type="button"
            className="shrink-0 rounded p-0.5 text-amber-200/75 hover:bg-white/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label="Dismiss"
            onClick={() => setError(null)}
          >
            <Check className="h-3.5 w-3.5" />
          </button>
        </div>
      )}
    </div>
  )
}

function ElapsedText({ startedAt }: { startedAt: number | null }) {
  return <span>{useElapsed(startedAt)}</span>
}

function DragHandle() {
  return (
    <span
      aria-hidden
      title="Drag toolbar"
      className="flex h-8 w-5 shrink-0 cursor-grab items-center justify-center rounded-md text-white/35 transition-colors hover:bg-white/[0.06] hover:text-white/70 active:cursor-grabbing"
    >
      <GripVertical className="pointer-events-none h-4 w-4" />
    </span>
  )
}
