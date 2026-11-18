import { describe, expect, it } from "vitest"
import type { ShortcutStatusView } from "../shared/types"
import { actionTrigger, isEditableTarget, matchInAppShortcut } from "../src/lib/in-app-shortcuts"

function makeStatus(overrides?: Partial<ShortcutStatusView>): ShortcutStatusView {
  return {
    authority: { kind: "desktopPortal", session: "needsApproval", canConfigure: true },
    actions: [
      {
        action: "showToolbar",
        configured: "CommandOrControl+Space",
        binding: {
          kind: "inAppFallback",
          chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: false, code: "Space" },
          displayTrigger: "CommandOrControl+Space",
          reason: "portalNotBound"
        }
      },
      {
        action: "toggleAll",
        configured: "CommandOrControl+B",
        binding: {
          kind: "inAppFallback",
          chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: false, code: "KeyB" },
          displayTrigger: "CommandOrControl+B",
          reason: "portalNotBound"
        }
      },
      {
        action: "listen",
        configured: "CommandOrControl+L",
        binding: {
          kind: "inAppFallback",
          chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: false, code: "KeyL" },
          displayTrigger: "CommandOrControl+L",
          reason: "portalNotBound"
        }
      },
      {
        action: "chat",
        configured: "CommandOrControl+Shift+C",
        binding: {
          kind: "inAppFallback",
          chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: true, code: "KeyC" },
          displayTrigger: "CommandOrControl+Shift+C",
          reason: "portalNotBound"
        }
      },
      {
        action: "screenshot",
        configured: "CommandOrControl+Enter",
        binding: {
          kind: "inAppFallback",
          chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: false, code: "Enter" },
          displayTrigger: "CommandOrControl+Enter",
          reason: "portalNotBound"
        }
      },
      {
        action: "moveLeft",
        configured: "CommandOrControl+Left",
        binding: { kind: "unsupported", reason: "noPositioningOnWayland" }
      }
    ],
    ...overrides
  }
}

function key(
  partial: Partial<{
    key: string
    code: string
    ctrlKey: boolean
    metaKey: boolean
    altKey: boolean
    shiftKey: boolean
    repeat: boolean
    isComposing: boolean
  }>
) {
  return {
    key: "",
    code: "",
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...partial
  }
}

describe("matchInAppShortcut", () => {
  it("matches inAppFallback chords under normal focus on Linux and macOS", () => {
    const status = makeStatus()
    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", ctrlKey: true }), status, false, false)).toBe("listen")
    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", metaKey: true }), status, true, false)).toBe("listen")
    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", ctrlKey: true }), status, true, false)).toBeNull()
    expect(matchInAppShortcut(key({ key: "C", code: "KeyC", ctrlKey: true, shiftKey: true }), status, false, false)).toBe("chat")
    expect(matchInAppShortcut(key({ key: "Enter", code: "Enter", ctrlKey: true }), status, false, false)).toBe("screenshot")
    expect(matchInAppShortcut(key({ key: " ", code: "Space", ctrlKey: true }), status, false, false)).toBe("showToolbar")
  })

  it("ignores repeat and composing events", () => {
    const status = makeStatus()
    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", ctrlKey: true, repeat: true }), status, false, false)).toBeNull()
    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", ctrlKey: true, isComposing: true }), status, false, false)).toBeNull()
  })

  it("protects editable inputs while allowing dedicated app chords", () => {
    const status = makeStatus({
      actions: [
        ...makeStatus().actions,
        {
          action: "moveRight",
          configured: "CommandOrControl+C",
          binding: {
            kind: "inAppFallback",
            chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: false, code: "KeyC" },
            displayTrigger: "CommandOrControl+C",
            reason: "portalNotBound"
          }
        },
        {
          action: "moveUp",
          configured: "CommandOrControl+ArrowLeft",
          binding: {
            kind: "inAppFallback",
            chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: false, code: "ArrowLeft" },
            displayTrigger: "CommandOrControl+ArrowLeft",
            reason: "portalNotBound"
          }
        }
      ]
    })

    expect(matchInAppShortcut(key({ key: "Enter", code: "Enter" }), status, false, true)).toBeNull()
    expect(matchInAppShortcut(key({ key: "c", code: "KeyC", ctrlKey: true }), status, false, true)).toBeNull()
    expect(matchInAppShortcut(key({ key: "c", code: "KeyC", metaKey: true }), status, true, true)).toBeNull()
    expect(matchInAppShortcut(key({ key: "ArrowLeft", code: "ArrowLeft", ctrlKey: true }), status, false, true)).toBeNull()
    expect(matchInAppShortcut(key({ key: "Backspace", code: "Backspace", ctrlKey: true }), status, false, true)).toBeNull()

    expect(matchInAppShortcut(key({ key: "Enter", code: "Enter", ctrlKey: true }), status, false, true)).toBe("screenshot")
    expect(matchInAppShortcut(key({ key: "b", code: "KeyB", ctrlKey: true }), status, false, true)).toBe("toggleAll")
    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", ctrlKey: true }), status, false, true)).toBe("listen")
    expect(matchInAppShortcut(key({ key: "C", code: "KeyC", ctrlKey: true, shiftKey: true }), status, false, true)).toBe("chat")
    expect(matchInAppShortcut(key({ key: " ", code: "Space", ctrlKey: true }), status, false, true)).toBe("showToolbar")
  })

  it("suppresses matching for Global and Unsupported bindings", () => {
    const status = makeStatus({
      authority: { kind: "desktopPortal", session: "bound", canConfigure: true },
      actions: [
        {
          action: "listen",
          configured: "CommandOrControl+L",
          binding: { kind: "global", displayTrigger: "Ctrl+L" }
        },
        {
          action: "chat",
          configured: "CommandOrControl+Shift+C",
          binding: {
            kind: "inAppFallback",
            chord: { ctrlOrMeta: true, ctrl: false, meta: false, alt: false, shift: true, code: "KeyC" },
            displayTrigger: "CommandOrControl+Shift+C",
            reason: "portalNotBound"
          }
        },
        {
          action: "moveLeft",
          configured: "CommandOrControl+Left",
          binding: { kind: "unsupported", reason: "noPositioningOnWayland" }
        }
      ]
    })

    expect(matchInAppShortcut(key({ key: "l", code: "KeyL", ctrlKey: true }), status, false, false)).toBeNull()
    expect(matchInAppShortcut(key({ key: "ArrowLeft", code: "ArrowLeft", ctrlKey: true }), status, false, false)).toBeNull()
    expect(matchInAppShortcut(key({ key: "C", code: "KeyC", ctrlKey: true, shiftKey: true }), status, false, false)).toBe("chat")
    expect(actionTrigger(status, "listen", "CommandOrControl+L")).toBe("Ctrl+L")
    expect(actionTrigger(status, "moveLeft", "CommandOrControl+Left")).toBe("CommandOrControl+Left")
  })

  it("detects editable targets", () => {
    expect(isEditableTarget({ tagName: "TEXTAREA" } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: "INPUT" } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: "SELECT" } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: "DIV", isContentEditable: true } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: "DIV", isContentEditable: false } as unknown as EventTarget)).toBe(false)
    expect(isEditableTarget(null)).toBe(false)
  })
})
