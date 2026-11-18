import { describe, expect, it } from "vitest"
import { DEFAULT_SHORTCUTS } from "../shared/types"
import {
  CHAT_MESSAGE_MAX_LENGTH,
  getSessionReadiness,
  isLocalEndpoint,
  keyEventToAccelerator,
  validateAccelerator,
  validateChatMessage,
  validatePromptInput,
  validateProviderInput,
  validateShortcuts
} from "../src/lib/validation"

describe("validateProviderInput", () => {
  it("validates Deepgram endpoints and required API keys", () => {
    const missingKey = validateProviderInput({
      kind: "deepgram",
      label: "Deepgram",
      endpoint: "wss://api.deepgram.com/v1/listen",
      model: "nova-3",
      apiKey: "",
      clearKey: false,
      hasExistingKey: false
    })
    expect(missingKey.valid).toBe(false)
    expect(missingKey.errors.apiKey).toBeTruthy()

    const badScheme = validateProviderInput({
      kind: "deepgram",
      label: "Deepgram",
      endpoint: "https://api.deepgram.com/v1/listen",
      model: "nova-3",
      apiKey: "dg_123",
      clearKey: false,
      hasExistingKey: false
    })
    expect(badScheme.valid).toBe(false)
    expect(badScheme.errors.endpoint).toContain("wss://")

    const validEditWithExistingKey = validateProviderInput({
      kind: "deepgram",
      label: "Deepgram",
      endpoint: "wss://api.deepgram.com/v1/listen",
      model: "nova-3",
      apiKey: "",
      clearKey: false,
      hasExistingKey: true
    })
    expect(validEditWithExistingKey.valid).toBe(true)
  })

  it("enforces https:// or localhost http:// for OpenAI-compatible providers", () => {
    expect(isLocalEndpoint("http://localhost:11434/v1")).toBe(true)
    expect(isLocalEndpoint("http://127.0.0.1:8080/v1")).toBe(true)
    expect(isLocalEndpoint("http://example.com/v1")).toBe(false)

    const remoteHttp = validateProviderInput({
      kind: "openai-compatible",
      label: "Remote",
      endpoint: "http://example.com/v1",
      model: "gpt-4.1-mini",
      apiKey: "sk-123",
      clearKey: false,
      hasExistingKey: false
    })
    expect(remoteHttp.valid).toBe(false)
    expect(remoteHttp.errors.endpoint).toContain("localhost")

    const localOllama = validateProviderInput({
      kind: "openai-compatible",
      label: "Ollama",
      endpoint: "http://localhost:11434/v1",
      model: "llama3.2",
      apiKey: "",
      clearKey: false,
      hasExistingKey: false
    })
    expect(localOllama.valid).toBe(true)
    expect(localOllama.warning).toBeUndefined()

    const hostedNoKey = validateProviderInput({
      kind: "openai-compatible",
      label: "OpenAI",
      endpoint: "https://api.openai.com/v1",
      model: "gpt-4.1-mini",
      apiKey: "",
      clearKey: false,
      hasExistingKey: false
    })
    expect(hostedNoKey.valid).toBe(true)
    expect(hostedNoKey.warning).toBeTruthy()
  })
})

describe("validatePromptInput", () => {
  it("requires non-empty title and system instructions", () => {
    const empty = validatePromptInput({ title: "  ", content: "", notes: "" })
    expect(empty.valid).toBe(false)
    expect(empty.errors.title).toBeTruthy()
    expect(empty.errors.content).toBeTruthy()

    const ok = validatePromptInput({ title: "Interview", content: "Be concise.", notes: "Staff engineer" })
    expect(ok.valid).toBe(true)
  })
})

describe("validateShortcuts & keyEventToAccelerator", () => {
  it("accepts default shortcuts and rejects duplicates or invalid syntax", () => {
    expect(validateShortcuts(DEFAULT_SHORTCUTS).valid).toBe(true)
    expect(validateAccelerator("L")).toContain("modifier")
    expect(validateAccelerator("CommandOrControl+Shift+L")).toBeNull()

    const dup = validateShortcuts({
      ...DEFAULT_SHORTCUTS,
      listen: "CmdOrCtrl+B"
    })
    expect(dup.valid).toBe(false)
    expect(dup.errors.listen).toContain("Duplicates")
    expect(dup.errors.toggleAll).toContain("Duplicates")

    expect(validateAccelerator("AltGr+L")).toContain("Unknown modifier")
    expect(validateAccelerator("CommandOrControl+MediaTrackNext")).toBeNull()
    expect(validateAccelerator("CommandOrControl+MediaTrackPrevious")).toBeNull()
  })

  it("converts keyboard events to Tauri accelerators", () => {
    expect(
      keyEventToAccelerator({ key: "l", code: "KeyL", ctrlKey: true, metaKey: false, altKey: false, shiftKey: true })
    ).toBe("CommandOrControl+Shift+L")
    expect(
      keyEventToAccelerator({ key: "Control", code: "ControlLeft", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false })
    ).toBeNull()
  })
})

describe("validateChatMessage & getSessionReadiness", () => {
  it("validates chat length and empty input", () => {
    expect(validateChatMessage("   ").valid).toBe(false)
    expect(validateChatMessage("Hello").valid).toBe(true)
    const tooLong = validateChatMessage("a".repeat(CHAT_MESSAGE_MAX_LENGTH + 1))
    expect(tooLong.valid).toBe(false)
    expect(tooLong.error).toContain("too long")
  })

  it("reports missing transcriber and disconnected devices", () => {
    const readiness = getSessionReadiness(
      {
        transcriberId: null,
        brainId: "b1",
        audioSource: "both",
        microphoneId: "missing-mic",
        monitorId: ""
      },
      { microphones: [{ id: "mic-1", label: "Built-in Mic" }], monitors: [{ id: "mon-1", label: "Monitor" }] },
      false
    )
    expect(readiness.canStart).toBe(false)
    expect(readiness.hasTranscriber).toBe(false)
    expect(readiness.micConnected).toBe(false)
    expect(readiness.monitorReady).toBe(false)
    expect(readiness.missingReasons.length).toBe(3)
  })
})
