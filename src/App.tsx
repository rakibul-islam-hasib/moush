import { useCallback, useEffect, useRef, useState } from "react";
import { backend, type FeaturesSnapshot } from "./api";
import { NavPane, useNavPane } from "./components/NavPane";
import { PairingDialog } from "./components/PairingDialog";
import { TitleBar } from "./components/TitleBar";
import { DropOverlay, SendTargetDialog, noTargetReason, targetArg } from "./components/SendFiles";
import { AppProvider, useSettings, useSnapshot } from "./hooks/useApp";
import { useFileDrop } from "./hooks/useFileDrop";
import { useMotionEffect, useSurfaceEffect, useThemeEffect } from "./hooks/useTheme";
import { ToastProvider, useToast } from "./hooks/useToast";
import { dev } from "./lib/dev";
import { peersFor, transferTitle } from "./lib/features";
import { SECTION_OF, type FeatureAnchor, type PageId, type Route } from "./lib/navigation";
import { liveSummary } from "./lib/status";
import { DevicesPage } from "./pages/Devices";
import { DiagnosticsPage } from "./pages/Diagnostics";
import { FeaturesPage } from "./pages/Features";
import { HomePage } from "./pages/Home";
import { Onboarding } from "./pages/Onboarding";
import { SettingsPage } from "./pages/Settings";
import { TransfersPage } from "./pages/Transfers";

/**
 * Announces feature events to screen readers (polite live region) and shows a toast
 * for things that need an answer: incoming transfer requests, finished transfers and
 * received texts. Compares consecutive snapshots; the first snapshot is not announced.
 */
function useFeatureEvents(features: FeaturesSnapshot, onReview: () => void, viewingTransfers: boolean): string {
  const toast = useToast();
  const prev = useRef<FeaturesSnapshot | null>(null);
  const [message, setMessage] = useState("");

  useEffect(() => {
    const before = prev.current;
    prev.current = features;
    if (!before || before.transfers === features.transfers && before.sharedTexts === features.sharedTexts) return;
    const old = new Map(before.transfers.map((t) => [t.id, t]));
    const lines: string[] = [];
    for (const t of features.transfers) {
      const was = old.get(t.id);
      if (was?.state === t.state) continue;
      const name = transferTitle(t);
      if (t.state === "pending" && t.direction === "receive" && !was) {
        lines.push(`${t.peerName} wants to send ${name}.`);
        if (!viewingTransfers) toast.notify("informational", `${t.peerName} wants to send ${name}.`, "Incoming files", { label: "Review", onClick: onReview });
      } else if (was && t.state === "done") {
        lines.push(t.direction === "send" ? `Sent ${name} to ${t.peerName}.` : `Received ${name} from ${t.peerName}.`);
      } else if (was && t.state === "failed") {
        lines.push(`Transfer of ${name} failed${t.error ? `: ${t.error}` : "."}`);
      }
    }
    const known = new Set(before.sharedTexts.map((s) => s.id));
    for (const s of features.sharedTexts) {
      if (!known.has(s.id) && s.direction === "received") lines.push(`${s.peerName} shared ${s.isLink ? "a link" : "a text"}.`);
    }
    if (lines.length > 0) setMessage(lines.join(" "));
  }, [features, onReview, viewingTransfers, toast]);

  return message;
}

function Shell() {
  const { snapshot } = useSnapshot();
  const { settings } = useSettings();
  const toast = useToast();
  const [route, setRoute] = useState<Route>({ page: dev?.page ?? "home", anchor: dev?.expand && dev.expand !== "all" ? dev.expand : undefined });
  const [dropped, setDropped] = useState<string[] | null>(null);
  const contentRef = useRef<HTMLElement>(null);
  const pane = useNavPane(dev?.navCollapsed);

  useThemeEffect(settings.theme);
  useMotionEffect(settings.appearance.motion);
  useSurfaceEffect(snapshot.platform.mica, settings.appearance.transparency);

  const navigate = useCallback((page: PageId, anchor?: FeatureAnchor) => {
    setRoute({ page, anchor });
    if (!anchor) contentRef.current?.scrollTo({ top: 0 });
  }, []);
  const reviewTransfers = useCallback(() => navigate("transfers"), [navigate]);

  // Files dropped anywhere on the window are offered to the connected computers.
  const snapshotRef = useRef(snapshot);
  const settingsRef = useRef(settings);
  snapshotRef.current = snapshot;
  settingsRef.current = settings;
  const onDrop = useCallback(
    (paths: string[]) => {
      const snap = snapshotRef.current;
      const st = settingsRef.current;
      const reason = noTargetReason(snap, st);
      if (reason) {
        toast.notify("warning", reason, "Can't send these files");
        return;
      }
      navigate("transfers");
      const { available } = peersFor(snap, st, "files");
      if (available.length === 1) {
        backend.api.sendFiles(targetArg(snap, available[0].id), paths).catch((e: unknown) => toast.error(e, "Couldn't send the files"));
      } else {
        setDropped(paths);
      }
    },
    [navigate, toast],
  );
  const dragging = useFileDrop(onDrop, snapshot.role !== null) || (dev?.drag ?? false);

  const announcement = useFeatureEvents(snapshot.features, reviewTransfers, route.page === "transfers");

  const pairing = snapshot.pairing.length > 0 && <PairingDialog requests={snapshot.pairing} />;
  // Announces connection changes on every page (the visual status lives on Home).
  const live = (
    <>
      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {liveSummary(snapshot)}
      </div>
      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {announcement}
      </div>
    </>
  );

  if (snapshot.role === null) {
    return (
      <div className="window">
        <TitleBar />
        <div className="window-body">
          <Onboarding onComplete={(role) => navigate(role === "client" ? "devices" : "home")} />
        </div>
        {pairing}
      </div>
    );
  }

  const page = route.page;
  // Sub-pages that share a section (Devices tabs) keep the page frame without a transition.
  const transitionKey = page === "layout" ? "devices" : page;

  return (
    <div className="window">
      <TitleBar nav={{ expanded: pane.expanded, controls: pane.paneId, onToggle: pane.toggle }} />
      <div className="app window-body">
        <NavPane section={SECTION_OF[page]} snapshot={snapshot} onNavigate={navigate} pane={pane} />
        <main className="content" ref={contentRef} aria-label="Content">
          <div className="page-transition" key={transitionKey}>
            {page === "home" && <HomePage onNavigate={navigate} />}
            {(page === "devices" || page === "layout") && <DevicesPage tab={page === "layout" ? "layout" : "devices"} onNavigate={navigate} />}
            {page === "features" && <FeaturesPage onNavigate={navigate} anchor={route.anchor} expandAll={dev?.expand === "all"} />}
            {page === "transfers" && <TransfersPage onNavigate={navigate} />}
            {page === "settings" && <SettingsPage onNavigate={navigate} />}
            {page === "diagnostics" && <DiagnosticsPage onNavigate={navigate} />}
          </div>
        </main>
        {dragging && <DropOverlay snapshot={snapshot} settings={settings} />}
        {dropped && (
          <SendTargetDialog snapshot={snapshot} settings={settings} paths={dropped} onSent={() => undefined} onClose={() => setDropped(null)} />
        )}
        {live}
        {pairing}
      </div>
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
