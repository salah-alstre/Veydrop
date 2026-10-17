import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { applyLanguage } from "./i18n";
import { initialLang } from "./lib/language";
import { applyTheme, loadPrefs } from "./lib/prefs";
import App from "./App";

async function boot() {
  const prefs = loadPrefs();
  applyTheme(prefs.theme);
  await applyLanguage(initialLang());
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();
