import { Fragment, memo, useEffect, useMemo, useRef, useState } from "react"
import { Check, Copy } from "lucide-react"
import { t } from "@shared/i18n"
import { parseMarkdownBlocks, type InlineToken, type MarkdownBlock } from "@/lib/markdown"
import { cn, copyText } from "@/lib/utils"

function inlineEqual(a: InlineToken[], b: InlineToken[]): boolean {
  if (a === b) return true
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    const ta = a[i]
    const tb = b[i]
    if (ta.kind !== tb.kind) return false
    switch (ta.kind) {
      case "text":
      case "code":
      case "placeholder":
        if (ta.value !== (tb as typeof ta).value) return false
        break
      case "link":
        if (ta.label !== (tb as typeof ta).label || ta.href !== (tb as typeof ta).href) return false
        break
      case "strong":
      case "em":
        if (!inlineEqual(ta.children, (tb as typeof ta).children)) return false
        break
    }
  }
  return true
}

function linesEqual(a: InlineToken[][], b: InlineToken[][]): boolean {
  if (a === b) return true
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (!inlineEqual(a[i], b[i])) return false
  }
  return true
}

function blocksEqual(a: MarkdownBlock, b: MarkdownBlock): boolean {
  if (a === b) return true
  if (a.kind !== b.kind) return false
  switch (a.kind) {
    case "code": {
      const cb = b as typeof a
      return a.language === cb.language && a.closed === cb.closed && a.code === cb.code
    }
    case "heading": {
      const hb = b as typeof a
      return a.level === hb.level && inlineEqual(a.tokens, hb.tokens)
    }
    case "paragraph": {
      const pb = b as typeof a
      return a.isLead === pb.isLead && linesEqual(a.lines, pb.lines)
    }
    case "list": {
      const lb = b as typeof a
      return a.ordered === lb.ordered && linesEqual(a.items, lb.items)
    }
  }
}

function renderInlineTokens(tokens: InlineToken[]): React.ReactNode {
  return tokens.map((token, idx) => {
    switch (token.kind) {
      case "text":
        return <Fragment key={idx}>{token.value}</Fragment>
      case "code":
        return (
          <code key={idx} className="rounded bg-white/10 px-1.5 py-0.5 font-mono text-[12.5px] text-amber-200">
            {token.value}
          </code>
        )
      case "strong":
        return (
          <strong key={idx} className="font-semibold text-white">
            {renderInlineTokens(token.children)}
          </strong>
        )
      case "em":
        return <em key={idx}>{renderInlineTokens(token.children)}</em>
      case "link":
        return (
          <a
            key={idx}
            href={token.href}
            target="_blank"
            rel="noreferrer"
            className="text-sky-300 underline underline-offset-4 hover:text-sky-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {token.label}
          </a>
        )
      case "placeholder":
        return (
          <span
            key={idx}
            className="rounded border border-amber-400/30 bg-amber-500/12 px-1.5 py-0.5 text-[12.5px] font-medium text-amber-200"
          >
            [{token.value}]
          </span>
        )
    }
  })
}

function CodeBlock({ language, code, closed }: { language: string; code: string; closed: boolean }) {
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<number | null>(null)

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [])

  const copy = async () => {
    if (await copyText(code)) {
      setCopied(true)
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = window.setTimeout(() => setCopied(false), 1200)
    }
  }

  return (
    <div className="my-2.5 overflow-hidden rounded-lg border border-white/10 bg-[#0b0e14]">
      <div className="flex items-center justify-between border-b border-white/[0.07] bg-white/[0.02] px-3 py-1.5 text-[11px] text-muted-foreground">
        <div className="flex items-center gap-2 font-mono">
          <span>{language || "code"}</span>
          {!closed && <span className="text-sky-300/80">streaming…</span>}
        </div>
        <button
          type="button"
          onClick={() => void copy()}
          aria-label={t("Copy code")}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-white/10 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {copied ? <Check className="h-3 w-3 text-emerald-300" /> : <Copy className="h-3 w-3" />}
          <span>{copied ? t("Copied") : t("Copy")}</span>
        </button>
      </div>
      <pre className="m-0 overflow-x-auto bg-transparent p-3.5 font-mono text-[12.5px] leading-relaxed text-emerald-200">
        <code>{code}</code>
      </pre>
    </div>
  )
}

const MarkdownBlockView = memo(
  function MarkdownBlockView({ block }: { block: MarkdownBlock }) {
    switch (block.kind) {
      case "code":
        return <CodeBlock language={block.language} code={block.code} closed={block.closed} />
      case "heading":
        return (
          <h3 className="mt-3 mb-1 text-[14.5px] font-semibold tracking-[-0.01em] text-white first:mt-0">
            {renderInlineTokens(block.tokens)}
          </h3>
        )
      case "paragraph":
        return (
          <p
            className={cn(
              "my-0 mb-2.5 last:mb-0",
              block.isLead ? "text-[15.5px] font-semibold leading-[1.45] text-white" : "text-[14px] leading-[1.6] text-foreground/95"
            )}
          >
            {block.lines.map((lineTokens, idx) => (
              <Fragment key={idx}>
                {idx > 0 && <br />}
                {renderInlineTokens(lineTokens)}
              </Fragment>
            ))}
          </p>
        )
      case "list":
        if (block.ordered) {
          return (
            <ol className="my-2.5 list-decimal space-y-1.5 pl-5 text-[14px] leading-[1.55] text-foreground/95">
              {block.items.map((itemTokens, idx) => (
                <li key={idx}>{renderInlineTokens(itemTokens)}</li>
              ))}
            </ol>
          )
        }
        return (
          <ul className="my-2.5 list-none space-y-1.5 pl-0 text-[14px] leading-[1.55] text-foreground/95">
            {block.items.map((itemTokens, idx) => (
              <li key={idx} className="flex items-start gap-2.5">
                <span aria-hidden className="mt-[0.55em] h-1.5 w-1.5 shrink-0 rounded-full bg-sky-400/80" />
                <span className="min-w-0 flex-1">{renderInlineTokens(itemTokens)}</span>
              </li>
            ))}
          </ul>
        )
    }
  },
  (prev, next) => blocksEqual(prev.block, next.block)
)

export function Markdown({
  text,
  streaming = false,
  className
}: {
  text: string
  streaming?: boolean
  className?: string
}) {
  const blocks = useMemo(() => parseMarkdownBlocks(text, { streaming }), [text, streaming])
  return (
    <div className={cn("selectable text-[14px] leading-[1.6] text-foreground/95", className)}>
      {blocks.map((block, idx) => (
        <MarkdownBlockView key={idx} block={block} />
      ))}
    </div>
  )
}
