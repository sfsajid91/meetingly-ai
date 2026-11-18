import type { AudioSource, ChatChunk, ChatKind, ChatMessage, Utterance } from "@shared/types"

export type PromptOrigin =
  | { kind: "heard"; text: string }
  | { kind: "asked"; text: string }
  | { kind: "screen" }

export interface AnswerTurn {
  id: string
  prompt: PromptOrigin | null
  response: ChatMessage | null
  streamingText: string | null
  streamingKind: ChatKind | null
  thinking: boolean
  timestamp: number
  isLatest: boolean
}

export function buildAnswerTurns(history: ChatMessage[], stream: ChatChunk | null, busy: boolean): AnswerTurn[] {
  const turns: AnswerTurn[] = []

  for (const msg of history) {
    if (msg.role === "user") {
      turns.push({
        id: msg.id,
        prompt: msg.kind === "auto" ? { kind: "heard", text: msg.text } : { kind: "asked", text: msg.text },
        response: null,
        streamingText: null,
        streamingKind: null,
        thinking: false,
        timestamp: msg.timestamp,
        isLatest: false
      })
      continue
    }

    const last = turns[turns.length - 1]
    if (last && last.response === null) {
      last.response = msg
      if (msg.kind === "screenshot" && last.prompt === null) {
        last.prompt = { kind: "screen" }
      }
    } else {
      turns.push({
        id: msg.id,
        prompt: msg.kind === "screenshot" ? { kind: "screen" } : null,
        response: msg,
        streamingText: null,
        streamingKind: null,
        thinking: false,
        timestamp: msg.timestamp,
        isLatest: false
      })
    }
  }

  if (stream !== null || busy) {
    const last = turns[turns.length - 1]
    if (last && last.response === null) {
      last.streamingText = stream?.text ?? null
      last.streamingKind = stream?.kind ?? null
      last.thinking = stream === null
    } else {
      turns.push({
        id: stream?.id ?? "__thinking__",
        prompt: stream?.kind === "screenshot" ? { kind: "screen" } : null,
        response: null,
        streamingText: stream?.text ?? null,
        streamingKind: stream?.kind ?? null,
        thinking: stream === null,
        timestamp: Date.now(),
        isLatest: false
      })
    }
  }

  for (let i = turns.length - 1; i >= 0; i--) {
    const t = turns[i]
    if (t.response !== null || t.streamingText !== null || t.thinking) {
      t.isLatest = true
      break
    }
  }

  return turns
}

export function selectHeardUtterance(current: Utterance | null, incoming: Utterance, audioSource: AudioSource): Utterance {
  if (audioSource !== "microphone" && incoming.channel === "you" && current?.channel === "them") {
    return current
  }
  return incoming
}

export function computeAnswerScrollTop(
  metrics: { scrollTop: number; clientHeight: number; scrollHeight: number; activeTurnTop: number | null },
  userScrolledUp: boolean
): number | null {
  if (userScrolledUp) return null
  const bottomScroll = Math.max(0, metrics.scrollHeight - metrics.clientHeight)
  if (metrics.activeTurnTop === null) return bottomScroll
  const maxAllowedToKeepLeadVisible = Math.max(0, metrics.activeTurnTop - 16)
  return Math.min(bottomScroll, maxAllowedToKeepLeadVisible)
}
