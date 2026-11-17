import React from "react"
import ReactDOM from "react-dom/client"
import "@fontsource-variable/inter/opsz.css"
import "./index.css"
import { isTauri } from "@tauri-apps/api/core"

const container = document.getElementById("root")!

if (isTauri()) {
  document.body.dataset.window = "tauri"
  void import("./windows/TauriRecording").then(({ TauriRecording }) => {
    ReactDOM.createRoot(container).render(
      <React.StrictMode>
        <TauriRecording />
      </React.StrictMode>
    )
  })
} else {
  void Promise.all([
    import("./windows/MainWindow"),
    import("./windows/PanelWindow"),
    import("@/lib/hooks"),
    import("@shared/i18n")
  ]).then(([{ MainWindow }, { PanelWindow }, { useSettings }, { setLocale }]) => {
    const windows = { main: MainWindow, chat: PanelWindow }
    const kind = window.location.hash.replace(/^#\/?/, "") === "chat" ? "chat" : "main"
    document.body.dataset.window = kind
    const Root = windows[kind]

    function LocaleRoot() {
      const [settings] = useSettings()
      if (!settings) return null
      setLocale(settings.uiLocale)
      document.documentElement.lang = settings.uiLocale
      return <Root key={settings.uiLocale} />
    }

    ReactDOM.createRoot(container).render(
      <React.StrictMode>
        <LocaleRoot />
      </React.StrictMode>
    )
  })
}
