import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import type { EventChannel, EventMap } from "@shared/ipc"
import type { AudioDevices, Meeting, MigrationOutcome, SessionState, SettingsPatch, SettingsView, SuggestionsState, WindowsState } from "@shared/types"
import { api } from "./api"

export function useEvent<K extends EventChannel>(channel: K, handler: (payload: EventMap[K]) => void): void {
  const ref = useRef(handler)
  ref.current = handler
  useEffect(() => api.on(channel, (payload) => ref.current(payload)), [channel])
}

export function useSettings(): [SettingsView | null, (patch: SettingsPatch) => Promise<void>, (next: SettingsView) => void] {
  const [settings, setSettings] = useState<SettingsView | null>(null)
  useEffect(() => {
    void api.invoke("settings:get").then(setSettings)
  }, [])
  useEvent("settings:changed", setSettings)
  const update = useCallback(async (patch: SettingsPatch) => {
    setSettings(await api.invoke("settings:update", patch))
  }, [])
  return [settings, update, setSettings]
}

const IDLE: SessionState = { status: "idle", startedAt: null, notice: null, connected: false, audioSource: "both", microphoneId: "", monitorId: "" }

export function useSessionState(): SessionState {
  const [state, setState] = useState<SessionState>(IDLE)
  useEffect(() => {
    void api.invoke("session:state").then(setState)
  }, [])
  useEvent("session:state", setState)
  return state
}

export function useWindowsState(): WindowsState {
  const [state, setState] = useState<WindowsState>({ chat: false, panelTab: "answers" })
  useEffect(() => {
    void api.invoke("windows:state").then(setState)
  }, [])
  useEvent("windows:state", setState)
  return state
}

export function useSuggestions(): SuggestionsState {
  const [state, setState] = useState<SuggestionsState>({ topic: "", items: [], updating: false })
  useEffect(() => {
    void api.invoke("suggestions:state").then(setState)
  }, [])
  useEvent("suggestions:state", setState)
  return state
}

export function useAudioDevices(): { devices: AudioDevices | null; loading: boolean; error: string | null; refresh: () => Promise<void> } {
  const [devices, setDevices] = useState<AudioDevices | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setDevices(await api.invoke("audio:devices"))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return { devices, loading, error, refresh }
}

export function useMeetings(): { meetings: Meeting[]; refresh: () => Promise<void> } {
  const [meetings, setMeetings] = useState<Meeting[]>([])
  const refresh = useCallback(async () => {
    setMeetings(await api.invoke("meetings:list"))
  }, [])
  useEffect(() => {
    void refresh()
  }, [refresh])
  useEvent("meetings:changed", () => void refresh())
  return { meetings, refresh }
}

export function useMigrationStatus(): MigrationOutcome | null {
  const [status, setStatus] = useState<MigrationOutcome | null>(null)
  useEffect(() => {
    void api.invoke("migration:status").then(setStatus)
  }, [])
  return status
}

export function useFitWindow<T extends HTMLElement>(extra = { width: 0, height: 0 }): React.RefObject<T | null> {
  const ref = useRef<T | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    let frame = 0
    const fit = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        const rect = el.getBoundingClientRect()
        void api.invoke("windows:fit", {
          width: Math.ceil(rect.width) + extra.width,
          height: Math.ceil(rect.height) + extra.height
        })
      })
    }
    const observer = new ResizeObserver(fit)
    observer.observe(el)
    fit()
    return () => {
      observer.disconnect()
      cancelAnimationFrame(frame)
    }
  }, [extra.width, extra.height])
  return ref
}

export function useElapsed(since: number | null): string {
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    if (!since) return
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [since])
  if (!since) return "00:00"
  const total = Math.max(0, Math.floor((now - since) / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
}

const NO_DRAG_SELECTOR =
  "button, select, input, textarea, a, label, [role='button'], [role='switch'], [role='tab'], [role='combobox'], [data-no-drag]"

/** Native window drag handler using Tauri's startDragging (works on Wayland, X11, macOS, Windows). */
export function useDragToolbar() {
  return {
    onMouseDown: (e: React.MouseEvent<HTMLElement>) => {
      if (e.button !== 0) return
      const target = e.target as HTMLElement | null
      if (target?.closest(NO_DRAG_SELECTOR)) return
      if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return
      e.preventDefault()
      void getCurrentWindow()
        .startDragging()
        .catch(() => {})
    }
  }
}
