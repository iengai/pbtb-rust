import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { App } from "./App";
import { AuthProvider } from "./auth/AuthProvider";
import { LocaleProvider } from "./i18n/locale";
import "./styles.css";

// `/pbtb-rust/` on GitHub Pages, `/` in dev; the router wants it without the
// trailing slash.
const basename = import.meta.env.BASE_URL.replace(/\/$/, "");

async function boot() {
  // A dev-only fake of the API and session, for working on the pages without
  // an issuer or a Lambda. The condition is a build-time constant, so the
  // module is not part of a production bundle.
  if (import.meta.env.DEV && import.meta.env.VITE_MOCK_API === "1") {
    const { installMock } = await import("./api/mock");
    installMock();
  }
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <LocaleProvider>
        <AuthProvider>
          <BrowserRouter basename={basename}>
            <App />
          </BrowserRouter>
        </AuthProvider>
      </LocaleProvider>
    </StrictMode>,
  );
}

void boot();
