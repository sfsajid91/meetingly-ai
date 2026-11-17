import { describe, expect, it } from "vitest"
import { markdownToPlain } from "../src/lib/plain-text"
import { shortcutLabel } from "../src/lib/api"
import {
  BRAIN_PRESETS,
  DEFAULT_SHORTCUTS,
  LANGUAGES,
  TRANSCRIBER_PRESETS,
  getLanguage,
  hostOf,
  segmentLabel,
  suggestionsVisible
} from "../shared/types"

describe("markdownToPlain", () => {
  it("drops markdown markers and keeps content", () => {
    const answer = "**Kafka is a distributed event-streaming platform.**\n- Producers write to `topics`\n* Consumers *read* at their own pace\n## Notes\nSee [docs](https://kafka.apache.org)"
    expect(markdownToPlain(answer)).toBe(
      "Kafka is a distributed event-streaming platform.\n- Producers write to topics\n- Consumers read at their own pace\nNotes\nSee docs (https://kafka.apache.org)"
    )
  })

  it("keeps code blocks without fences and preserves multiplication", () => {
    expect(markdownToPlain("**Use a map.**\n```js\nconst a = { b: 2 * 3 }\n```")).toBe("Use a map.\nconst a = { b: 2 * 3 }")
    expect(markdownToPlain("2 * 3 * 4 and user_id_field")).toBe("2 * 3 * 4 and user_id_field")
  })
})

describe("shared BYOK types and helpers", () => {
  it("resolves languages with English fallback", () => {
    expect(getLanguage("ru").code).toBe("ru")
    expect(getLanguage("unknown").code).toBe("en")
    expect(LANGUAGES.length).toBeGreaterThanOrEqual(28)
  })

  it("formats segment labels for channels and diarized speakers", () => {
    expect(segmentLabel({ speaker: 0, channel: "you" })).toBe("You")
    expect(segmentLabel({ speaker: 1, channel: "them" })).toBe("Them")
    expect(segmentLabel({ speaker: 1, channel: null })).toBe("Speaker 2")
    expect(segmentLabel({ speaker: null, channel: null })).toBe("")
  })

  it("shows suggestions rail only during active session when enabled", () => {
    expect(suggestionsVisible({ suggestions: true }, "idle")).toBe(false)
    expect(suggestionsVisible({ suggestions: false }, "recording")).toBe(false)
    expect(suggestionsVisible({ suggestions: true }, "recording")).toBe(true)
  })

  it("includes Gemini AI Studio, OpenAI, OpenRouter, Ollama, and Deepgram presets", () => {
    expect(BRAIN_PRESETS.some((p) => p.endpoint.includes("generativelanguage.googleapis.com"))).toBe(true)
    expect(TRANSCRIBER_PRESETS[0].kind).toBe("deepgram")
    expect(hostOf("https://api.openai.com/v1")).toBe("api.openai.com")
    expect(DEFAULT_SHORTCUTS.listen).toBe("CommandOrControl+L")
    expect(shortcutLabel("CommandOrControl+Shift+C")).toContain("Shift")
  })
})
