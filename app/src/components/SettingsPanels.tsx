import { useEffect, useState } from "react"
import {
  AlertCircle,
  ArrowLeft,
  Check,
  Eye,
  EyeOff,
  FileText,
  FolderOpen,
  KeyRound,
  Keyboard,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  X
} from "lucide-react"
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
import { api, errorText, shortcutLabel } from "@/lib/api"
import { formatDate, formatDuration } from "@/lib/format"
import { useMeetings, useMigrationStatus } from "@/lib/hooks"
import {
  keyEventToAccelerator,
  validatePromptInput,
  validateProviderInput,
  validateShortcuts
} from "@/lib/validation"
import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger
} from "@/components/ui/alert-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Kbd } from "@/components/ui/kbd"
import { Label } from "@/components/ui/label"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

function FieldError({ message }: { message?: string }) {
  if (!message) return null
  return <p className="mt-1.5 text-xs text-rose-300">{message}</p>
}

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
  const [showKey, setShowKey] = useState(false)
  const [clearKey, setClearKey] = useState(false)
  const [submitted, setSubmitted] = useState(false)
  const [models, setModels] = useState<string[]>([])
  const [modelsBusy, setModelsBusy] = useState(false)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const formKind = editing?.kind ?? addingKind

  const validation = formKind
    ? validateProviderInput({
        kind: formKind,
        label,
        endpoint,
        model,
        apiKey,
        clearKey,
        hasExistingKey: Boolean(editing?.hasKey)
      })
    : null

  function openNew(kind: ProviderKind) {
    setEditing(null)
    setAddingKind(kind)
    setTestResult(null)
    setError(null)
    setSubmitted(false)
    setShowKey(false)
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
    setShowKey(false)
    setClearKey(false)
    setSubmitted(false)
    setModels([])
    setTestResult(null)
    setError(null)
  }

  function closeForm() {
    setEditing(null)
    setAddingKind(null)
    setError(null)
    setTestResult(null)
    setSubmitted(false)
  }

  async function loadModels() {
    if (!endpoint.trim() || formKind !== "openai-compatible" || validation?.errors.endpoint) return
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
    setSubmitted(true)
    if (validation && (validation.errors.endpoint || validation.errors.model || (formKind === "deepgram" && validation.errors.apiKey))) {
      return
    }
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
    setSubmitted(true)
    if (validation && !validation.valid) return
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
  const showErr = (field: "label" | "endpoint" | "model" | "apiKey") => (submitted ? validation?.errors[field] : undefined)

  return (
    <div className="space-y-5 text-[13px]">
      <Alert className="border-sky-400/25 bg-sky-500/10 py-3 text-sky-100">
        <KeyRound className="h-4 w-4 text-sky-300" />
        <AlertDescription className="text-xs leading-relaxed text-sky-100/90">
          Audio and transcript text are sent directly to your configured cloud providers and may incur usage charges. API keys are stored in your OS keychain.
        </AlertDescription>
      </Alert>

      <div className="flex flex-wrap gap-2">
        <Button size="default" variant="secondary" onClick={() => openNew("deepgram")}>
          <Plus className="h-4 w-4" /> Add transcriber
        </Button>
        <Button size="default" variant="secondary" onClick={() => openNew("openai-compatible")}>
          <Plus className="h-4 w-4" /> Add brain
        </Button>
      </div>

      {error && (
        <Alert variant="destructive" className="border-rose-400/30 bg-rose-500/12 py-2.5 text-rose-100">
          <AlertCircle className="h-4 w-4 text-rose-300" />
          <AlertDescription className="selectable text-xs text-rose-100">{error}</AlertDescription>
        </Alert>
      )}

      {testResult && (
        <Alert
          className={cn(
            "py-3",
            testResult.ok ? "border-emerald-400/30 bg-emerald-500/10 text-emerald-100" : "border-rose-400/30 bg-rose-500/12 text-rose-100"
          )}
        >
          {testResult.ok ? <Check className="h-4 w-4 text-emerald-300" /> : <AlertCircle className="h-4 w-4 text-rose-300" />}
          <AlertDescription className="space-y-2 text-xs text-current">
            <div>{testResult.detail}</div>
            {(testResult.streaming !== null || testResult.vision !== null) && (
              <div className="flex flex-wrap gap-2">
                {testResult.streaming !== null && (
                  <Badge variant="outline" className="border-current/25 text-[11px]">
                    Streaming: {testResult.streaming ? "supported" : "failed"}
                  </Badge>
                )}
                {testResult.vision !== null && (
                  <Badge variant="outline" className="border-current/25 text-[11px]">
                    Images: {testResult.vision ? "supported" : "unsupported"}
                  </Badge>
                )}
              </div>
            )}
          </AlertDescription>
        </Alert>
      )}

      {formKind && (
        <form onSubmit={(e) => void save(e)} noValidate className="space-y-4 rounded-xl border border-white/[0.12] bg-[#1e2532] p-5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-sm font-semibold text-white">
              {editing ? `Edit ${editing.label}` : formKind === "deepgram" ? "New Deepgram transcriber" : "New OpenAI-compatible brain"}
            </span>
            {!editing && formKind === "openai-compatible" && (
              <div className="flex flex-wrap gap-1.5">
                {BRAIN_PRESETS.map((p) => (
                  <Button
                    key={p.label}
                    type="button"
                    size="xs"
                    variant={endpoint === p.endpoint ? "secondary" : "ghost"}
                    onClick={() => {
                      setLabel(p.label)
                      setEndpoint(p.endpoint)
                      setModel(p.model)
                      setModels([])
                    }}
                  >
                    {p.label}
                  </Button>
                ))}
              </div>
            )}
          </div>

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div>
              <Label htmlFor="provider-label" className="mb-1.5 block text-xs text-muted-foreground">
                Label
              </Label>
              <Input
                id="provider-label"
                className="h-9 bg-[#0f131a] text-[13px]"
                value={label}
                aria-invalid={Boolean(showErr("label"))}
                onChange={(e) => setLabel(e.target.value)}
                placeholder="My provider"
              />
              <FieldError message={showErr("label")} />
            </div>

            <div>
              <Label htmlFor="provider-endpoint" className="mb-1.5 block text-xs text-muted-foreground">
                Endpoint URL
              </Label>
              <Input
                id="provider-endpoint"
                className="h-9 bg-[#0f131a] text-[13px]"
                value={endpoint}
                aria-invalid={Boolean(showErr("endpoint"))}
                onChange={(e) => setEndpoint(e.target.value)}
                placeholder={formKind === "deepgram" ? "wss://api.deepgram.com/v1/listen" : "https://api.openai.com/v1"}
                spellCheck={false}
              />
              <FieldError message={showErr("endpoint")} />
            </div>

            <div>
              <Label htmlFor="provider-apikey" className="mb-1.5 block text-xs text-muted-foreground">
                API key
              </Label>
              <div className="flex gap-1.5">
                <Input
                  id="provider-apikey"
                  type={showKey ? "text" : "password"}
                  className="h-9 flex-1 bg-[#0f131a] text-[13px]"
                  value={apiKey}
                  aria-invalid={Boolean(showErr("apiKey"))}
                  onChange={(e) => {
                    setApiKey(e.target.value)
                    if (e.target.value) setClearKey(false)
                  }}
                  placeholder={editing?.hasKey ? "Saved in keychain. Leave empty to keep." : formKind === "deepgram" ? "Deepgram API key" : "Optional for localhost"}
                  autoComplete="off"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  className="h-9 w-9"
                  onClick={() => setShowKey((s) => !s)}
                  aria-label={showKey ? "Hide API key" : "Show API key"}
                >
                  {showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </Button>
              </div>
              <FieldError message={showErr("apiKey")} />
              {!showErr("apiKey") && validation?.warning && (
                <p className="mt-1.5 text-xs text-amber-200/90">{validation.warning}</p>
              )}
            </div>

            <div>
              <Label htmlFor="provider-model" className="mb-1.5 block text-xs text-muted-foreground">
                Model
              </Label>
              <div className="flex gap-1.5">
                <Input
                  id="provider-model"
                  className="h-9 flex-1 bg-[#0f131a] text-[13px]"
                  value={model}
                  aria-invalid={Boolean(showErr("model"))}
                  onChange={(e) => setModel(e.target.value)}
                  list="provider-models-list"
                  placeholder={formKind === "deepgram" ? "nova-3" : "gpt-4.1-mini"}
                  spellCheck={false}
                />
                {formKind === "openai-compatible" && (
                  <Button
                    type="button"
                    size="icon"
                    variant="outline"
                    className="h-9 w-9"
                    onClick={() => void loadModels()}
                    disabled={modelsBusy || Boolean(validation?.errors.endpoint)}
                    title="Fetch models from endpoint"
                    aria-label="Fetch models"
                  >
                    <RefreshCw className={cn("h-4 w-4", modelsBusy && "animate-spin")} />
                  </Button>
                )}
                <datalist id="provider-models-list">
                  {models.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
              </div>
              <FieldError message={showErr("model")} />
              {models.length > 0 && (
                <div className="mt-2 flex max-h-24 flex-wrap gap-1.5 overflow-y-auto">
                  {models.slice(0, 14).map((m) => (
                    <button
                      key={m}
                      type="button"
                      onClick={() => setModel(m)}
                      className={cn(
                        "rounded-md border px-2 py-0.5 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                        model === m ? "border-sky-400/50 bg-sky-500/20 text-sky-100" : "border-white/10 bg-[#0f131a] text-muted-foreground hover:text-foreground"
                      )}
                    >
                      {m}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>

          {editing?.hasKey && formKind === "openai-compatible" && (
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <Checkbox checked={clearKey} onCheckedChange={(c) => setClearKey(Boolean(c))} />
              <span>Remove saved key from keychain</span>
            </label>
          )}

          <div className="flex items-center justify-end gap-2 pt-1">
            <Button type="button" size="sm" variant="ghost" onClick={closeForm}>
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              disabled={testingId !== null || !endpoint.trim() || !model.trim()}
              onClick={() => void testDraft()}
            >
              {testingId === "draft" ? (
                <>
                  <Loader2 className="h-3.5 w-3.5 animate-spin" /> Testing…
                </>
              ) : (
                "Test connection"
              )}
            </Button>
            <Button type="submit" size="sm" variant="default" disabled={saving}>
              {saving ? (
                <>
                  <Loader2 className="h-3.5 w-3.5 animate-spin" /> Saving…
                </>
              ) : (
                "Save provider"
              )}
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
    <section className="space-y-2">
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{title}</h3>
      {items.length === 0 ? (
        <p className="rounded-xl border border-dashed border-white/15 px-4 py-4 text-xs text-muted-foreground">{empty}</p>
      ) : (
        <div className="space-y-2">
          {items.map((p) => {
            const active = p.id === activeId
            return (
              <div
                key={p.id}
                className={cn(
                  "flex items-center gap-3 rounded-xl border px-4 py-3 transition-colors",
                  active ? "border-sky-400/40 bg-sky-500/10" : "border-white/[0.08] bg-[#1e2532]/70"
                )}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="truncate text-[13.5px] font-semibold text-foreground">{p.label}</span>
                    {active && <Badge className="bg-sky-500/25 text-[11px] text-sky-200">Active</Badge>}
                    <span className="inline-flex items-center gap-1 text-[11.5px] text-muted-foreground">
                      <KeyRound className="h-3 w-3" /> {p.hasKey ? "Key saved" : "No key"}
                    </span>
                  </div>
                  <div className="mt-0.5 truncate text-xs text-muted-foreground">
                    {p.model} · {hostOf(p.endpoint)}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {!active && (
                    <Button size="sm" variant="secondary" onClick={() => onSelect(p)}>
                      Use
                    </Button>
                  )}
                  <Button size="sm" variant="ghost" disabled={testingId !== null} onClick={() => onTest(p)}>
                    {testingId === p.id ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : "Test"}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => onEdit(p)}>
                    Edit
                  </Button>
                  <AlertDialog>
                    <AlertDialogTrigger
                      render={
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          className="text-rose-300/80 hover:bg-rose-500/15 hover:text-rose-200"
                          aria-label={`Remove ${p.label}`}
                        />
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </AlertDialogTrigger>
                    <AlertDialogContent size="sm">
                      <AlertDialogHeader>
                        <AlertDialogTitle>Remove "{p.label}"?</AlertDialogTitle>
                        <AlertDialogDescription>
                          {active
                            ? "This provider is currently active. Removing it also removes its saved key from your keychain."
                            : "This deletes the provider configuration and removes its saved key from your keychain."}
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <AlertDialogAction variant="destructive" onClick={() => onRemove(p.id)}>
                          Remove
                        </AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
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
  const [submitted, setSubmitted] = useState(false)
  const [saving, setSaving] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!current) return
    setTitle(current.title)
    setContent(current.content)
    setNotes(current.notes ?? "")
    setSubmitted(false)
    setError(null)
  }, [current?.id])

  const validation = validatePromptInput({ title, content, notes })
  const dirty = Boolean(
    current && (title !== current.title || content !== current.content || notes !== (current.notes ?? ""))
  )

  async function savePrompt(e: React.FormEvent) {
    e.preventDefault()
    if (!current) return
    setSubmitted(true)
    if (!validation.valid) return
    setSaving(true)
    setError(null)
    try {
      const nextPrompts: Prompt[] = settings.prompts.map((p) =>
        p.id === current.id ? { ...p, title: title.trim(), content: content.trim(), notes } : p
      )
      await update({ prompts: nextPrompts, activePromptId: current.id })
      setSaved(true)
      setTimeout(() => setSaved(false), 1500)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setSaving(false)
    }
  }

  function resetDraft() {
    if (!current) return
    setTitle(current.title)
    setContent(current.content)
    setNotes(current.notes ?? "")
    setSubmitted(false)
    setError(null)
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
    <form onSubmit={(e) => void savePrompt(e)} noValidate className="space-y-4 text-[13px]">
      <div className="flex items-center gap-2">
        <Select
          value={current?.id ?? ""}
          onValueChange={(v) => {
            if (!v) return
            setSelectedId(v)
            void update({ activePromptId: v })
          }}
        >
          <SelectTrigger className="h-9 min-w-0 flex-1 bg-[#0f131a] text-[13px]" aria-label="Active prompt">
            <SelectValue>
              {(v: string | null) => {
                const found = settings.prompts.find((p) => p.id === v)
                if (!found) return "Select prompt"
                return `${found.title}${found.id === settings.activePromptId ? " (Active)" : ""}`
              }}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {settings.prompts.map((p) => (
              <SelectItem key={p.id} value={p.id} className="text-xs">
                {p.title} {p.id === settings.activePromptId ? "(Active)" : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Button type="button" size="default" variant="secondary" onClick={() => void createPrompt()}>
          <Plus className="h-4 w-4" /> New
        </Button>

        {current && settings.prompts.length > 1 && (
          <AlertDialog>
            <AlertDialogTrigger
              render={
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  className="text-rose-300/80 hover:bg-rose-500/15 hover:text-rose-200"
                  aria-label="Delete prompt"
                />
              }
            >
              <Trash2 className="h-4 w-4" />
            </AlertDialogTrigger>
            <AlertDialogContent size="sm">
              <AlertDialogHeader>
                <AlertDialogTitle>Delete "{current.title}"?</AlertDialogTitle>
                <AlertDialogDescription>This permanently deletes this prompt preset.</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => void deletePrompt(current.id)}>
                  Delete
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
      </div>

      {error && (
        <Alert variant="destructive" className="border-rose-400/30 bg-rose-500/12 py-2 text-rose-100">
          <AlertCircle className="h-4 w-4 text-rose-300" />
          <AlertDescription className="text-xs text-rose-100">{error}</AlertDescription>
        </Alert>
      )}

      <div>
        <div className="mb-1.5 flex items-center justify-between">
          <Label htmlFor="prompt-title" className="text-xs text-muted-foreground">
            Prompt title
          </Label>
          {dirty && <Badge variant="secondary" className="text-[10.5px] text-amber-200">Unsaved changes</Badge>}
        </div>
        <Input
          id="prompt-title"
          className="h-9 bg-[#0f131a] text-[13px]"
          value={title}
          aria-invalid={Boolean(submitted && validation.errors.title)}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Meeting assistant"
        />
        <FieldError message={submitted ? validation.errors.title : undefined} />
      </div>

      <div>
        <div className="mb-1.5 flex items-center justify-between">
          <Label htmlFor="prompt-content" className="text-xs text-muted-foreground">
            System instructions
          </Label>
          <span className="text-[11px] tabular-nums text-muted-foreground">{content.length.toLocaleString()} chars</span>
        </div>
        <Textarea
          id="prompt-content"
          className="scroll h-36 resize-y bg-[#0f131a] text-[13px] leading-relaxed"
          value={content}
          aria-invalid={Boolean(submitted && validation.errors.content)}
          onChange={(e) => setContent(e.target.value)}
        />
        <FieldError message={submitted ? validation.errors.content : undefined} />
      </div>

      <div>
        <div className="mb-1.5 flex items-center justify-between">
          <Label htmlFor="prompt-notes" className="text-xs text-muted-foreground">
            Background notes (CV, job description, product facts)
          </Label>
          {notes.length > 0 && (
            <span className="text-[11px] tabular-nums text-muted-foreground">{notes.length.toLocaleString()} chars</span>
          )}
        </div>
        <Textarea
          id="prompt-notes"
          className="scroll h-28 resize-y bg-[#0f131a] text-[13px] leading-relaxed"
          value={notes}
          aria-invalid={Boolean(submitted && validation.errors.notes)}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Paste your role, CV highlights, or product context…"
        />
        <FieldError message={submitted ? validation.errors.notes : undefined} />
      </div>

      <div className="flex items-center justify-end gap-2">
        {dirty && (
          <Button type="button" size="default" variant="ghost" onClick={resetDraft}>
            Discard
          </Button>
        )}
        <Button type="submit" size="default" variant="default" disabled={saving || (!dirty && !saved)}>
          {saving ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" /> Saving…
            </>
          ) : saved ? (
            <>
              <Check className="h-4 w-4" /> Saved
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
  const [recording, setRecording] = useState<ShortcutAction | null>(null)
  const [saving, setSaving] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setDraft(settings.shortcuts)
  }, [settings.shortcuts])

  const validation = validateShortcuts(draft)
  const dirty = (Object.keys(SHORTCUT_LABELS) as ShortcutAction[]).some((k) => draft[k] !== settings.shortcuts[k])

  async function save(e: React.FormEvent) {
    e.preventDefault()
    if (!validation.valid) return
    setSaving(true)
    setError(null)
    try {
      await update({ shortcuts: draft })
      setSaved(true)
      setTimeout(() => setSaved(false), 1500)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setSaving(false)
    }
  }

  async function reset() {
    setDraft(DEFAULT_SHORTCUTS)
    setRecording(null)
    await update({ shortcuts: DEFAULT_SHORTCUTS })
  }

  return (
    <form onSubmit={(e) => void save(e)} noValidate className="space-y-4 text-[13px]">
      {settings.shortcutConflicts.length > 0 && (
        <Alert className="border-amber-400/30 bg-amber-500/10 py-2.5 text-amber-100">
          <AlertCircle className="h-4 w-4 text-amber-300" />
          <AlertDescription className="text-xs text-amber-100">
            Could not bind globally: {settings.shortcutConflicts.join(", ")}. Wayland compositors may restrict global hotkeys.
          </AlertDescription>
        </Alert>
      )}

      {error && (
        <Alert variant="destructive" className="border-rose-400/30 bg-rose-500/12 py-2.5 text-rose-100">
          <AlertCircle className="h-4 w-4 text-rose-300" />
          <AlertDescription className="text-xs text-rose-100">{error}</AlertDescription>
        </Alert>
      )}

      <div className="divide-y divide-white/[0.06] rounded-xl border border-white/[0.08] bg-[#1e2532]/60 px-4">
        {(Object.keys(SHORTCUT_LABELS) as ShortcutAction[]).map((action) => {
          const rowError = validation.errors[action]
          const isRecording = recording === action
          const hasConflict = settings.shortcutConflicts.includes(action)
          return (
            <div key={action} className="space-y-1 py-3">
              <div className="grid grid-cols-[1fr_auto] items-center gap-3">
                <div className="min-w-0">
                  <div className="truncate text-[13px] font-medium text-foreground" title={SHORTCUT_LABELS[action]}>
                    {SHORTCUT_LABELS[action]}
                  </div>
                </div>
                <div className="flex items-center gap-1.5">
                  <Input
                    className={cn("h-8 w-[180px] bg-[#0f131a] text-xs", hasConflict && !rowError && "border-amber-400/50")}
                    value={isRecording ? "Press keys…" : draft[action]}
                    aria-label={SHORTCUT_LABELS[action]}
                    aria-invalid={Boolean(rowError)}
                    readOnly={isRecording}
                    onChange={(e) => setDraft({ ...draft, [action]: e.target.value })}
                    onKeyDown={(e) => {
                      if (!isRecording) return
                      e.preventDefault()
                      e.stopPropagation()
                      if (e.key === "Escape") {
                        setRecording(null)
                        return
                      }
                      const accel = keyEventToAccelerator(e)
                      if (accel) {
                        setDraft({ ...draft, [action]: accel })
                        setRecording(null)
                      }
                    }}
                    onBlur={() => {
                      if (isRecording) setRecording(null)
                    }}
                    spellCheck={false}
                  />
                  <Button
                    type="button"
                    size="icon"
                    variant={isRecording ? "default" : "outline"}
                    onClick={() => setRecording(isRecording ? null : action)}
                    title={isRecording ? "Cancel recording (Esc)" : "Click and press shortcut keys"}
                    aria-label={isRecording ? "Cancel recording" : `Record shortcut for ${SHORTCUT_LABELS[action]}`}
                  >
                    <Keyboard className="h-4 w-4" />
                  </Button>
                  {!rowError && (
                    <Kbd className="hidden h-7 min-w-[64px] justify-center px-2 text-[11px] sm:inline-flex">
                      {shortcutLabel(draft[action])}
                    </Kbd>
                  )}
                </div>
              </div>
              {rowError && <p className="text-right text-xs text-rose-300">{rowError}</p>}
            </div>
          )
        })}
      </div>

      <div className="flex justify-end gap-2 pt-1">
        <Button type="button" size="default" variant="ghost" onClick={() => void reset()}>
          Reset defaults
        </Button>
        <Button type="submit" size="default" variant="default" disabled={!validation.valid || saving || (!dirty && !saved)}>
          {saving ? "Applying…" : saved ? "Saved" : "Apply shortcuts"}
        </Button>
      </div>
    </form>
  )
}

export function MeetingsHistory() {
  const { meetings, refresh } = useMeetings()
  const migration = useMigrationStatus()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [query, setQuery] = useState("")
  const [busyId, setBusyId] = useState<string | null>(null)
  const [exportNotice, setExportNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const selected = meetings.find((m) => m.id === selectedId) ?? null
  const needle = query.trim().toLowerCase()
  const filtered = needle
    ? meetings.filter((m) => m.title.toLowerCase().includes(needle) || m.fullTranscriptText.toLowerCase().includes(needle))
    : meetings

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
    setExportNotice(null)
    try {
      const path = await api.invoke("meetings:export", id, true)
      setExportNotice(`Exported to ${path}`)
      setTimeout(() => setExportNotice(null), 4000)
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
    return (
      <MeetingDetail
        meeting={selected}
        busy={busyId === selected.id}
        error={error}
        exportNotice={exportNotice}
        onBack={() => {
          setSelectedId(null)
          setExportNotice(null)
          setError(null)
        }}
        onRegenerate={() => void regenerate(selected.id)}
        onExport={() => void exportMeeting(selected.id)}
        onDelete={() => void remove(selected.id)}
      />
    )
  }

  return (
    <div className="space-y-4 text-[13px]">
      <MigrationNotice outcome={migration} />
      {error && (
        <Alert variant="destructive" className="border-rose-400/30 bg-rose-500/12 py-2.5 text-rose-100">
          <AlertCircle className="h-4 w-4 text-rose-300" />
          <AlertDescription className="text-xs text-rose-100">{error}</AlertDescription>
        </Alert>
      )}
      <div className="flex items-center justify-between gap-2">
        <span className="tabular-nums text-muted-foreground">
          {meetings.length} saved {meetings.length === 1 ? "meeting" : "meetings"}
        </span>
        <Button size="sm" variant="ghost" onClick={() => void api.invoke("transcripts:reveal")}>
          <FolderOpen className="h-3.5 w-3.5" /> Open folder
        </Button>
      </div>

      {meetings.length > 0 && (
        <div className="relative">
          <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search meetings or transcript text…"
            aria-label="Search meetings"
            className="h-9 bg-[#0f131a] pl-9 pr-8 text-[13px]"
          />
          {query && (
            <button
              type="button"
              onClick={() => setQuery("")}
              className="absolute right-2.5 top-2.5 rounded text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label="Clear search"
            >
              <X className="h-4 w-4" />
            </button>
          )}
        </div>
      )}

      {meetings.length === 0 ? (
        <p className="rounded-xl border border-dashed border-white/15 px-4 py-10 text-center text-sm text-muted-foreground">
          Finished meetings and transcripts appear here.
        </p>
      ) : filtered.length === 0 ? (
        <p className="rounded-xl border border-dashed border-white/15 px-4 py-8 text-center text-sm text-muted-foreground">
          No meetings match "{query}".
        </p>
      ) : (
        <div className="space-y-2">
          {filtered.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => setSelectedId(m.id)}
              className="flex w-full items-center gap-3 rounded-xl border border-white/[0.08] bg-[#1e2532]/70 px-4 py-3 text-left transition-colors hover:border-white/20 hover:bg-[#1e2532] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <FileText className="h-4 w-4 shrink-0 text-sky-300" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13.5px] font-semibold text-foreground">{m.title || "Untitled meeting"}</div>
                <div className="mt-0.5 text-xs tabular-nums text-muted-foreground">
                  {formatDate(m.startTime)} · {formatDuration(m.duration)}
                </div>
              </div>
              <Badge
                variant="outline"
                className={cn(
                  "border-transparent px-2 text-[11px]",
                  m.status === "completed"
                    ? "bg-emerald-500/15 text-emerald-200"
                    : m.status === "failed"
                      ? "bg-rose-500/15 text-rose-200"
                      : "bg-sky-500/15 text-sky-200"
                )}
              >
                {m.status}
              </Badge>
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
  exportNotice,
  onBack,
  onRegenerate,
  onExport,
  onDelete
}: {
  meeting: Meeting
  busy: boolean
  error: string | null
  exportNotice: string | null
  onBack: () => void
  onRegenerate: () => void
  onExport: () => void
  onDelete: () => void
}) {
  const s = meeting.summary
  return (
    <div className="space-y-4 text-[13.5px]">
      <div className="flex items-center gap-2">
        <Button size="sm" variant="ghost" onClick={onBack}>
          <ArrowLeft className="h-3.5 w-3.5" /> Back
        </Button>
        <div className="ml-auto flex items-center gap-1.5">
          <Button size="sm" variant="secondary" disabled={busy} onClick={onRegenerate}>
            {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Sparkles className="h-3.5 w-3.5" />}
            <span>{busy ? "Generating…" : s ? "Regenerate" : "Generate report"}</span>
          </Button>
          <Button size="sm" variant="secondary" onClick={onExport}>
            Export .txt
          </Button>
          <AlertDialog>
            <AlertDialogTrigger
              render={
                <Button
                  size="icon-sm"
                  variant="ghost"
                  className="text-rose-300/80 hover:bg-rose-500/15 hover:text-rose-200"
                  aria-label="Delete meeting"
                />
              }
            >
              <Trash2 className="h-4 w-4" />
            </AlertDialogTrigger>
            <AlertDialogContent size="sm">
              <AlertDialogHeader>
                <AlertDialogTitle>Delete this meeting?</AlertDialogTitle>
                <AlertDialogDescription>
                  This permanently deletes "{meeting.title || "Untitled meeting"}" and its summary from Meetingly.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={onDelete}>
                  Delete
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      </div>

      {error && (
        <Alert variant="destructive" className="border-rose-400/30 bg-rose-500/12 py-2.5 text-rose-100">
          <AlertCircle className="h-4 w-4 text-rose-300" />
          <AlertDescription className="text-xs text-rose-100">{error}</AlertDescription>
        </Alert>
      )}

      {exportNotice && (
        <Alert className="border-emerald-400/30 bg-emerald-500/10 py-2.5 text-emerald-100">
          <Check className="h-4 w-4 text-emerald-300" />
          <AlertDescription className="truncate text-xs text-emerald-100" title={exportNotice}>
            {exportNotice}
          </AlertDescription>
        </Alert>
      )}

      <div>
        <h3 className="text-lg font-semibold text-white">{meeting.title}</h3>
        <p className="mt-0.5 text-xs tabular-nums text-muted-foreground">
          {formatDate(meeting.startTime)} · {formatDuration(meeting.duration)}
        </p>
      </div>

      {s && (
        <div className="space-y-3.5 rounded-xl border border-white/[0.08] bg-[#1e2532] p-5">
          {s.overview && <p className="selectable leading-relaxed text-foreground/95">{s.overview}</p>}
          <SummaryList label="Key points" items={s.keyPoints} />
          <SummaryList label="Decisions" items={s.decisions} />
          <SummaryList label="Action items" items={s.actionItems} />
          <SummaryList label="Next steps" items={s.nextSteps} />
          <SummaryList label="Follow-up questions" items={s.followUpQuestions} />
        </div>
      )}

      <div>
        <div className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">Transcript</div>
        <pre className="scroll selectable whitespace-pre-wrap rounded-xl border border-white/[0.08] bg-[#0f131a] p-4 font-sans text-[13.5px] leading-relaxed text-foreground/90">
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
      <div className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</div>
      <ul className="mt-1.5 list-disc space-y-1 pl-5 text-foreground/90">
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
      <Alert variant="destructive" className="border-rose-400/30 bg-rose-500/12 py-2.5 text-rose-100">
        <AlertCircle className="h-4 w-4 text-rose-300" />
        <AlertDescription className="text-xs text-rose-100">
          Migration from Electron did not complete: {outcome.error}
        </AlertDescription>
      </Alert>
    )
  }
  const { meetingsImported, meetingsSkipped } = outcome.marker
  if (outcome.state === "imported") {
    return (
      <Alert className="border-emerald-400/30 bg-emerald-500/10 py-2.5 text-emerald-100">
        <Check className="h-4 w-4 text-emerald-300" />
        <AlertDescription className="text-xs text-emerald-100">
          Imported {meetingsImported} meeting(s) and preferences from Electron ({meetingsSkipped} skipped). Legacy credentials were not imported.
        </AlertDescription>
      </Alert>
    )
  }
  return null
}
