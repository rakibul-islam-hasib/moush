import { useCallback, useRef, useState } from "react";
import { NavPane } from "./components/NavPane";
import { PairingDialog } from "./components/PairingDialog";
import { AppProvider, useApp } from "./hooks/useApp";
import { useMicaEffect, useThemeEffect } from "./hooks/useTheme";
import { ToastProvider } from "./hooks/useToast";
import type { PageId } from "./lib/navigation";
import { liveSummary } from "./lib/status";
import { DevicesPage } from "./pages/Devices";
import { DiagnosticsPage } from "./pages/Diagnostics";
import { HomePage } from "./pages/Home";
import { LayoutPage } from "./pages/Layout";
import { Onboarding } from "./pages/Onboarding";
import { SettingsPage } from "./pages/Settings";

function Shell() {
  const { snapshot, settings } = useApp();
  const [page, setPage] = useState<PageId>("home");
  const contentRef = useRef<HTMLElement>(null);

  useThemeEffect(settings.theme);
  useMicaEffect(snapshot.platform.mica);

  const navigate = useCallback((next: PageId) => {
    setPage(next);
    contentRef.current?.scrollTo({ top: 0 });
  }, []);

  const pairing = snapshot.pairing.length > 0 && <PairingDialog requests={snapshot.pairing} />;
  // Announces connection changes on every page (the visual status lives on Home).
  const live = (
    <div className="sr-only" aria-live="polite" aria-atomic="true">
      {liveSummary(snapshot)}
    </div>
  );

  if (snapshot.role === null) {
    return (
      <>
        <Onboarding onComplete={(role) => navigate(role === "client" ? "devices" : "home")} />
        {pairing}
      </>
    );
  }

  return (
    <div className="app">
      <NavPane page={page} onNavigate={navigate} />
      <main className="content" ref={contentRef} aria-label="Content">
        {page === "home" && <HomePage onNavigate={navigate} />}
        {page === "devices" && <DevicesPage />}
        {page === "layout" && <LayoutPage />}
        {page === "settings" && <SettingsPage onNavigate={navigate} />}
        {page === "diagnostics" && <DiagnosticsPage />}
      </main>
      {live}
      {pairing}
    </div>
  );
}

export function App() {
  return (
    <ToastProvider>
      <AppProvider>
        <Shell />
      </AppProvider>
    </ToastProvider>
  );
}
