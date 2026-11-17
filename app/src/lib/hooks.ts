import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react"
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

/** Pointer-driven native window drag handler for frameless toolbars across Wayland/X11/macOS/Windows. */
export function useDragToolbar() {
  const drag = useRef<{ x: number; y: number; dx: number; dy: number; frame: number } | null>(null)
  const end = (e: React.PointerEvent<HTMLElement>) => {
    const d = drag.current
    if (!d) return
    cancelAnimationFrame(d.frame)
    drag.current = null
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId)
    void api.invoke("windows:drag", { phase: "move", dx: d.dx, dy: d.dy }).then(() => api.invoke("windows:drag", { phase: "end", dx: 0, dy: 0 }))
  }
  return {
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
      if (e.button !== 0 || (e.target as HTMLElement).closest("button, select, input, a, label")) return
      e.currentTarget.setPointerCapture(e.pointerId)
      drag.current = { x: e.screenX, y: e.screenY, dx: 0, dy: 0, frame: 0 }
      void api.invoke("windows:drag", { phase: "start", dx: 0, dy: 0 })
    },
    onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
      const d = drag.current
      if (!d) return
      d.dx = e.screenX - d.x
      d.dy = e.screenY - d.y
      if (d.frame) return
      d.frame = requestAnimationFrame(() => {
        d.frame = 0
        if (drag.current === d) void api.invoke("windows:drag", { phase: "move", dx: d.dx, dy: d.dy })
      })
    },
    onPointerUp: end,
    onPointerCancel: end
  }
}
