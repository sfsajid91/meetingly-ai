import { useEffect, useState } from "react"
import { Check, FileText, FolderOpen, KeyRound, Plus, RefreshCw, Sparkles, Trash2 } from "lucide-react"
import {
  BRAIN_PRESETS,
  DEFAULT_SHORTCUTS,
  SHORTCUT_LABELS,
  TRANSCRIBER_PRESETS,
  hostOf,
  type Meeting,
  type MigrationOutcome,
  type Prompt,
  type ProviderKind,
  type ProviderTestResult,
  type ProviderView,
  type SettingsPatch,
  type SettingsView,
  type ShortcutAction,
  type Shortcuts
} from "@shared/types"
import { api, errorText } from "@/lib/api"
import { formatDate, formatDuration } from "@/lib/format"
import { useMeetings, useMigrationStatus } from "@/lib/hooks"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

const INPUT_CLS =
  "h-7 w-full min-w-0 rounded border border-white/[0.14] bg-black/30 px-2 text-[11.5px] text-white placeholder:text-white/30 focus:border-white/35 focus:outline-none"
const TEXTAREA_CLS =
  "w-full min-w-0 rounded border border-white/[0.14] bg-black/30 px-2 py-1.5 text-[11.5px] leading-relaxed text-white placeholder:text-white/30 focus:border-white/35 focus:outline-none"

export function ProvidersSettings({
  settings,
  onSettingsChange
}: {
  settings: SettingsView
  onSettingsChange: (next: SettingsView) => void
}) {
  const [editing, setEditing] = useState<ProviderView | null>(null)
  const [addingKind, setAddingKind] = useState<ProviderKind | null>(null)
  const [label, setLabel] = useState("")
  const [endpoint, setEndpoint] = useState("")
  const [model, setModel] = useState("")
  const [apiKey, setApiKey] = useState("")
  const [clearKey, setClearKey] = useState(false)
  const [models, setModels] = useState<string[]>([])
  const [modelsBusy, setModelsBusy] = useState(false)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const formKind = editing?.kind ?? addingKind

  function openNew(kind: ProviderKind) {
    setEditing(null)
    setAddingKind(kind)
    setTestResult(null)
    setError(null)
    setModels([])
    setApiKey("")
    setClearKey(false)
    if (kind === "deepgram") {
      const p = TRANSCRIBER_PRESETS[0]
      setLabel(p.label)
      setEndpoint(p.endpoint)
      setModel(p.model)
    } else {
      const p = BRAIN_PRESETS[0]
      setLabel(p.label)
      setEndpoint(p.endpoint)
      setModel(p.model)
    }
  }

  function openEdit(provider: ProviderView) {
    setAddingKind(null)
    setEditing(provider)
    setLabel(provider.label)
    setEndpoint(provider.endpoint)
    setModel(provider.model)
    setApiKey("")
    setClearKey(false)
    setModels([])
    setTestResult(null)
    setError(null)
  }

  function closeForm() {
    setEditing(null)
    setAddingKind(null)
    setError(null)
    setTestResult(null)
  }

  async function loadModels() {
    if (!endpoint.trim() || formKind !== "openai-compatible") return
    setModelsBusy(true)
    setError(null)
    try {
      const list = await api.invoke("provider:models", { id: editing?.id, endpoint: endpoint.trim(), key: apiKey.trim() || undefined })
      setModels(list)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setModelsBusy(false)
    }
  }

  async function testDraft() {
    if (!formKind) return
    setTestingId("draft")
    setTestResult(null)
    setError(null)
    try {
      const res = await api.invoke("provider:test", {
        id: editing?.id,
        kind: formKind,
        endpoint: endpoint.trim(),
        model: model.trim(),
        key: apiKey.trim() || undefined
      })
      setTestResult(res)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setTestingId(null)
    }
  }

  async function testSaved(p: ProviderView) {
    setTestingId(p.id)
    setError(null)
    try {
      const res = await api.invoke("provider:test", {
        id: p.id,
        kind: p.kind,
        endpoint: p.endpoint,
        model: p.model
      })
      setTestResult(res)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setTestingId(null)
    }
  }

  async function save(e: React.FormEvent) {
    e.preventDefault()
    if (!formKind) return
    setSaving(true)
    setError(null)
    try {
      const next = await api.invoke("provider:save", {
        id: editing?.id,
        kind: formKind,
        label: label.trim(),
        endpoint: endpoint.trim(),
        model: model.trim(),
        key: apiKey.trim() || undefined,
        clearKey
      })
      onSettingsChange(next)
      closeForm()
    } catch (err) {
      setError(errorText(err))
    } finally {
      setSaving(false)
    }
  }

  async function select(p: ProviderView) {
    setError(null)
    try {
      onSettingsChange(await api.invoke("provider:select", p.role, p.id))
    } catch (err) {
      setError(errorText(err))
    }
  }

  async function remove(id: string) {
    setError(null)
    try {
      onSettingsChange(await api.invoke("provider:remove", id))
      if (editing?.id === id) closeForm()
    } catch (err) {
      setError(errorText(err))
    }
  }

  const transcribers = settings.providers.filter((p) => p.role === "transcription")
  const brains = settings.providers.filter((p) => p.role === "brain")

  return (
    <div className="space-y-3 py-2 text-[12px]">
      <div className="rounded-lg border border-sky-400/20 bg-sky-500/10 px-2.5 py-2 text-[11px] leading-snug text-sky-100/90">
        Audio and transcript text are sent directly to your configured cloud providers and may incur usage charges. API keys are stored in your OS keychain and never leave Rust.
      </div>

      <div className="flex flex-wrap gap-1.5">
        <Button size="sm" variant="secondary" onClick={() => openNew("deepgram")}>
          <Plus className="h-3 w-3" /> Add transcriber
        </Button>
        <Button size="sm" variant="secondary" onClick={() => openNew("openai-compatible")}>
          <Plus className="h-3 w-3" /> Add brain
        </Button>
      </div>

      {error && <p className="selectable rounded bg-rose-500/15 px-2.5 py-1.5 text-[11px] text-rose-200">{error}</p>}

      {testResult && (
        <div
          className={cn(
            "rounded border px-2.5 py-1.5 text-[11px]",
            testResult.ok ? "border-emerald-400/25 bg-emerald-500/10 text-emerald-100" : "border-rose-400/25 bg-rose-500/10 text-rose-200"
          )}
        >
          <div>{testResult.detail}</div>
          <div className="mt-1 flex gap-2 text-[10.5px] opacity-80">
            {testResult.streaming !== null && <span>Streaming: {testResult.streaming ? "supported" : "failed"}</span>}
            {testResult.vision !== null && <span>Images: {testResult.vision ? "supported" : "unsupported"}</span>}
          </div>
        </div>
      )}

      {formKind && (
        <form onSubmit={(e) => void save(e)} className="space-y-2 rounded-lg border border-white/15 bg-white/[0.04] p-2.5">
          <div className="flex items-center justify-between">
            <span className="font-medium text-white">
              {editing ? `Edit ${editing.label}` : formKind === "deepgram" ? "New Deepgram transcriber" : "New OpenAI-compatible brain"}
            </span>
            {!editing && formKind === "openai-compatible" && (
              <div className="flex flex-wrap gap-1">
                {BRAIN_PRESETS.map((p) => (
                  <button
                    key={p.label}
                    type="button"
                    className={cn("rounded px-1.5 py-0.5 text-[10.5px]", endpoint === p.endpoint ? "bg-white/20 text-white" : "text-white/55 hover:bg-white/10")}
                    onClick={() => {
                      setLabel(p.label)
                      setEndpoint(p.endpoint)
                      setModel(p.model)
                      setModels([])
                    }}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="grid grid-cols-[76px_1fr] items-center gap-x-2 gap-y-1.5 text-[11px]">
            <label className="text-white/55">Label</label>
            <input className={INPUT_CLS} value={label} onChange={(e) => setLabel(e.target.value)} placeholder="My provider" required />

            <label className="text-white/55">Endpoint</label>
            <input
              className={INPUT_CLS}
              value={endpoint}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder={formKind === "deepgram" ? "wss://api.deepgram.com/v1/listen" : "https://api.openai.com/v1"}
              spellCheck={false}
              required
            />

            <label className="text-white/55">API key</label>
            <input
              type="password"
              className={INPUT_CLS}
              value={apiKey}
              onChange={(e) => {
                setApiKey(e.target.value)
                if (e.target.value) setClearKey(false)
              }}
              placeholder={editing?.hasKey ? "Saved in keychain. Leave empty to keep." : formKind === "deepgram" ? "Deepgram API key" : "Optional for localhost"}
              autoComplete="off"
            />

            <label className="text-white/55">Model</label>
            <div className="flex gap-1">
              <input
                className={INPUT_CLS}
                value={model}
                onChange={(e) => setModel(e.target.value)}
                list="provider-models-list"
                placeholder={formKind === "deepgram" ? "nova-3" : "gpt-4.1-mini"}
                spellCheck={false}
                required
              />
              {formKind === "openai-compatible" && (
                <Button type="button" size="sm" variant="ghost" className="h-7 shrink-0 px-2 text-[11px]" onClick={() => void loadModels()} disabled={modelsBusy}>
                  <RefreshCw className={cn("h-3 w-3", modelsBusy && "animate-spin")} />
                </Button>
              )}
              <datalist id="provider-models-list">
                {models.map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            </div>
          </div>

          {editing?.hasKey && formKind === "openai-compatible" && (
            <label className="flex items-center gap-1.5 text-[11px] text-white/60">
              <input type="checkbox" checked={clearKey} onChange={(e) => setClearKey(e.target.checked)} />
              Remove saved key from keychain
            </label>
          )}

          <div className="flex items-center justify-end gap-1.5 pt-1">
            <Button type="button" size="sm" variant="ghost" onClick={closeForm}>
              Cancel
            </Button>
            <Button type="button" size="sm" variant="secondary" disabled={testingId !== null || !endpoint.trim() || !model.trim()} onClick={() => void testDraft()}>
              {testingId === "draft" ? "Testing…" : "Test"}
            </Button>
            <Button type="submit" size="sm" variant="primary" disabled={saving || !label.trim() || !endpoint.trim() || !model.trim()}>
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </form>
      )}

      <ProviderGroup
        title="Transcription"
        empty="No transcriber configured. Add a Deepgram provider to transcribe live audio."
        items={transcribers}
        activeId={settings.transcriberId}
        testingId={testingId}
        onSelect={select}
        onTest={testSaved}
        onEdit={openEdit}
        onRemove={remove}
      />

      <ProviderGroup
        title="Brain (Answers & Reports)"
        empty="No brain configured. Add Gemini AI Studio, OpenAI, OpenRouter, or Ollama."
        items={brains}
        activeId={settings.brainId}
        testingId={testingId}
        onSelect={select}
        onTest={testSaved}
        onEdit={openEdit}
        onRemove={remove}
      />
    </div>
  )
}

function ProviderGroup({
  title,
  empty,
  items,
  activeId,
  testingId,
  onSelect,
  onTest,
  onEdit,
  onRemove
}: {
  title: string
  empty: string
  items: ProviderView[]
  activeId: string | null
  testingId: string | null
  onSelect: (p: ProviderView) => void
  onTest: (p: ProviderView) => void
  onEdit: (p: ProviderView) => void
  onRemove: (id: string) => void
}) {
  return (
    <section className="space-y-1.5">
      <h3 className="text-[11px] font-semibold uppercase tracking-wide text-white/45">{title}</h3>
      {items.length === 0 ? (
        <p className="rounded-md border border-dashed border-white/10 px-2.5 py-2 text-[11px] text-white/45">{empty}</p>
      ) : (
        <div className="space-y-1.5">
          {items.map((p) => {
            const active = p.id === activeId
            return (
              <div key={p.id} className={cn("flex items-center gap-2 rounded-lg border px-2.5 py-2", active ? "border-sky-400/40 bg-sky-500/10" : "border-white/10 bg-white/[0.03]")}>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <span className="truncate font-medium text-white">{p.label}</span>
                    {active && <span className="rounded bg-sky-500/25 px-1.5 py-0.5 text-[10px] font-medium text-sky-200">Active</span>}
                    <span className="inline-flex items-center gap-0.5 text-[10px] text-white/45">
                      <KeyRound className="h-2.5 w-2.5" /> {p.hasKey ? "Key saved" : "No key"}
                    </span>
                  </div>
                  <div className="truncate text-[10.5px] text-white/45">
                    {p.model} · {hostOf(p.endpoint)}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {!active && (
                    <Button size="sm" variant="ghost" className="h-6 px-2 text-[11px]" onClick={() => onSelect(p)}>
                      Use
                    </Button>
                  )}
                  <Button size="sm" variant="ghost" className="h-6 px-2 text-[11px]" disabled={testingId !== null} onClick={() => onTest(p)}>
                    {testingId === p.id ? "…" : "Test"}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-6 px-2 text-[11px]" onClick={() => onEdit(p)}>
                    Edit
                  </Button>
                  <Button size="icon-sm" variant="ghost" className="h-6 w-6 text-rose-300/75 hover:text-rose-200" onClick={() => onRemove(p.id)} title="Remove provider">
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}

export function PromptsSettings({
  settings,
  update
}: {
  settings: SettingsView
  update: (patch: SettingsPatch) => Promise<void>
}) {
  const active = settings.prompts.find((p) => p.id === settings.activePromptId) ?? settings.prompts[0]
  const [selectedId, setSelectedId] = useState(active?.id ?? "")
  const current = settings.prompts.find((p) => p.id === selectedId) ?? active

  const [title, setTitle] = useState(current?.title ?? "")
  const [content, setContent] = useState(current?.content ?? "")
  const [notes, setNotes] = useState(current?.notes ?? "")
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    if (!current) return
    setTitle(current.title)
    setContent(current.content)
    setNotes(current.notes ?? "")
  }, [current?.id])

  async function savePrompt(e: React.FormEvent) {
    e.preventDefault()
    if (!current) return
    const nextPrompts: Prompt[] = settings.prompts.map((p) =>
      p.id === current.id ? { ...p, title: title.trim() || "Untitled prompt", content, notes } : p
    )
    await update({ prompts: nextPrompts, activePromptId: current.id })
    setSaved(true)
    setTimeout(() => setSaved(false), 1500)
  }

  async function createPrompt() {
    const id = `p_${Date.now().toString(36)}`
    const next: Prompt = { id, title: "New prompt", content: settings.prompts[0]?.content ?? "", notes: "" }
    await update({ prompts: [...settings.prompts, next], activePromptId: id })
    setSelectedId(id)
  }

  async function deletePrompt(id: string) {
    if (settings.prompts.length <= 1) return
    const remaining = settings.prompts.filter((p) => p.id !== id)
    const nextActive = settings.activePromptId === id ? remaining[0].id : settings.activePromptId ?? remaining[0].id
    await update({ prompts: remaining, activePromptId: nextActive })
    setSelectedId(nextActive)
  }

  return (
    <form onSubmit={(e) => void savePrompt(e)} className="space-y-2.5 py-2 text-[11.5px]">
      <div className="flex items-center gap-1.5">
        <select
          className={INPUT_CLS}
          value={current?.id ?? ""}
          onChange={(e) => {
            setSelectedId(e.target.value)
            void update({ activePromptId: e.target.value })
          }}
        >
          {settings.prompts.map((p) => (
            <option key={p.id} value={p.id}>
              {p.title} {p.id === settings.activePromptId ? "(Active)" : ""}
            </option>
          ))}
        </select>
        <Button type="button" size="sm" variant="secondary" className="h-7 shrink-0 px-2 text-[11px]" onClick={() => void createPrompt()}>
          <Plus className="h-3 w-3" /> New
        </Button>
        {current && settings.prompts.length > 1 && (
          <Button type="button" size="icon-sm" variant="ghost" className="h-7 w-7 text-rose-300/80" onClick={() => void deletePrompt(current.id)} title="Delete prompt">
            <Trash2 className="h-3 w-3" />
          </Button>
        )}
      </div>

      <div>
        <label className="mb-1 block text-[11px] text-white/55">Prompt title</label>
        <input className={INPUT_CLS} value={title} onChange={(e) => setTitle(e.target.value)} />
      </div>

      <div>
        <label className="mb-1 block text-[11px] text-white/55">System instructions</label>
        <textarea className={cn(TEXTAREA_CLS, "h-24 resize-y")} value={content} onChange={(e) => setContent(e.target.value)} />
      </div>

      <div>
        <label className="mb-1 block text-[11px] text-white/55">Background notes (CV, job description, product facts)</label>
        <textarea className={cn(TEXTAREA_CLS, "h-20 resize-y")} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="Paste your role, CV highlights, or product context…" />
      </div>

      <div className="flex justify-end">
        <Button type="submit" size="sm" variant="primary">
          {saved ? (
            <>
              <Check className="h-3 w-3" /> Saved
            </>
          ) : (
            "Save prompt"
          )}
        </Button>
      </div>
    </form>
  )
}

export function ShortcutsSettings({
  settings,
  update
}: {
  settings: SettingsView
  update: (patch: SettingsPatch) => Promise<void>
}) {
  const [draft, setDraft] = useState<Shortcuts>(settings.shortcuts)
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    setDraft(settings.shortcuts)
  }, [settings.shortcuts])

  async function save(e: React.FormEvent) {
    e.preventDefault()
    await update({ shortcuts: draft })
    setSaved(true)
    setTimeout(() => setSaved(false), 1500)
  }

  async function reset() {
    setDraft(DEFAULT_SHORTCUTS)
    await update({ shortcuts: DEFAULT_SHORTCUTS })
  }

  return (
    <form onSubmit={(e) => void save(e)} className="space-y-2 py-2 text-[11.5px]">
      {settings.shortcutConflicts.length > 0 && (
        <div className="rounded border border-amber-400/25 bg-amber-500/10 px-2.5 py-1.5 text-[11px] text-amber-200">
          Could not bind globally: {settings.shortcutConflicts.join(", ")}. Wayland compositors may restrict global hotkeys.
        </div>
      )}
      <div className="space-y-1.5">
        {(Object.keys(SHORTCUT_LABELS) as ShortcutAction[]).map((action) => (
          <div key={action} className="grid grid-cols-[1fr_170px] items-center gap-2">
            <span className="truncate text-white/75" title={SHORTCUT_LABELS[action]}>
              {SHORTCUT_LABELS[action]}
            </span>
            <input
              className={cn(INPUT_CLS, settings.shortcutConflicts.includes(action) && "border-amber-400/50")}
              value={draft[action]}
              onChange={(e) => setDraft({ ...draft, [action]: e.target.value })}
              spellCheck={false}
            />
          </div>
        ))}
      </div>
      <div className="flex justify-end gap-1.5 pt-1">
        <Button type="button" size="sm" variant="ghost" onClick={() => void reset()}>
          Reset defaults
        </Button>
        <Button type="submit" size="sm" variant="primary">
          {saved ? "Saved" : "Apply shortcuts"}
        </Button>
      </div>
    </form>
  )
}

export function MeetingsHistory() {
  const { meetings, refresh } = useMeetings()
  const migration = useMigrationStatus()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const selected = meetings.find((m) => m.id === selectedId) ?? null

  async function regenerate(id: string) {
    setBusyId(id)
    setError(null)
    try {
      await api.invoke("reports:generate", id)
      await refresh()
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusyId(null)
    }
  }

  async function exportMeeting(id: string) {
    setError(null)
    try {
      await api.invoke("meetings:export", id, true)
    } catch (err) {
      setError(errorText(err))
    }
  }

  async function remove(id: string) {
    setError(null)
    try {
      await api.invoke("meetings:delete", id)
      if (selectedId === id) setSelectedId(null)
      await refresh()
    } catch (err) {
      setError(errorText(err))
    }
  }

  if (selected) {
    return <MeetingDetail meeting={selected} busy={busyId === selected.id} error={error} onBack={() => setSelectedId(null)} onRegenerate={() => void regenerate(selected.id)} onExport={() => void exportMeeting(selected.id)} onDelete={() => void remove(selected.id)} />
  }

  return (
    <div className="space-y-2 py-2 text-[11.5px]">
      <MigrationNotice outcome={migration} />
      {error && <p className="rounded bg-rose-500/15 px-2.5 py-1 text-[11px] text-rose-200">{error}</p>}
      <div className="flex items-center justify-between">
        <span className="text-white/55">{meetings.length} saved meeting(s)</span>
        <Button size="sm" variant="ghost" className="h-6 px-2 text-[11px]" onClick={() => void api.invoke("transcripts:reveal")}>
          <FolderOpen className="h-3 w-3" /> Open folder
        </Button>
      </div>
      {meetings.length === 0 ? (
        <p className="rounded-md border border-dashed border-white/10 px-3 py-4 text-center text-[11px] text-white/45">
          Finished meetings and transcripts appear here.
        </p>
      ) : (
        <div className="space-y-1.5">
          {meetings.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => setSelectedId(m.id)}
              className="flex w-full items-center gap-2 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-2 text-left transition-colors hover:bg-white/[0.07]"
            >
              <FileText className="h-3.5 w-3.5 shrink-0 text-white/45" />
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium text-white">{m.title || "Untitled meeting"}</div>
                <div className="text-[10.5px] text-white/45">
                  {formatDate(m.startTime)} · {formatDuration(m.duration)}
                </div>
              </div>
              <span
                className={cn(
                  "rounded px-1.5 py-0.5 text-[10px]",
                  m.status === "completed" ? "bg-emerald-500/15 text-emerald-200" : m.status === "failed" ? "bg-rose-500/15 text-rose-200" : "bg-sky-500/15 text-sky-200"
                )}
              >
                {m.status}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function MeetingDetail({
  meeting,
  busy,
  error,
  onBack,
  onRegenerate,
  onExport,
  onDelete
}: {
  meeting: Meeting
  busy: boolean
  error: string | null
  onBack: () => void
  onRegenerate: () => void
  onExport: () => void
  onDelete: () => void
}) {
  const s = meeting.summary
  return (
    <div className="space-y-2.5 py-2 text-[11.5px]">
      <div className="flex items-center gap-1.5">
        <Button size="sm" variant="ghost" className="h-6 px-2 text-[11px]" onClick={onBack}>
          ← Back
        </Button>
        <div className="ml-auto flex items-center gap-1">
          <Button size="sm" variant="secondary" className="h-6 px-2 text-[11px]" disabled={busy} onClick={onRegenerate}>
            <Sparkles className="h-3 w-3" /> {busy ? "Generating…" : s ? "Regenerate" : "Generate report"}
          </Button>
          <Button size="sm" variant="secondary" className="h-6 px-2 text-[11px]" onClick={onExport}>
            Export .txt
          </Button>
          <Button size="icon-sm" variant="ghost" className="h-6 w-6 text-rose-300/80" onClick={onDelete} title="Delete meeting">
            <Trash2 className="h-3 w-3" />
          </Button>
        </div>
      </div>

      {error && <p className="rounded bg-rose-500/15 px-2.5 py-1 text-[11px] text-rose-200">{error}</p>}

      <div>
        <h3 className="text-[13px] font-semibold text-white">{meeting.title}</h3>
        <p className="text-[10.5px] text-white/45">
          {formatDate(meeting.startTime)} · {formatDuration(meeting.duration)}
        </p>
      </div>

      {s && (
        <div className="space-y-2 rounded-lg border border-white/10 bg-white/[0.03] p-2.5">
          {s.overview && <p className="selectable leading-relaxed text-white/85">{s.overview}</p>}
          <SummaryList label="Key points" items={s.keyPoints} />
          <SummaryList label="Decisions" items={s.decisions} />
          <SummaryList label="Action items" items={s.actionItems} />
          <SummaryList label="Next steps" items={s.nextSteps} />
          <SummaryList label="Follow-up questions" items={s.followUpQuestions} />
        </div>
      )}

      <div>
        <div className="mb-1 text-[10.5px] font-semibold uppercase tracking-wide text-white/45">Transcript</div>
        <pre className="selectable whitespace-pre-wrap rounded-lg border border-white/10 bg-black/35 p-2.5 font-sans text-[11.5px] leading-relaxed text-white/85">
          {meeting.fullTranscriptText || "(Empty transcript)"}
        </pre>
      </div>
    </div>
  )
}

function SummaryList({ label, items }: { label: string; items: string[] }) {
  if (!items || items.length === 0) return null
  return (
    <div>
      <div className="text-[10.5px] font-semibold uppercase tracking-wide text-white/45">{label}</div>
      <ul className="mt-0.5 list-disc space-y-0.5 pl-4 text-white/80">
        {items.map((item, i) => (
          <li key={i} className="selectable">
            {item}
          </li>
        ))}
      </ul>
    </div>
  )
}

function MigrationNotice({ outcome }: { outcome: MigrationOutcome | null }) {
  if (!outcome || outcome.state === "noLegacyData") return null
  if (outcome.state === "failed") {
    return (
      <div className="rounded border border-rose-400/25 bg-rose-500/10 px-2.5 py-1.5 text-[11px] text-rose-200">
        Migration from Electron did not complete: {outcome.error}
      </div>
    )
  }
  const { meetingsImported, meetingsSkipped } = outcome.marker
  if (outcome.state === "imported") {
    return (
      <div className="rounded border border-emerald-400/25 bg-emerald-500/10 px-2.5 py-1.5 text-[11px] text-emerald-100">
        Imported {meetingsImported} meeting(s) and preferences from Electron ({meetingsSkipped} skipped). Legacy credentials were not imported.
      </div>
    )
  }
  return null
}
