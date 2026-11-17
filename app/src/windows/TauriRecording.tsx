import { useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { Mic, Square } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Select } from "@/components/ui/fields"

type Device = { id: string; label: string }
type Devices = { microphones: Device[]; monitors: Device[] }
type RecordingPaths = { microphone_path: string; denoised_path: string; system_path: string }
type Phase = "idle" | "starting" | "recording" | "stopping"

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function TauriRecording() {
  const [eventReady, setEventReady] = useState(false)
  const [devices, setDevices] = useState<Devices | null>(null)
  const [loading, setLoading] = useState(true)
  const [deviceError, setDeviceError] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [microphone, setMicrophone] = useState("")
  const [monitor, setMonitor] = useState("")
  const [phase, setPhase] = useState<Phase>("idle")
  const [paths, setPaths] = useState<RecordingPaths | null>(null)
  const mounted = useRef(false)
  const operation = useRef<Phase>("idle")
  const captureError = useRef<string | null>(null)
  const request = useRef(0)

  async function loadDevices() {
    const current = ++request.current
    setLoading(true)
    setDeviceError(null)
    try {
      const result = await invoke<Devices>("audio_devices")
      if (!mounted.current || current !== request.current) return
      setDevices(result)
      setMicrophone((value) => result.microphones.some((device) => device.id === value) ? value : "")
      setMonitor((value) => result.monitors.some((device) => device.id === value) ? value : "")
    } catch (cause) {
      if (mounted.current && current === request.current) setDeviceError(errorMessage(cause))
    } finally {
      if (mounted.current && current === request.current) setLoading(false)
    }
  }

  useEffect(() => {
    let active = true
    let unlisten: (() => void) | undefined
    mounted.current = true
    void loadDevices()
    void listen<string>("audio-error", (event) => {
      if (!active) return
      captureError.current = event.payload
      setError(event.payload)
      if (operation.current === "recording") void stop()
    }).then((dispose) => {
      if (!active) dispose()
      else {
        unlisten = dispose
        setEventReady(true)
      }
    }).catch((cause) => {
      if (active) setError(`Could not listen for recording errors: ${errorMessage(cause)}`)
    })
    return () => {
      active = false
      mounted.current = false
      request.current++
      unlisten?.()
      if (operation.current === "recording") {
        operation.current = "stopping"
        void invoke<RecordingPaths>("audio_stop").catch(() => {})
      }
    }
  }, [])

  async function start() {
    if (operation.current !== "idle" || !microphone || !monitor) return
    operation.current = "starting"
    setPhase("starting")
    setError(null)
    setPaths(null)
    captureError.current = null
    try {
      await invoke("audio_start", { microphone, monitor })
      if (!mounted.current) {
        operation.current = "stopping"
        await invoke<RecordingPaths>("audio_stop")
        operation.current = "idle"
        return
      }
      operation.current = "recording"
      setPhase("recording")
      if (captureError.current) {
        await stop()
        return
      }
    } catch (cause) {
      operation.current = "idle"
      if (mounted.current) {
        setError(errorMessage(cause))
        setPhase("idle")
      }
    }
  }

  async function stop() {
    if (operation.current !== "recording") return
    operation.current = "stopping"
    setPhase("stopping")
    if (!captureError.current) setError(null)
    try {
      const result = await invoke<RecordingPaths>("audio_stop")
      operation.current = "idle"
      if (mounted.current) {
        setPaths(result)
        setPhase("idle")
      }
    } catch (cause) {
      operation.current = "idle"
      if (mounted.current) {
        setError(errorMessage(cause))
        setPhase("idle")
      }
    }
  }

  const busy = phase !== "idle"
  const unavailable = loading || !!deviceError || !devices || !eventReady

  return (
    <main className="h-screen overflow-y-auto bg-black/95 px-4 py-8 text-white sm:px-6">
      <div className="mx-auto max-w-xl space-y-6">
        <header>
          <p className="text-xs font-medium uppercase tracking-widest text-white/50">Meetingly</p>
          <h1 className="mt-2 text-2xl font-semibold tracking-tight">Audio recording</h1>
          <p className="mt-2 text-sm leading-relaxed text-white/65">Choose a microphone and a system monitor to record both sources.</p>
        </header>

        <section aria-labelledby="sources-heading" className="space-y-4 rounded-xl border border-white/15 bg-white/5 p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h2 id="sources-heading" className="text-sm font-semibold">Audio sources</h2>
            <Button variant="ghost" size="sm" onClick={() => void loadDevices()} disabled={busy || loading}>Refresh devices</Button>
          </div>
          {loading && <p role="status" className="text-sm text-white/65">Loading audio devices…</p>}
          {deviceError && <p role="alert" className="break-words text-sm text-rose-300">Could not load audio devices: {deviceError}</p>}
          {!loading && !deviceError && devices && (
            <div className="space-y-4">
              <div>
                <label htmlFor="microphone" className="mb-2 block text-sm font-medium">Microphone</label>
                <Select id="microphone" value={microphone} onChange={(event) => setMicrophone(event.target.value)} disabled={busy || devices.microphones.length === 0}>
                  <option value="">{devices.microphones.length ? "Select a microphone" : "No microphones found"}</option>
                  {devices.microphones.map((device) => <option key={device.id} value={device.id}>{device.label}</option>)}
                </Select>
              </div>
              <div>
                <label htmlFor="monitor" className="mb-2 block text-sm font-medium">System monitor</label>
                <Select id="monitor" value={monitor} onChange={(event) => setMonitor(event.target.value)} disabled={busy || devices.monitors.length === 0}>
                  <option value="">{devices.monitors.length ? "Select a system monitor" : "No system monitors found"}</option>
                  {devices.monitors.map((device) => <option key={device.id} value={device.id}>{device.label}</option>)}
                </Select>
              </div>
              {(!devices.microphones.length || !devices.monitors.length) && <p className="text-sm leading-relaxed text-white/65">Connect the missing source, then refresh devices.</p>}
            </div>
          )}
        </section>

        <div className="space-y-3">
          {phase === "recording" || phase === "stopping" ? (
            <Button variant="danger" size="lg" onClick={() => void stop()} disabled={phase === "stopping"}>
              <Square size={15} aria-hidden="true" />{phase === "stopping" ? "Stopping…" : "Stop recording"}
            </Button>
          ) : (
            <Button variant="primary" size="lg" onClick={() => void start()} disabled={unavailable || busy || !microphone || !monitor}>
              <Mic size={16} aria-hidden="true" />{phase === "starting" ? "Starting…" : "Start recording"}
            </Button>
          )}
          <p role="status" className="text-sm text-white/65">{phase === "recording" ? "Recording in progress" : phase === "starting" ? "Starting recording" : phase === "stopping" ? "Saving recordings" : paths ? "Recording saved" : "Not recording"}</p>
          {error && <p role="alert" className="break-words text-sm text-rose-300">{error}</p>}
        </div>

        <p className="rounded-lg border border-white/10 bg-white/5 p-4 text-sm leading-relaxed text-white/70">The microphone and system audio are saved as separate tracks. They are not synchronized. Stop before 10 minutes, or the recording will be discarded.</p>

        {paths && (
          <section aria-labelledby="saved-heading" className="space-y-3 rounded-xl border border-white/15 bg-white/5 p-5">
            <h2 id="saved-heading" className="text-sm font-semibold">Saved recordings</h2>
            <dl className="selectable space-y-3 text-sm">
              {([ ["Microphone", paths.microphone_path], ["Denoised microphone", paths.denoised_path], ["System audio", paths.system_path] ] as const).map(([label, path]) => (
                <div key={label}>
                  <dt className="text-white/55">{label}</dt>
                  <dd className="mt-1 break-all text-white/90">{path}</dd>
                </div>
              ))}
            </dl>
          </section>
        )}
      </div>
    </main>
  )
}
