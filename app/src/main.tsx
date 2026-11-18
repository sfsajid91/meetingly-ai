import React from "react"
import ReactDOM from "react-dom/client"
import "@fontsource-variable/inter/opsz.css"
import "./index.css"
import { loadLocale, resolveLocale, setLocale } from "@shared/i18n"

const container = document.getElementById("root")!
const kind = window.location.hash.replace(/^#\/?/, "") === "chat" ? "chat" : "main"
const uiLocale = resolveLocale("auto", typeof navigator !== "undefined" ? navigator.language : "en")
setLocale(uiLocale)
document.documentElement.lang = uiLocale
document.body.dataset.window = kind

async function boot() {
  const [, Root] = await Promise.all([
    loadLocale(uiLocale),
    kind === "chat"
      ? import("./windows/PanelWindow").then((m) => m.PanelWindow)
      : import("./windows/MainWindow").then((m) => m.MainWindow)
  ])

  ReactDOM.createRoot(container).render(
    <React.StrictMode>
      <Root />
    </React.StrictMode>
  )
}

void boot()
