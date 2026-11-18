import type { KeyChord, ShortcutAction, ShortcutStatusView } from "@shared/types"

export interface KeyEventLike {
  key: string
  code: string
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  shiftKey: boolean
  repeat?: boolean
  isComposing?: boolean
}

const EDITING_NAV_CODES = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "Backspace",
  "Delete"
])

const TEXT_EDIT_CODES = new Set(["KeyA", "KeyC", "KeyV", "KeyX", "KeyZ", "KeyY"])
const TEXT_EDIT_KEYS = new Set(["a", "c", "v", "x", "z", "y"])

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || typeof target !== "object") return false
  const el = target as {
    tagName?: string
    isContentEditable?: boolean
    closest?: (selector: string) => unknown
  }
  if (el.isContentEditable) return true
  if (typeof el.closest === "function" && el.closest("[contenteditable='true'], [contenteditable='']")) {
    return true
  }
  const tag = typeof el.tagName === "string" ? el.tagName.toUpperCase() : ""
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT"
}

export function chordMatchesEvent(chord: KeyChord, event: KeyEventLike, macPlatform: boolean): boolean {
  const expectedCtrl = chord.ctrl || (chord.ctrlOrMeta && !macPlatform)
  const expectedMeta = chord.meta || (chord.ctrlOrMeta && macPlatform)
  return (
    event.ctrlKey === expectedCtrl &&
    event.metaKey === expectedMeta &&
    event.altKey === chord.alt &&
    event.shiftKey === chord.shift &&
    event.code === chord.code
  )
}

export function matchInAppShortcut(
  event: KeyEventLike,
  status: ShortcutStatusView | null,
  macPlatform: boolean,
  inEditableTarget: boolean
): ShortcutAction | null {
  if (!status || event.repeat || event.isComposing) return null

  if (inEditableTarget) {
    if (!event.ctrlKey && !event.metaKey && !event.altKey) return null
    if (EDITING_NAV_CODES.has(event.code) || EDITING_NAV_CODES.has(event.key)) return null
    const singlePrimary = !event.altKey && !event.shiftKey && event.ctrlKey !== event.metaKey
    if (singlePrimary && (TEXT_EDIT_CODES.has(event.code) || TEXT_EDIT_KEYS.has(event.key.toLowerCase()))) {
      return null
    }
  }

  for (const item of status.actions) {
    if (item.binding.kind !== "inAppFallback") continue
    if (chordMatchesEvent(item.binding.chord, event, macPlatform)) {
      return item.action
    }
  }
  return null
}

export function actionTrigger(status: ShortcutStatusView | null, action: ShortcutAction, fallback: string): string {
  const item = status?.actions.find((a) => a.action === action)
  if (!item) return fallback
  if (item.binding.kind === "global" || item.binding.kind === "inAppFallback") {
    return item.binding.displayTrigger || fallback
  }
  return fallback
}
