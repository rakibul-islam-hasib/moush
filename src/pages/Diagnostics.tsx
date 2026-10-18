import { useState } from "react";
import { backend, type Diagnostics, type FirewallState, type LatencyStats, type LatencyTestResult } from "../api";
import { Badge } from "../components/Badge";
import { Button } from "../components/Button";
import { InfoBar } from "../components/InfoBar";
import { LogViewer } from "../components/LogViewer";
import { Page } from "../components/Page";
import { ProgressRing } from "../components/ProgressRing";
import { CardGroup, SettingsCard } from "../components/SettingsCard";
import { useAction } from "../hooks/useAction";
import { useApp } from "../hooks/useApp";
import { useDiagnostics } from "../hooks/useDiagnostics";
import { useToast } from "../hooks/useToast";
import { formatBytes, formatCount, formatDuration, formatMs, formatUs } from "../lib/format";
import type { Tone } from "../lib/status";

const FIREWALL: Record<FirewallState, { label: string; tone: Tone; text: string }> = {
  allowed: { label: "Allowed", tone: "success", text: "Windows Firewall allows Moush on Private networks." },
  missing: {
    label: "Blocked",
    tone: "critical",
    text: "There's no firewall rule for Moush, so other computers may not be able to connect.",
  },
  unknown: { label: "Unknown", tone: "caution", text: "Moush couldn't check the Windows Firewall rules." },
};

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <span className="stat-label">{label}</span>
      <span className="stat-value">{value}</span>
    </div>
  );
}

interface LatencyRow {
  label: string;
  hint: string;
  stats: LatencyStats;
}

function LatencyTable({ caption, rows, format }: { caption: string; rows: LatencyRow[]; format: (v: number) => string }) {
  return (
    <div className="table-card">
      <table className="data-table">
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr>
            <th scope="col">Measurement</th>
            <th scope="col">Samples</th>
            <th scope="col">Median (p50)</th>
            <th scope="col">p95</th>
            <th scope="col">p99</th>
            <th scope="col">Max</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const empty = r.stats.count === 0;
            const cell = (v: number) => (empty ? "—" : format(v));
            return (
              <tr key={r.label}>
                <th scope="row">
                  <span className="table-row-title">{r.label}</span>
                  <span className="table-row-hint">{r.hint}</span>
                </th>
                <td>{formatCount(r.stats.count)}</td>
                <td>{cell(r.stats.p50)}</td>
                <td>{cell(r.stats.p95)}</td>
                <td>{cell(r.stats.p99)}</td>
                <td>{cell(r.stats.max)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function LatencyTest() {
  const toast = useToast();
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<LatencyTestResult | null>(null);

  const start = async () => {
    setRunning(true);
    try {
      setResult(await backend.api.runLatencyTest());
    } catch (e) {
      toast.error(e, "Latency test failed");
    } finally {
      setRunning(false);
    }
  };

  const loss = result && result.sent > 0 ? ((result.sent - result.received) / result.sent) * 100 : 0;

  return (
    <SettingsCard
      icon="pulse"
      title="Latency test"
      description="Sends a burst of test messages to the connected computer and measures the round trip"
      footer={
        result && (
          <div className="latency-result" role="status">
            <Stat label="Computer" value={result.peerName} />
            <Stat label="Received" value={`${result.received} / ${result.sent}${loss > 0 ? ` (${loss.toFixed(1)}% lost)` : ""}`} />
            <Stat label="Median" value={formatMs(result.rttMs.p50)} />
            <Stat label="p95" value={formatMs(result.rttMs.p95)} />
            <Stat label="Max" value={formatMs(result.rttMs.max)} />
          </div>
        )
      }
    >
      <Button icon="play" busy={running} onClick={() => void start()}>
        {running ? "Testing…" : "Run latency test"}
      </Button>
    </SettingsCard>
  );
}

function DiagnosticsBody({ d }: { d: Diagnostics }) {
  const { busy, run } = useAction();
  const fw = FIREWALL[d.firewall];
  const { input, network: net } = d;

  return (
    <>
      <CardGroup title="Status">
        <SettingsCard icon="shield" title="Windows Firewall" description={fw.text}>
          <Badge tone={fw.tone}>{fw.label}</Badge>
          {d.firewall !== "allowed" && (
            <Button variant="accent" busy={busy === "fw"} onClick={() => void run("fw", () => backend.api.openFirewallSetup())}>
              Add firewall rule
            </Button>
          )}
        </SettingsCard>
        <SettingsCard
          icon="lock"
          title="Administrator"
          description={
            d.elevated
              ? "Moush is running as administrator and can control apps that run as administrator."
              : "Apps running as administrator on this computer can't be controlled from another computer unless Moush runs as administrator."
          }
        >
          <Badge tone={d.elevated ? "success" : "neutral"}>{d.elevated ? "Elevated" : "Not elevated"}</Badge>
        </SettingsCard>
      </CardGroup>

      <CardGroup title="Process">
        <div className="stat-grid">
          <Stat label="Uptime" value={formatDuration(d.uptimeSecs)} />
          <Stat label="CPU" value={`${d.process.cpuPercent.toFixed(1)}%`} />
          <Stat label="Memory" value={`${d.process.memoryMb.toFixed(1)} MB`} />
          <Stat label="Threads" value={formatCount(d.process.threads)} />
        </div>
      </CardGroup>

      <CardGroup title="Input">
        <div className="stat-grid">
          <Stat label="Input hook" value={input.hookInstalled ? "Installed" : "Not installed"} />
          <Stat label="Events captured" value={formatCount(input.eventsCaptured)} />
          <Stat label="Events forwarded" value={formatCount(input.eventsForwarded)} />
          <Stat label="Events injected" value={formatCount(input.eventsInjected)} />
          <Stat label="Injection failures" value={formatCount(input.injectFailures)} />
        </div>
        <LatencyTable
          caption="Input latency"
          format={formatUs}
          rows={[
            { label: "Hook", hint: "Host: time inside the input hook", stats: input.hookLatencyUs },
            { label: "Send", hint: "Host: hook to network write", stats: input.sendLatencyUs },
            { label: "Inject", hint: "Client: received to injected", stats: input.injectLatencyUs },
          ]}
        />
      </CardGroup>

      <CardGroup title="Network">
        <div className="stat-grid">
          <Stat label="Sent" value={formatBytes(net.bytesSent)} />
          <Stat label="Received" value={formatBytes(net.bytesReceived)} />
          <Stat label="Frames sent" value={formatCount(net.framesSent)} />
          <Stat label="Frames received" value={formatCount(net.framesReceived)} />
          <Stat label="Reconnects" value={formatCount(net.reconnects)} />
          <Stat label="Last reconnect took" value={net.lastReconnectMs === null ? "—" : formatMs(net.lastReconnectMs)} />
        </div>
        <LatencyTable caption="Network round trip" format={formatMs} rows={[{ label: "Round trip", hint: "Heartbeat round-trip time", stats: net.rttMs }]} />
        <SettingsCard
          icon="wifi"
          title="Network adapters"
          description={
            net.interfaces.length === 0
              ? "No network adapters found."
              : net.interfaces.map((i) => (
                  <span key={`${i.name}-${i.address}`} className="iface">
                    {i.name} <span className="mono">{i.address}</span>
                  </span>
                ))
          }
        />
        <LatencyTest />
      </CardGroup>
    </>
  );
}

export function DiagnosticsPage() {
  const { data, error } = useDiagnostics();
  const { snapshot } = useApp();
  return (
    <Page title="Diagnostics">
      {error && (
        <InfoBar severity="error" title="Couldn't read diagnostics" live>
          {error}
        </InfoBar>
      )}
      {data ? (
        <DiagnosticsBody d={data} />
      ) : (
        !error && (
          <div className="loading-row">
            <ProgressRing size={20} />
            <span className="text-secondary">Collecting diagnostics…</span>
          </div>
        )
      )}
      <CardGroup title="Log" description={`Moush ${snapshot.version} · Windows build ${snapshot.platform.osBuild}`}>
        <LogViewer logPath={data?.logPath ?? null} />
      </CardGroup>
    </Page>
  );
}
