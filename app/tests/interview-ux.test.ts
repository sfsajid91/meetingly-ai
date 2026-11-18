import { describe, expect, it } from "vitest"
import { buildAnswerTurns, computeAnswerScrollTop, selectHeardUtterance } from "../src/lib/turns"
import type { ChatMessage, Utterance } from "../shared/types"

describe("buildAnswerTurns", () => {
  it("pairs user prompts and assistant replies into turns and tracks latest streaming turn", () => {
    const history: ChatMessage[] = [
      { id: "u1", role: "user", kind: "auto", text: "What is Raft?", timestamp: 100 },
      { id: "a1", role: "assistant", kind: "auto", text: "**Raft is consensus.**", timestamp: 200 },
      { id: "u2", role: "user", kind: "text", text: "Give an example", timestamp: 300 }
    ]

    const thinking = buildAnswerTurns(history, null, true)
    expect(thinking).toHaveLength(2)
    expect(thinking[0].prompt).toEqual({ kind: "heard", text: "What is Raft?" })
    expect(thinking[0].isLatest).toBe(false)
    expect(thinking[1].prompt).toEqual({ kind: "asked", text: "Give an example" })
    expect(thinking[1].thinking).toBe(true)
    expect(thinking[1].isLatest).toBe(true)

    const streaming = buildAnswerTurns(history, { id: "a2", text: "**Leader election", kind: "text" }, true)
    expect(streaming[1].thinking).toBe(false)
    expect(streaming[1].streamingText).toBe("**Leader election")
    expect(streaming[1].isLatest).toBe(true)
  })

  it("creates a screen turn when screenshot analysis streams without a user message", () => {
    const turns = buildAnswerTurns([], { id: "s1", text: "**Binary search bug.**", kind: "screenshot" }, true)
    expect(turns).toHaveLength(1)
    expect(turns[0].prompt).toEqual({ kind: "screen" })
    expect(turns[0].streamingText).toBe("**Binary search bug.**")
    expect(turns[0].isLatest).toBe(true)
  })
})

describe("selectHeardUtterance", () => {
  const them: Utterance = { text: "Tell me about Redis?", speaker: null, channel: "them", timestamp: 1 }
  const you: Utterance = { text: "Good question let me think", speaker: null, channel: "you", timestamp: 2 }

  it("preserves interviewer utterance when user speaks filler in both/system modes", () => {
    expect(selectHeardUtterance(them, you, "both")).toEqual(them)
    expect(selectHeardUtterance(them, you, "system")).toEqual(them)
    expect(selectHeardUtterance(them, you, "microphone")).toEqual(you)
    const nextThem: Utterance = { text: "What about persistence?", speaker: null, channel: "them", timestamp: 3 }
    expect(selectHeardUtterance(them, nextThem, "both")).toEqual(nextThem)
  })
})

describe("computeAnswerScrollTop", () => {
  it("pins long streaming answers so their opening bold sentence stays visible", () => {
    expect(
      computeAnswerScrollTop({ scrollTop: 0, clientHeight: 400, scrollHeight: 900, activeTurnTop: 600 }, false)
    ).toBe(500)

    expect(
      computeAnswerScrollTop({ scrollTop: 500, clientHeight: 400, scrollHeight: 1300, activeTurnTop: 600 }, false)
    ).toBe(584)

    expect(
      computeAnswerScrollTop({ scrollTop: 100, clientHeight: 400, scrollHeight: 1300, activeTurnTop: 600 }, true)
    ).toBeNull()
  })
})
