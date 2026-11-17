import { useEffect, useState } from "react"
import type { AudioLevels, Channel, SessionState } from "@shared/types"
import { api } from "@/lib/api"
import { useEvent } from "@/lib/hooks"

const NO_LEVELS: AudioLevels = { microphone: null, system: null }

export interface TranscriptLine {
  text: string
  speaker: number | null
  channel: Channel | null
}

export type Interim = Partial<Record<Channel | "other", string>>

export function useAudioLevels(session: SessionState): AudioLevels {
  const [levels, setLevels] = useState<AudioLevels>(NO_LEVELS)
  useEvent("audio-levels", setLevels)
  useEffect(() => {
    if (session.status === "idle") setLevels(NO_LEVELS)
  }, [session.status])
  return levels
}

export function useLiveTranscript(session: SessionState): { lines: TranscriptLine[]; interim: Interim } {
  const [lines, setLines] = useState<TranscriptLine[]>([])
  const [interim, setInterim] = useState<Interim>({})

  useEffect(() => {
    void api
      .invoke("session:segments")
      .then((segments) => setLines(segments.map((s) => ({ text: s.text, speaker: s.speaker, channel: s.channel ?? null }))))
  }, [])
  useEvent("session:transcript", (e) => {
    const side = e.channel ?? "other"
    if (e.isFinal) {
      setLines((prev) => [...prev, { text: e.text, speaker: e.speaker, channel: e.channel ?? null }])
      setInterim((cur) => ({ ...cur, [side]: "" }))
    } else {
      setInterim((cur) => ({ ...cur, [side]: e.text }))
    }
  })
  useEffect(() => {
    if (session.status === "idle") {
      setLines([])
      setInterim({})
    }
  }, [session.status])
  return { lines, interim }
}
