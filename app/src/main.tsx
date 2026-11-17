import React from "react"
import ReactDOM from "react-dom/client"
import "@fontsource-variable/inter/opsz.css"
import "./index.css"
import { resolveLocale, setLocale } from "@shared/i18n"
import { MainWindow } from "./windows/MainWindow"
import { PanelWindow } from "./windows/PanelWindow"

const container = document.getElementById("root")!
const windows = { main: MainWindow, chat: PanelWindow }
const kind = window.location.hash.replace(/^#\/?/, "") === "chat" ? "chat" : "main"
const uiLocale = resolveLocale("auto", typeof navigator !== "undefined" ? navigator.language : "en")
setLocale(uiLocale)
document.documentElement.lang = uiLocale
document.body.dataset.window = kind
const Root = windows[kind]

ReactDOM.createRoot(container).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>
)
