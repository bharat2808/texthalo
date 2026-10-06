import { languageNames, languages as interfaceLanguages } from "./localization";
import KokoroLanguageSetup, { KokoroCjkSetup, type KokoroLanguageInstallEvent } from "./KokoroLanguageSetup";
import { sourceMessage, t, message, useLanguage, currentLanguage } from "./i18n";
import TranscriptionSource from "./TranscriptionSource";
import Row from "./SelectionRow";
import { createPortal } from "react-dom";
import { serviceReadiness } from "./serviceReadiness";
import TranscriptionHistory from "./TranscriptionHistory";
import GenerationHistory from "./GenerationHistory";
import AssistantSettings, { PermissionsSettings, TranscriptionSettings, TaskReadiness, useAvailability, defaultAssistantPreferences, type AssistantPreferences } from "./AssistantSettings";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { openUrl } from "@tauri-apps/plugin-opener";
import KokoroModelPicker, { type KokoroModelOption } from "./KokoroModelPicker";
import SetupWizard, { type SetupPlansResponse } from "./SetupWizard";
import appLogo from "../src-tauri/icons/icon.png";
import "./App.css";

/* ── types mirroring the Rust side ─────────────────────────────────── */

type ShortcutRole = "speak" | "stop" | "transcribe" | "assistant";

const shortcutLabels: Record<ShortcutRole, string> = { speak: "Listen", stop: "Stop", transcribe: "Dictate", assistant: "Assistant" };
const holdKeys: Record<string, string> = { "Hold:Fn": "Hold Fn", "Hold:LeftOption": "Hold Left Option", "Hold:RightOption": "Hold Right Option", "Hold:Escape": "Hold Escape" };
const holdKeySymbols: Record<string, string> = { "Hold:Fn": "fn", "Hold:LeftOption": "⌥", "Hold:RightOption": "⌥", "Hold:Escape": "⎋" };
const shortcutLabel = (key: string) => holdKeys[key] ? t(holdKeys[key]) : key;

type CaptureMode = "ax_then_copy" | "ax_only" | "copy_only";

type Voice = { name: string; locale: string; novelty: boolean };

/** Which synthesis backend is selected. Mirrors the Rust `Engine` enum's wire format. */
type EngineId = "apple" | "kokoro" | "chatterbox" | "fish";

/** One voice offered by whichever engine is active. */
type EngineVoice = {
  id: string;
  label: string;
  language: string;
  requires_espeak?: boolean;
  requires_cjk?: boolean;
  /** Non-null when the engine cannot use this voice at all — shown, not selectable. */
  unavailable: string | null;
  note: string | null;
};

/** One reference clip a cloning engine can speak in. `id` is a file name, not a language. */
type RefVoice = {
  id: string;
  label: string;
  note: string;
  /** The clip that ships with the weights: selectable, never deletable. */
  builtin: boolean;
};

/** An engine, its voices, and — as the Rust side computes it — whether it can speak. */
type EngineInfo = {
  id: EngineId;
  label: string;
  summary: string;
  can_speak: boolean;
  status: string;
  /** Full-sentence reason, used for the failed-shortcut message — never shown in the pane. */
  blocked_reason: string | null;
  needs_download: boolean;
  download_bytes: number;
  repo: string;
  voices: EngineVoice[];
  /** Cloning engines only: the reference clips, built-in first. */
  ref_voices: RefVoice[];
  selected_voice: string;
  models: KokoroModelOption[];
};

type KokoroSettings = {
  voice: string;
  quant: string;
  speed: number;
  keep_warm: boolean;
  idle_unload_minutes: number;
};

/**
 * Chatterbox Multilingual's own section. `voice` is a language code, not a speaker:
 * Chatterbox clones its speaker from a reference clip, so the only thing to choose is
 * which of its languages to read in.
 */
type ChatterboxSettings = {
  quant: string;
  voice: string;
  exaggeration: number;
  cfg_weight: number;
  ref_audio: string | null;
  keep_warm: boolean;
};

type FishSettings = { voice_id: string; model_id: string; enhance_text: boolean; privacy_accepted: boolean };
type HostedVoice = { id: string; name: string; description: string; languageCodes: string[]; tags: string[]; previewAvailable: boolean; samples: { id: string; title: string; text: string }[] };
type HostedVoiceLanguage = { code: string; voiceCount: number };
type HostedAccount = { availableCredits: number; accountEmail?: string | null; creditBreakdown: { totalCredits: number; planCredits: number; topupCredits: number; otherCredits: number; debtCredits?: number }; billingEnabled: boolean; subscription: { planId: string; status: string; currentPeriodEnd: string; cancelAtPeriodEnd: boolean } | null; plans: { id: string; creditsPerPeriod: number; cloneLimit: number }[]; cloneEntitlement?: { limit: number; source: "subscription" | "topup_trial" | "service" | null; expiresAt: string | null } };
type HostedCloneStatus = "created" | "training" | "trained" | "failed";
type HostedClone = { id: string; name: string; status: HostedCloneStatus; createdAt: string; access?: "active" | "disabled" };

const cloneIsPending = (status: HostedCloneStatus) => status === "created" || status === "training";
const subscriptionIsActive = (subscription: HostedAccount["subscription"]): boolean => Boolean(
  subscription && ["active", "trialing"].includes(subscription.status) && new Date(subscription.currentPeriodEnd).getTime() > Date.now(),
);

function formatPlanPrice(price: SetupPlansResponse["plans"][number]["price"]): string {
  if (!price) return t("Pricing unavailable");
  const formatter = new Intl.NumberFormat(currentLanguage(), { style: "currency", currency: price.currency });
  const amount = formatter.format(price.unitAmount / (10 ** (formatter.resolvedOptions().maximumFractionDigits ?? 2)));
  if (!price.interval) return amount;
  const count = price.intervalCount ?? 1;
  return `${amount} / ${count > 1 ? `${count} ` : ""}${t(`${price.interval}${count > 1 ? "s" : ""}`)}`;
}

const websiteUrl = (import.meta.env.VITE_TEXTHALO_WEBSITE_URL || "https://texthalo.app").replace(/\/$/, "");

/** Progress of a weight download, emitted by the Rust side as `kiegen:install`. */
type InstallEvent = {
  engine: EngineId;
  quant?: string | null;
  phase: "downloading" | "done" | "error";
  file: string;
  done: number;
  total: number;
  message: string | null;
};

type AudioHistoryEntry = {
  id: string;
  createdAt: number;
  engine: string;
  voice: string;
  text: string;
  durationSeconds: number | null;
  audioFile: string;
  appName?: string | null;
  windowTitle?: string | null;
  partial?: boolean;
};

type Settings = {
  accessibility_prompted: boolean;
  onboarding_completed: boolean;
  launch_at_login: boolean;
  ui_language?: import("./localization").LanguagePreference;
  shortcuts: { speak: string; stop: string; transcribe: string; assistant: string };
  assistant: AssistantPreferences;
  engine: EngineId;
  kokoro: KokoroSettings;
  chatterbox: ChatterboxSettings;
  fish: FishSettings;
  voice: string | null;
  rate: number;
  capture_mode: CaptureMode;
  max_chars: number;
  restore_clipboard: boolean;
};

type UiState = {
  settings: Settings;
  voices: Voice[];
  engines: EngineInfo[];
  trusted: boolean;
  focused_password_field: boolean;
  speaking: boolean;
  refused_shortcuts: string[];
  system_language: string;
  config_path: string;
};

type Phase = "idle" | "capturing" | "preparing" | "speaking" | "error";
type Status = { phase: Phase; message?: string | null; chars?: number | null };

type Tab = "home" | "general" | "voice" | "account" | "shortcuts" | "capture" | "history" | "permissions" | "transcription" | "assistant";

/* ── icons (16×16, currentColor) ───────────────────────────────────── */

/* Icons: Bootstrap Icons (MIT), 16×16 paths inlined to avoid a runtime dependency. */
function ico(paths: string[], size = 16) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      {paths.map((d, index) => (
        <path key={index} d={d} />
      ))}
    </svg>
  );
}

type UpdateOverlayStatus = {
  visible: boolean;
  version: string | null;
  installable: boolean;
  installing: boolean;
  message: string | null;
  error: boolean;
};

// Persist before broadcasting so opening/reloading a window cannot lose a result.
let updateStatusQueue = Promise.resolve();
function publishUpdateStatus(status: Omit<UpdateOverlayStatus, "visible">) {
  updateStatusQueue = updateStatusQueue.then(() => invoke<void>("publish_update_status", {
    status: { ...status, visible: false },
  })).catch((error) => console.error(t("Could not publish update status"), error));
  return updateStatusQueue;
}

function updateErrorMessage(error: unknown): string {
  const detail = String(error);
  if (/platform.*(not found|were found)/i.test(detail)) {
    return sourceMessage("This release has no compatible update for this build of TextHalo. Please wait for a release supporting your Mac architecture.");
  }
  if (/os error (1|13)\b|permission denied|authentication failed|Failed to move the new app/i.test(detail)) {
    return sourceMessage("macOS could not replace TextHalo. Quit the app and use Finder to install the new version; ask your administrator if prompted. Details: {0}", [detail]);
  }
  if (/os error (18|30)\b|cross-device|read-only/i.test(detail)) {
    return sourceMessage("Copy TextHalo into Applications on your startup disk, eject the installer, and reopen it before updating. Details: {0}", [detail]);
  }
  if (/os error 28\b|no space left/i.test(detail)) {
    return sourceMessage("There is not enough free disk space to install the update. Free some space and try again.");
  }
  return detail;
}

function UpdateResultOverlay() {
  // Paint immediately. The native window can be shown as soon as the tray check
  // finishes, before this webview has completed its first invoke/listener setup.
  const [status, setStatus] = useState<UpdateOverlayStatus>({
    visible: true,
    version: null,
    installable: false,
    installing: false,
    message: "Checking for updates…",
    error: false,
  });

  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    let receivedEvent = false;
    void (async () => {
      off = await listen<UpdateOverlayStatus>("texthalo:update-overlay", (event) => {
        receivedEvent = true;
        if (!disposed) setStatus(event.payload);
      });
      if (disposed) { off(); return; }
      const initial = await invoke<UpdateOverlayStatus>("get_update_overlay_status");
      if (!disposed && !receivedEvent) setStatus(initial);
    })().catch((cause) => {
      if (!disposed) setStatus((current) => ({ ...current, visible: true,
        message: sourceMessage("Could not load update status: {0}", [String(cause)]), error: true }));
    });
    return () => { disposed = true; off?.(); };
  }, []);

  const close = () => void invoke("close_update_overlay");
  if (!status.visible) return null;

  const hasUpdate = Boolean(status.version);
  const checking = status.message === "Checking for updates…";
  return (
    <main className="update-overlay" role="dialog" aria-labelledby="update-title" aria-live="polite">
      <button className="update-overlay-close" onClick={close} aria-label={t("Close")}>×</button>
      <img className="update-overlay-mark" src={appLogo} alt="" />
      <h1 id="update-title">
        {checking ? t("Checking for updates…") : status.error ? t("Update needs attention") : hasUpdate ? t("TextHalo {0} is available", [status.version]) : t("You’re up to date")}
      </h1>
      <p className={status.error ? "update-overlay-message error" : "update-overlay-message"}>
        {message(status.message ?? (hasUpdate
          ? status.installing ? t("Preparing to install…") : t("A newer version is ready to install.")
          : status.error ? t("Please try again in a moment.") : t("You have the latest version of TextHalo.")))}
      </p>
      <div className="update-overlay-actions">
        {hasUpdate ? (
          <button
            className="update-overlay-install"
            disabled={!status.installable || status.installing}
            onClick={() => void emit("texthalo:install-update-requested")}
          >
            {status.installing ? t("Installing…") : status.installable ? t("Install {0}", [status.version]) : status.error ? t("Installation unavailable") : t("Preparing…")}
          </button>
        ) : null}
        <button className="update-overlay-later" onClick={close} disabled={status.installing}>
          {hasUpdate ? t("Later") : t("Done")}
        </button>
      </div>
    </main>
  );
}

export default function App() {
  useLanguage();
  const isUpdateOverlay = new URLSearchParams(window.location.search).has("update-overlay");
  return isUpdateOverlay ? <UpdateResultOverlay /> : <MainApp />;
}

const Icon = {
  sparkles: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m6 3 1.5 4.5L12 9l-4.5 1.5L6 15l-1.5-4.5L0 9l4.5-1.5L6 3Z" transform="translate(1 -1) scale(.9)" />
      <path d="M12.5 1v4M10.5 3h4" />
    </svg>
  ),
  person: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" aria-hidden="true">
      <circle cx="8" cy="4.75" r="2.75" />
      <path d="M2.5 14v-1a5.5 4 0 0 1 11 0v1" />
    </svg>
  ),
  gear: () =>
    ico([
      "M8 4.754a3.246 3.246 0 1 0 0 6.492 3.246 3.246 0 0 0 0-6.492M5.754 8a2.246 2.246 0 1 1 4.492 0 2.246 2.246 0 0 1-4.492 0",
      "M9.796 1.343c-.527-1.79-3.065-1.79-3.592 0l-.094.319a.873.873 0 0 1-1.255.52l-.292-.16c-1.64-.892-3.433.902-2.54 2.541l.159.292a.873.873 0 0 1-.52 1.255l-.319.094c-1.79.527-1.79 3.065 0 3.592l.319.094a.873.873 0 0 1 .52 1.255l-.16.292c-.892 1.64.901 3.434 2.541 2.54l.292-.159a.873.873 0 0 1 1.255.52l.094.319c.527 1.79 3.065 1.79 3.592 0l.094-.319a.873.873 0 0 1 1.255-.52l.292.16c1.64.893 3.434-.902 2.54-2.541l-.159-.292a.873.873 0 0 1 .52-1.255l.319-.094c1.79-.527 1.79-3.065 0-3.592l-.319-.094a.873.873 0 0 1-.52-1.255l.16-.292c.893-1.64-.902-3.433-2.541-2.54l-.292.159a.873.873 0 0 1-1.255-.52zm-2.633.283c.246-.835 1.428-.835 1.674 0l.094.319a1.873 1.873 0 0 0 2.693 1.115l.291-.16c.764-.415 1.6.42 1.184 1.185l-.159.292a1.873 1.873 0 0 0 1.116 2.692l.318.094c.835.246.835 1.428 0 1.674l-.319.094a1.873 1.873 0 0 0-1.115 2.693l.16.291c.415.764-.42 1.6-1.185 1.184l-.291-.159a1.873 1.873 0 0 0-2.693 1.116l-.094.318c-.246.835-1.428.835-1.674 0l-.094-.319a1.873 1.873 0 0 0-2.692-1.115l-.292.16c-.764.415-1.6-.42-1.184-1.185l.159-.291A1.873 1.873 0 0 0 1.945 8.93l-.319-.094c-.835-.246-.835-1.428 0-1.674l.319-.094A1.873 1.873 0 0 0 3.06 4.377l-.16-.292c-.415-.764.42-1.6 1.185-1.184l.292.159a1.873 1.873 0 0 0 2.692-1.115z",
    ]),
  speaker: () =>
    ico([
      "M11.536 14.01A8.47 8.47 0 0 0 14.026 8a8.47 8.47 0 0 0-2.49-6.01l-.708.707A7.48 7.48 0 0 1 13.025 8c0 2.071-.84 3.946-2.197 5.303z",
      "M10.121 12.596A6.48 6.48 0 0 0 12.025 8a6.48 6.48 0 0 0-1.904-4.596l-.707.707A5.48 5.48 0 0 1 11.025 8a5.48 5.48 0 0 1-1.61 3.89z",
      "M10.025 8a4.5 4.5 0 0 1-1.318 3.182L8 10.475A3.5 3.5 0 0 0 9.025 8c0-.966-.392-1.841-1.025-2.475l.707-.707A4.5 4.5 0 0 1 10.025 8M7 4a.5.5 0 0 0-.812-.39L3.825 5.5H1.5A.5.5 0 0 0 1 6v4a.5.5 0 0 0 .5.5h2.325l2.363 1.89A.5.5 0 0 0 7 12zM4.312 6.39 6 5.04v5.92L4.312 9.61A.5.5 0 0 0 4 9.5H2v-3h2a.5.5 0 0 0 .312-.11",
    ]),
  history: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M3.1 4.8A5.55 5.55 0 1 1 2.5 8M2.3 2.8v3.1h3.1M8 4.8V8l2.2 1.4"
        stroke="currentColor"
        strokeWidth="1.45"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  ),
  command: () =>
    ico([
      "M3.5 2A1.5 1.5 0 0 1 5 3.5V5H3.5a1.5 1.5 0 1 1 0-3M6 5V3.5A2.5 2.5 0 1 0 3.5 6H5v4H3.5A2.5 2.5 0 1 0 6 12.5V11h4v1.5a2.5 2.5 0 1 0 2.5-2.5H11V6h1.5A2.5 2.5 0 1 0 10 3.5V5zm4 1v4H6V6zm1-1V3.5A1.5 1.5 0 1 1 12.5 5zm0 6h1.5a1.5 1.5 0 1 1-1.5 1.5zm-6 0v1.5A1.5 1.5 0 1 1 3.5 11z",
    ]),
  textCursor: () =>
    ico([
      "M5 2a.5.5 0 0 1 .5-.5c.862 0 1.573.287 2.06.566.174.099.321.198.44.286.119-.088.266-.187.44-.286A4.17 4.17 0 0 1 10.5 1.5a.5.5 0 0 1 0 1c-.638 0-1.177.213-1.564.434a3.5 3.5 0 0 0-.436.294V7.5H9a.5.5 0 0 1 0 1h-.5v4.272c.1.08.248.187.436.294.387.221.926.434 1.564.434a.5.5 0 0 1 0 1 4.17 4.17 0 0 1-2.06-.566A5 5 0 0 1 8 13.65a5 5 0 0 1-.44.285 4.17 4.17 0 0 1-2.06.566.5.5 0 0 1 0-1c.638 0 1.177-.213 1.564-.434.188-.107.335-.214.436-.294V8.5H7a.5.5 0 0 1 0-1h.5V3.228a3.5 3.5 0 0 0-.436-.294A3.17 3.17 0 0 0 5.5 2.5.5.5 0 0 1 5 2m2.648 10.645",
    ]),
  checkCircle: () =>
    ico([
      "M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14m0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16",
      "m10.97 4.97-.02.022-3.473 4.425-2.093-2.094a.75.75 0 0 0-1.06 1.06L6.97 11.03a.75.75 0 0 0 1.079-.02l3.992-4.99a.75.75 0 0 0-1.071-1.05",
    ]),
  circle: () => ico(["M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14m0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16"]),
  warn: () =>
    ico([
      "M7.938 2.016A.13.13 0 0 1 8.002 2a.13.13 0 0 1 .063.016.15.15 0 0 1 .054.057l6.857 11.667c.036.06.035.124.002.183a.2.2 0 0 1-.054.06.1.1 0 0 1-.066.017H1.146a.1.1 0 0 1-.066-.017.2.2 0 0 1-.054-.06.18.18 0 0 1 .002-.183L7.884 2.073a.15.15 0 0 1 .054-.057m1.044-.45a1.13 1.13 0 0 0-1.96 0L.165 13.233c-.457.778.091 1.767.98 1.767h13.713c.889 0 1.438-.99.98-1.767z",
      "M7.002 12a1 1 0 1 1 2 0 1 1 0 0 1-2 0M7.1 5.995a.905.905 0 1 1 1.8 0l-.35 3.507a.552.552 0 0 1-1.1 0z",
    ]),
  xCircle: () =>
    ico([
      "M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14m0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16",
      "M4.646 4.646a.5.5 0 0 1 .708 0L8 7.293l2.646-2.647a.5.5 0 0 1 .708.708L8.707 8l2.647 2.646a.5.5 0 0 1-.708.708L8 8.707l-2.646 2.647a.5.5 0 0 1-.708-.708L7.293 8 4.646 5.354a.5.5 0 0 1 0-.708",
    ]),
  info: () =>
    ico([
      "M8 15A7 7 0 1 1 8 1a7 7 0 0 1 0 14m0 1A8 8 0 1 0 8 0a8 8 0 0 0 0 16",
      "m8.93 6.588-2.29.287-.082.38.45.083c.294.07.352.176.288.469l-.738 3.468c-.194.897.105 1.319.808 1.319.545 0 1.178-.252 1.465-.598l.088-.416c-.2.176-.492.246-.686.246-.275 0-.375-.193-.304-.533zM9 4.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0",
    ]),
  keyboard: () =>
    ico([
      "M14 5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM2 4a2 2 0 0 0-2 2v5a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2z",
      "M13 10.25a.25.25 0 0 1 .25-.25h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5a.25.25 0 0 1-.25-.25zm0-2a.25.25 0 0 1 .25-.25h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5a.25.25 0 0 1-.25-.25zm-5 0A.25.25 0 0 1 8.25 8h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 8 8.75zm2 0a.25.25 0 0 1 .25-.25h1.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-1.5a.25.25 0 0 1-.25-.25zm1 2a.25.25 0 0 1 .25-.25h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5a.25.25 0 0 1-.25-.25zm-5-2A.25.25 0 0 1 6.25 8h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 6 8.75zm-2 0A.25.25 0 0 1 4.25 8h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 4 8.75zm-2 0A.25.25 0 0 1 2.25 8h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 2 8.75zm11-2a.25.25 0 0 1 .25-.25h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5a.25.25 0 0 1-.25-.25zm-2 0a.25.25 0 0 1 .25-.25h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5a.25.25 0 0 1-.25-.25zm-2 0A.25.25 0 0 1 9.25 6h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 9 6.75zm-2 0A.25.25 0 0 1 7.25 6h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 7 6.75zm-2 0A.25.25 0 0 1 5.25 6h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5A.25.25 0 0 1 5 6.75zm-3 0A.25.25 0 0 1 2.25 6h1.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-1.5A.25.25 0 0 1 2 6.75zm0 4a.25.25 0 0 1 .25-.25h.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-.5a.25.25 0 0 1-.25-.25zm2 0a.25.25 0 0 1 .25-.25h5.5a.25.25 0 0 1 .25.25v.5a.25.25 0 0 1-.25.25h-5.5a.25.25 0 0 1-.25-.25z",
    ]),
  play: () =>
    ico([
      "m11.596 8.697-6.363 3.692c-.54.313-1.233-.066-1.233-.697V4.308c0-.63.692-1.01 1.233-.696l6.363 3.692a.802.802 0 0 1 0 1.393",
    ]),
  gauge: () =>
    ico([
      "M8 4a.5.5 0 0 1 .5.5V6a.5.5 0 0 1-1 0V4.5A.5.5 0 0 1 8 4M3.732 5.732a.5.5 0 0 1 .707 0l.915.914a.5.5 0 1 1-.708.708l-.914-.915a.5.5 0 0 1 0-.707M2 10a.5.5 0 0 1 .5-.5h1.586a.5.5 0 0 1 0 1H2.5A.5.5 0 0 1 2 10m9.5 0a.5.5 0 0 1 .5-.5h1.5a.5.5 0 0 1 0 1H12a.5.5 0 0 1-.5-.5m.754-4.246a.39.39 0 0 0-.527-.02L7.547 9.31a.91.91 0 1 0 1.302 1.258l3.434-4.297a.39.39 0 0 0-.029-.518z",
      "M0 10a8 8 0 1 1 15.547 2.661c-.442 1.253-1.845 1.602-2.932 1.25C11.309 13.488 9.475 13 8 13c-1.474 0-3.31.488-4.615.911-1.087.352-2.49.003-2.932-1.25A8 8 0 0 1 0 10m8-7a7 7 0 0 0-6.603 9.329c.203.575.923.876 1.68.63C4.397 12.533 6.358 12 8 12s3.604.532 4.923.96c.757.245 1.477-.056 1.68-.631A7 7 0 0 0 8 3",
    ]),
  download: () =>
    ico([
      "M.5 9.9a.5.5 0 0 1 .5.5v2.5a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-2.5a.5.5 0 0 1 1 0v2.5a2 2 0 0 1-2 2H2a2 2 0 0 1-2-2v-2.5a.5.5 0 0 1 .5-.5",
      "M7.646 11.854a.5.5 0 0 0 .708 0l3-3a.5.5 0 0 0-.708-.708L8.5 10.293V1.5a.5.5 0 0 0-1 0v8.793L5.354 8.146a.5.5 0 1 0-.708.708z",
    ]),
  box: () =>
    ico([
      "M8.186 1.113a.5.5 0 0 0-.372 0L1.846 3.5l2.404.961L10.404 2zm3.564 1.426L5.596 5 8 5.961 14.154 3.5zm3.25 1.7-6.5 2.6v7.922l6.5-2.6V4.24zM7.5 14.762V6.838L1 4.239v7.923zM7.443.184a1.5 1.5 0 0 1 1.114 0l7.129 2.852A.5.5 0 0 1 16 3.5v8.662a1 1 0 0 1-.629.928l-7.185 2.874a.5.5 0 0 1-.372 0L.63 13.09a1 1 0 0 1-.63-.928V3.5a.5.5 0 0 1 .314-.464z",
    ]),
};

/* ── language naming ───────────────────────────────────────────────── */

const languageName = (locale: string) => {
  try { return new Intl.DisplayNames([currentLanguage()], { type: "language" }).of(locale.replace(/_/g, "-")) ?? locale; }
  catch { return locale; }
};
const hostedLanguageName = languageName;
const familyName = (family: string) => languageName(family);

/** `Eddy (English (US))` → `Eddy`: the locale column already says the language. */
const voiceLabel = (name: string) => name.split(" (")[0].trim();

/** Decimal units: these are download sizes for humans, not disk blocks. */
function formatBytes(bytes: number): string {
  if (bytes <= 0) return "nothing";
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} kB`;
  if (bytes < 1_000_000_000) return `${Math.round(bytes / 1_000_000)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/* ── shortcut capture ──────────────────────────────────────────────── */

const NAMED_KEYS: Record<string, string> = {
  Backquote: "Backquote",
  Backslash: "Backslash",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Comma: "Comma",
  Equal: "Equal",
  Minus: "Minus",
  Period: "Period",
  Quote: "Quote",
  Semicolon: "Semicolon",
  Slash: "Slash",
  Space: "Space",
  Tab: "Tab",
  Enter: "Enter",
  Backspace: "Backspace",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Insert: "Insert",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/**
 * Build an accelerator in `global_hotkey` syntax from a DOM event. The vocabulary is
 * that crate's `parse_key`/`parse_hotkey` — bare `S`/`1`/`F5`, arrows as `Up`/`Down` —
 * not the browser's `event.key`. Combinations need a modifier, or the chord would
 * swallow ordinary typing system-wide; function keys are allowed bare.
 */
function acceleratorFromEvent(event: KeyboardEvent): string | null {
  const key = (() => {
    if (NAMED_KEYS[event.code]) return NAMED_KEYS[event.code];
    const letter = /^Key([A-Z])$/.exec(event.code);
    if (letter) return letter[1];
    const digit = /^Digit([0-9])$/.exec(event.code);
    if (digit) return digit[1];
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.code)) return event.code;
    return null;
  })();
  if (!key) return null;

  const parts: string[] = [];
  if (event.metaKey) parts.push("Cmd");
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");

  const isFunctionKey = /^F([1-9]|1[0-9]|2[0-4])$/.test(key);
  if (parts.length === 0 && !isFunctionKey) return null;

  parts.push(key);
  return parts.join("+");
}

/* ── shared components ─────────────────────────────────────────────── */

function Card({
  title,
  icon,
  children,
}: {
  title: string;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="card">
      <div className="card-title">
        {icon}
        <span>{title}</span>
      </div>
      {children}
    </div>
  );
}


function Note({
  kind,
  icon,
  children,
}: {
  kind: "error" | "warning" | "info" | "secondary";
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className={`note-line ${kind}`}>
      {icon}
      <span>{children}</span>
    </div>
  );
}

/* ── app ───────────────────────────────────────────────────────────── */

function MainApp() {
  const uiLanguage = useLanguage();
  const serviceAvailability = useAvailability();
  const [serviceTooltip, setServiceTooltip] = useState<{ id: string; x: number; y: number } | null>(null);
  const [state, setState] = useState<UiState | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [historyView, setHistoryView] = useState<"audio" | "transcripts" | "assistant">("audio");
  const [setupOpen, setSetupOpen] = useState(false);
  const [setupRevision, setSetupRevision] = useState(0);
  const accessibilityPromptStarted = useRef(false);
  const [languageInstall, setLanguageInstall] = useState<KokoroLanguageInstallEvent | null>(null);
  const [languageInstalling, setLanguageInstalling] = useState(false);
  const [tab, setTab] = useState<Tab>("home");
  const [status, setStatus] = useState<Status>({ phase: "idle" });
  const [error, setError] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState<Update | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [language, setLanguage] = useState<string | null>(null);
  const [showNovelty, setShowNovelty] = useState(false);
  const [shortcutFeedback, setShortcutFeedback] = useState<{ role: ShortcutRole; message: string; error: boolean } | null>(null);
  const [shortcutSaving, setShortcutSaving] = useState(false);
  const [recording, setRecording] = useState<ShortcutRole | null>(null);
  const [rateDraft, setRateDraft] = useState<number | null>(null);
  const [install, setInstall] = useState<InstallEvent | null>(null);
  const [installStarting, setInstallStarting] = useState(false);
  const [modelDeleting, setModelDeleting] = useState(false);
  const [advancedVoiceOpen, setAdvancedVoiceOpen] = useState(false);
  // The last word from adding or deleting a reference voice. Kept rather than timed out: it is
  // the only answer the user gets, and a refusal here is a reason, not a transient toast.
  const [voiceMessage, setVoiceMessage] = useState<string | null>(null);
  const [historyEntries, setHistoryEntries] = useState<AudioHistoryEntry[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const historyGroups = historyEntries.reduce((groups, entry) => {
    const app = entry.appName?.trim() || t("Unknown app");
    const title = entry.windowTitle?.trim() || "";
    const titles = groups.get(app) ?? new Map<string, AudioHistoryEntry[]>();
    const entries = titles.get(title) ?? [];
    entries.push(entry);
    titles.set(title, entries);
    groups.set(app, titles);
    return groups;
  }, new Map<string, Map<string, AudioHistoryEntry[]>>());
  const [account, setAccount] = useState<HostedAccount | null>(null);
  const [accountPlans, setAccountPlans] = useState<SetupPlansResponse["plans"] | null>(null);
  const [accountPlansError, setAccountPlansError] = useState(false);
  const [accountPlansLoading, setAccountPlansLoading] = useState(false);
  const [accountEmail, setAccountEmail] = useState("");
  const [accountBusy, setAccountBusy] = useState(false);
  const [accountError, setAccountError] = useState("");
  const [hostedVoices, setHostedVoices] = useState<HostedVoice[]>([]);
  const [hostedLanguages, setHostedLanguages] = useState<HostedVoiceLanguage[]>([]);
  const [hostedLanguage, setHostedLanguage] = useState("en");
  const hostedVoicesByLanguage = useMemo(() => {
    const grouped = new Map<string, HostedVoice[]>();
    for (const voice of hostedVoices) {
      const codes = hostedLanguage === "all" ? voice.languageCodes : [hostedLanguage];
      for (const code of codes) {
        const voices = grouped.get(code) ?? [];
        if (!voices.some((item) => item.id === voice.id)) voices.push(voice);
        grouped.set(code, voices);
      }
    }
    return [...grouped.entries()].sort(([a], [b]) => hostedLanguageName(a).localeCompare(hostedLanguageName(b)));
  }, [hostedVoices, hostedLanguage, uiLanguage]);
  const [hostedQuery, setHostedQuery] = useState("");
  const [hostedLoading, setHostedLoading] = useState(false);
  const hostedLoadRevision = useRef(0);
  const [hostedClones, setHostedClones] = useState<HostedClone[]>([]);
  const pendingCloneIds = hostedClones.filter((clone) => cloneIsPending(clone.status)).map((clone) => clone.id);
  const pendingCloneKey = pendingCloneIds.join("\u0000");
  const [cloneName, setCloneName] = useState("");
  const [cloneUploadError, setCloneUploadError] = useState("");
  const [cloneUploadSuccess, setCloneUploadSuccess] = useState("");
  const [clonePath, setClonePath] = useState("");
  const [cloneConsent, setCloneConsent] = useState(false);
  const hostedQueryRef = useRef("");
  const rateTimer = useRef<number | null>(null);
  const latestState = useRef(state);
  const saveTail = useRef<Promise<void>>(Promise.resolve());
  const saveRevision = useRef(0);
  const pendingSaves = useRef(0);
  const updateDownload = useRef({ downloaded: 0, total: 0 });
  const updateBusy = useRef(false);
  const checkBusy = useRef(false);

  useEffect(() => {
    void getVersion().then(setAppVersion).catch(() => setAppVersion("unknown"));
  }, []);

  const loadAccountPlans = useCallback(async () => {
    setAccountPlansLoading(true);
    setAccountPlansError(false);
    try {
      const result = await invoke<SetupPlansResponse>("desktop_billing_plans");
      setAccountPlans(result.plans);
    } catch {
      setAccountPlansError(true);
    } finally {
      setAccountPlansLoading(false);
    }
  }, []);

  useEffect(() => {
    if (tab === "account" && !accountPlans && !accountPlansLoading && !accountPlansError) void loadAccountPlans();
  }, [accountPlans, accountPlansError, accountPlansLoading, loadAccountPlans, tab]);

  const refreshHostedAccount = useCallback(async () => {
    setAccountError("");
    try {
      if (!(await invoke<boolean>("desktop_is_signed_in"))) {
        setAccount(null);
        setAccountEmail("");
        return;
      }
      const next = await invoke<HostedAccount>("desktop_account");
      setAccount(next);
      setAccountEmail(next.accountEmail ?? "");
    } catch (cause) {
      setAccount(null);
      setAccountError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => { void refreshHostedAccount(); }, [refreshHostedAccount]);

  useEffect(() => {
    const off = listen("texthalo:assistant-usage", () => { void refreshHostedAccount(); });
    return () => { void off.then(unlisten => unlisten()); };
  }, [refreshHostedAccount]);

  useEffect(() => {
    if (!account) return;
    const refreshTimer = window.setInterval(() => void refreshHostedAccount(), 3 * 60 * 1000);
    const expiry = account.cloneEntitlement?.expiresAt ? new Date(account.cloneEntitlement.expiresAt).getTime() : 0;
    const expiryTimer = expiry > Date.now()
      ? window.setTimeout(() => void refreshHostedAccount(), Math.min(2_147_483_647, expiry - Date.now() + 250))
      : undefined;
    return () => { window.clearInterval(refreshTimer); if (expiryTimer !== undefined) window.clearTimeout(expiryTimer); };
  }, [account, refreshHostedAccount]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<Status>("kiegen:status", (event) => {
      if ((event.payload.phase === "idle" || event.payload.phase === "error") && latestState.current?.settings.engine === "fish") {
        void refreshHostedAccount();
      }
    }).then((off) => { unlisten = off; });
    return () => unlisten?.();
  }, [refreshHostedAccount]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<{ success: boolean; message?: string }>("texthalo:desktop-auth", (event) => {
      if (event.payload.success) {
        setAccountError("");
        void refreshHostedAccount();
      } else if (event.payload.message === "Sign-in cancelled.") {
        setAccountError("");
      } else if (event.payload.message) setAccountError(event.payload.message);
      setAccountBusy(false);
    }).then((off) => { unlisten = off; });
    return () => unlisten?.();
  }, [refreshHostedAccount]);

  const refreshHistory = useCallback(async () => {
    setHistoryLoading(true);
    try {
      setHistoryEntries(await invoke<AudioHistoryEntry[]>("get_audio_history"));
      setHistoryError(null);
    } catch (e) {
      setHistoryError(String(e));
    } finally {
      setHistoryLoading(false);
    }
  }, []);

  useEffect(() => {
    if (tab === "history") void refreshHistory();
  }, [tab, refreshHistory]);

  const deleteHistoryEntry = async (id: string) => {
    try {
      await invoke("delete_audio_history", { id });
      setHistoryEntries((entries) => entries.filter((entry) => entry.id !== id));
      setHistoryError(null);
    } catch (e) {
      setHistoryError(String(e));
    }
  };

  const clearHistory = async () => {
    try {
      await invoke("clear_audio_history");
      setHistoryEntries([]);
      setHistoryError(null);
    } catch (e) {
      setHistoryError(String(e));
    }
  };

  const checkForUpdates = useCallback(async () => {
    if (updateBusy.current || checkBusy.current) return;
    checkBusy.current = true;
    setCheckingUpdate(true);
    setUpdateMessage(sourceMessage("Checking for updates…"));
    setUpdateAvailable(null);
    try {
      const update = await check();
      let issue: string | null = null;
      if (update) {
        try { await invoke("check_update_installation"); }
        catch (e) {
          issue = updateErrorMessage(e);
          void invoke("record_update_failure", { stage: "preflight", detail: String(e) }).catch(console.error);
        }
      }
      setUpdateAvailable(issue ? null : update);
      const message = issue ?? (update ? sourceMessage("Version {0} is available.", [update.version]) : sourceMessage("You're up to date."));
      setUpdateMessage(message);
      await publishUpdateStatus({ version: update?.version ?? null, installable: Boolean(update) && !issue,
        installing: false, message, error: Boolean(issue) });
      void invoke("set_update_menu_status_from_settings", {
        version: update?.version ?? null,
      }).catch(() => {});
    } catch (e) {
      const message = updateErrorMessage(e);
      setUpdateMessage(message);
      void invoke("record_update_failure", { stage: "check", detail: String(e) }).catch(console.error);
      await publishUpdateStatus({ version: null, installable: false, installing: false, message, error: true });
    } finally {
      checkBusy.current = false;
      setCheckingUpdate(false);
    }
  }, []);

  const installAvailableUpdate = useCallback(async () => {
    if (!updateAvailable || updateBusy.current) return;
    updateBusy.current = true;
    setInstallingUpdate(true);
    let stage = "preflight";
    let prepared = false;
    const report = (message: string, error = false, installing = true) => {
      setUpdateMessage(message);
      return publishUpdateStatus({ version: updateAvailable.version, installable: !installing && stage !== "relaunch",
        installing, message, error });
    };
    await report(sourceMessage("Checking installation permissions…"));
    try {
      await invoke("prepare_update_installation");
      prepared = true;
      stage = "download";
      await updateAvailable.downloadAndInstall((event) => {
        if (event.event === "Started") {
          updateDownload.current = { downloaded: 0, total: event.data.contentLength ?? 0 };
          void report(sourceMessage("Downloading update…"));
        } else if (event.event === "Progress") {
          updateDownload.current.downloaded += event.data.chunkLength;
          const { downloaded, total } = updateDownload.current;
          void report(
            total
              ? sourceMessage("Downloading update ({0} of {1})…", [formatBytes(downloaded), formatBytes(total)])
              : sourceMessage("Downloaded {0}…", [formatBytes(downloaded)]),
          );
        } else {
          stage = "install";
          void report(sourceMessage("Installing update…"));
        }
      });
      prepared = false;
      await invoke("finish_update_installation", { success: true }).catch(console.error);
      stage = "relaunch";
      await report(sourceMessage("Update installed. Restarting TextHalo…"));
      await relaunch();
    } catch (e) {
      const recovery = prepared
        ? await invoke<string>("finish_update_installation", { success: false }).catch(String)
        : "";
      void invoke("record_update_failure", { stage, detail: String(e) }).catch(console.error);
      await report(stage === "relaunch" ? sourceMessage("The update was installed, but TextHalo could not restart. Quit and reopen TextHalo from Applications.") : `${updateErrorMessage(e)} ${recovery}`.trim(), true, false);
      if (stage === "relaunch") setUpdateAvailable(null);
    } finally {
      updateBusy.current = false;
      setInstallingUpdate(false);
    }
  }, [updateAvailable]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen("texthalo:open-update-settings", () => {
      setTab("general");
      void checkForUpdates();
    }).then((off) => { unlisten = off; });
    return () => unlisten?.();
  }, [checkForUpdates]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<{ version: string | null }>(
      "texthalo:background-update-status",
      (event) => {
        if (updateBusy.current) return;
        if (event.payload.version) {
          setUpdateMessage(sourceMessage("Version {0} is available.", [event.payload.version]));
          void checkForUpdates(); // Load the installable Update object for the About panel.
        } else {
          setUpdateAvailable(null);
          setUpdateMessage(sourceMessage("You're up to date."));
          void publishUpdateStatus({ version: null, installable: false, installing: false,
            message: sourceMessage("You're up to date."), error: false });
        }
      },
    ).then((off) => { unlisten = off; });
    return () => unlisten?.();
  }, [checkForUpdates]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen("texthalo:install-update-requested", () => void installAvailableUpdate())
      .then((off) => { unlisten = off; });
    return () => unlisten?.();
  }, [installAvailableUpdate]);

  const refresh = useCallback(async () => {
    if (pendingSaves.current > 0) return;
    const revision = saveRevision.current;
    try {
      const next = await invoke<UiState>("get_state");
      if (pendingSaves.current === 0 && revision === saveRevision.current) {
        latestState.current = next;
        setState(next);
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const requestAccessibility = useCallback(async () => {
    if (accessibilityPromptStarted.current) return;
    accessibilityPromptStarted.current = true;
    try {
      const next = await invoke<UiState>("request_accessibility");
      latestState.current = next;
      setState(next);
    } catch (cause) {
      accessibilityPromptStarted.current = false;
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = listen<Status>("kiegen:status", (event) => setStatus(event.payload));
    // Downloads report their own progress. When one ends, re-read the catalogue — whether
    // an engine is installed is exactly what its badge shows.
    const uninstall = listen<InstallEvent>("kiegen:install", (event) => {
      setInstall(event.payload);
      if (event.payload.phase !== "downloading") void refresh();
    });
    const languageSetup = listen<KokoroLanguageInstallEvent>("kiegen:kokoro-languages", event => {
      setLanguageInstall(event.payload);
      const busy = event.payload.phase === "runtime" || event.payload.phase === "voices";
      setLanguageInstalling(busy);
      if (!busy) void refresh();
    });
    const recovery = listen<"account" | "voice" | "assistant">("texthalo:speech-recovery", (event) => {
      setTab(event.payload);
      void refresh();
    });
    const setupWizard = listen("texthalo:open-setup", () => {
      setSetupOpen(true);
      setSetupRevision((revision) => revision + 1);
    });
    // Permission is granted outside the app, and speech ends on its own: poll rather
    // than pretend we can observe either.
    const poll = window.setInterval(() => void refresh(), 2000);
    return () => {
      void unlisten.then((off) => off());
      void uninstall.then((off) => off());
      void languageSetup.then((off) => off());
      void setupWizard.then((off) => off());
      void recovery.then((off) => off());
      window.clearInterval(poll);
    };
  }, [refresh]);

  const save = useCallback(
    async (patch: Partial<Settings>) => {
      const current = latestState.current;
      if (!current) return false;
      const next: Settings = { ...current.settings, ...patch };
      latestState.current = { ...current, settings: next };
      // Finishing setup waits for persistence before replacing the wizard.
      if (!patch.onboarding_completed) setState({ ...current, settings: next });
      const revision = ++saveRevision.current;
      pendingSaves.current++;
      const request = saveTail.current.then(() => invoke<UiState>("save_settings", { settings: next }));
      saveTail.current = request.then(() => {}, () => {});
      try {
        const updated = await request;
        if (revision === saveRevision.current) {
          latestState.current = updated;
          setState(updated);
          setError(null);
        }
        return true;
      } catch (e) {
        setError(String(e));
        if (revision === saveRevision.current) latestState.current = current;
        return false;
      } finally {
        pendingSaves.current--;
        if (pendingSaves.current === 0) void refresh();
      }
    },
    [refresh],
  );

  const loadHostedVoices = useCallback(async (query: string, language: string) => {
    const revision = ++hostedLoadRevision.current;
    setHostedLoading(true);
    try {
      let page = 1;
      let voices: HostedVoice[] = [];
      let hasMore = true;
      while (hasMore) {
        const result = await invoke<{ items: HostedVoice[]; hasMore: boolean; modelId: string }>("desktop_voices", { query, language, page });
        if (revision !== hostedLoadRevision.current) return;
        voices = page === 1 ? result.items : [...voices, ...result.items];
        setHostedVoices(voices);
        if (page === 1 && result.modelId && latestState.current?.settings.fish.model_id !== result.modelId) {
          const current = latestState.current!.settings;
          void save({ fish: { ...current.fish, model_id: result.modelId } });
        }
        hasMore = result.hasMore && result.items.length > 0;
        page++;
      }
      setAccountError("");
    } catch (cause) {
      if (revision === hostedLoadRevision.current) setAccountError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (revision === hostedLoadRevision.current) setHostedLoading(false);
    }
  }, [save]);

  useEffect(() => {
    if (tab === "voice" && state?.settings.engine === "fish") {
      void invoke<{items: HostedVoiceLanguage[]}>("desktop_voice_languages").then((result) => {
        setHostedLanguages(result.items);
        const preferred = state.system_language.split(/[_-]/)[0]?.toLocaleLowerCase() || "en";
        const selected = result.items.some((item) => item.code === preferred) ? preferred : "en";
        setHostedLanguage(selected);
        void loadHostedVoices(hostedQueryRef.current, selected);
      }).catch((cause) => setAccountError(cause instanceof Error ? cause.message : String(cause)));
    }
    let current = true;
    const shouldLoadClones = account && tab === "voice";
    if (shouldLoadClones) void invoke<{items: HostedClone[]}>("desktop_clones").then((v) => {
      if (!current) return;
      setHostedClones(v.items);
      const settings = latestState.current?.settings;
      const selectedClone = v.items.find((clone) => clone.id === settings?.fish.voice_id);
      if (settings && selectedClone && (selectedClone.access === "disabled" || (account.cloneEntitlement?.limit ?? 0) <= 0)) {
        void save({ fish: { ...settings.fish, voice_id: "" } });
      }
    }).catch((cause) => { if (current) setAccountError(cause instanceof Error ? cause.message : String(cause)); });
    return () => { current = false; };
  }, [tab, state?.settings.engine, state?.system_language, account, loadHostedVoices, save]);

  useEffect(() => {
    if (!pendingCloneKey) return;
    const cloneIds = pendingCloneKey.split("\u0000");
    let active = true;
    let refreshing = false;
    const refreshStatuses = async () => {
      if (refreshing) return;
      refreshing = true;
      try {
        const updated = await Promise.all(
          cloneIds.map((voiceId) => invoke<HostedClone>("desktop_clone_status", { voiceId })),
        );
        if (active) {
          setHostedClones((current) => current.map((clone) => updated.find((item) => item.id === clone.id) ?? clone));
          const currentState = latestState.current;
          const selectedClone = updated.find((clone) => clone.id === currentState?.settings.fish.voice_id);
          if (currentState?.settings.engine === "fish" && selectedClone && selectedClone.status !== "trained") {
            void save({ fish: { ...currentState.settings.fish, voice_id: "" } });
          }
        }
      } catch {
        // Keep the last known status and retry on the next interval.
      } finally {
        refreshing = false;
      }
    };
    void refreshStatuses();
    const timer = window.setInterval(() => void refreshStatuses(), 10_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [pendingCloneKey, save]);

  /* Voice + language derivation. */
  const voices = state?.voices ?? [];
  const byLanguage = useMemo(() => {
    const map = new Map<string, Voice[]>();
    for (const voice of voices) {
      const list = map.get(voice.locale) ?? [];
      list.push(voice);
      map.set(voice.locale, list);
    }
    for (const list of map.values()) {
      list.sort(
        (a, b) => Number(a.novelty) - Number(b.novelty) || a.name.localeCompare(b.name),
      );
    }
    return map;
  }, [voices]);

  const systemLanguage = state?.system_language ?? "en_US";
  const family = systemLanguage.split("_")[0];

  /**
   * Open on the user's own language. There is frequently no voice for the exact locale
   * — this machine reports `en_CA` and macOS ships no Canadian English voice — so fall
   * back to the same language family, largest first, and say so in the UI.
   */
  const defaultLanguage = useMemo(() => {
    if (voices.length === 0) return null;
    if (byLanguage.has(systemLanguage)) return systemLanguage;
    const siblings = [...byLanguage.keys()].filter((locale) =>
      locale.startsWith(`${family}_`),
    );
    if (siblings.length === 0) return null;
    return siblings.sort(
      (a, b) => (byLanguage.get(b)?.length ?? 0) - (byLanguage.get(a)?.length ?? 0),
    )[0];
  }, [voices.length, byLanguage, systemLanguage, family]);

  useEffect(() => {
    if (language === null && defaultLanguage) setLanguage(defaultLanguage);
  }, [defaultLanguage, language]);

  const activeLanguage = language ?? defaultLanguage;
  const activeVoices = activeLanguage ? (byLanguage.get(activeLanguage) ?? []) : voices;
  const speechVoices = activeVoices.filter((voice) => !voice.novelty);
  const noveltyVoices = activeVoices.filter((voice) => voice.novelty);
  const siblingLocales = [...byLanguage.keys()]
    .filter((locale) => locale.startsWith(`${family}_`) && locale !== activeLanguage)
    .sort((a, b) => (byLanguage.get(b)?.length ?? 0) - (byLanguage.get(a)?.length ?? 0));
  const exactLocaleMissing = voices.length > 0 && !byLanguage.has(systemLanguage);

  /*
   * Engine derivation. The catalogue arrives from Rust, so the picker needs no knowledge
   * of which engines exist — and a voice's usability comes from the engine, never from a
   * guess made here.
   */
  const engines = state?.engines ?? [];
  const activeEngine = engines.find((entry) => entry.id === state?.settings.engine) ?? null;
  const advancedModelId = state?.settings.engine === "kokoro"
    ? state.settings.kokoro.quant : state?.settings.chatterbox.quant;
  const advancedNeedsDownload = Boolean(activeEngine?.needs_download
    || activeEngine?.models?.find(model => model.id === advancedModelId)?.installed === false);
  useEffect(() => {
    if (tab === "voice" && advancedNeedsDownload) setAdvancedVoiceOpen(true);
  }, [tab, advancedNeedsDownload, state?.settings.engine, advancedModelId]);

  const engineVoices = activeEngine?.voices ?? [];
  const engineVoicesByLanguage = useMemo(() => {
    const map = new Map<string, EngineVoice[]>();
    for (const voice of engineVoices) {
      const list = map.get(voice.language) ?? [];
      list.push(voice);
      map.set(voice.language, list);
    }
    for (const list of map.values()) {
      // Usable voices first: the unusable ones are shown for completeness, not as choices.
      list.sort(
        (a, b) =>
          Number(a.unavailable !== null) - Number(b.unavailable !== null) ||
          a.label.localeCompare(b.label),
      );
    }
    return map;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [engineVoices]);

  const engineVoiceCounts = {
    usable: engineVoices.filter((voice) => voice.unavailable === null).length,
    blocked: engineVoices.filter((voice) => voice.unavailable !== null).length,
  };

  const saveShortcut = useCallback(async (role: ShortcutRole, accelerator: string) => {
    const current = latestState.current;
    if (!current) return;
    const duplicate = accelerator && (Object.keys(current.settings.shortcuts) as ShortcutRole[]).find(other => other !== role && current.settings.shortcuts[other]?.toLowerCase() === accelerator.toLowerCase());
    if (duplicate) {
      setShortcutFeedback({ role, error: true, message: sourceMessage("Already used by {0}. Try another shortcut.", [shortcutLabels[duplicate]]) });
      return;
    }
    setRecording(null); setShortcutSaving(true);
    try {
      const saved = await save({ shortcuts: { ...current.settings.shortcuts, [role]: accelerator } });
      const refused = latestState.current?.refused_shortcuts ?? [];
      setShortcutFeedback({ role, error: !saved || refused.length > 0,
        message: !saved ? sourceMessage("Could not save this shortcut. Try another combination.") : refused.length > 0 ? sourceMessage("macOS could not register {0}. Choose another shortcut.", [refused.join(", ")]) : accelerator ? sourceMessage("Shortcut saved") : sourceMessage("Shortcut cleared") });
    } finally { setShortcutSaving(false); }
  }, [save]);

  useEffect(() => { setRecording(null); setShortcutFeedback(null); }, [tab]);

  useEffect(() => {
    if (!recording) return;
    // Keep an existing hold binding from starting dictation while choosing a key.
    void invoke("assistant_control", {command:"hold_suspend"}).catch(() => {});
    const blur = () => setRecording(null);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("blur", blur);
      void invoke("assistant_control", {command:"hold_resume"}).catch(() => {});
    };
  }, [recording]);

  /* The same recorder serves feature screens, setup, and Settings. */
  useEffect(() => {
    if (!recording || !state) return;
    const shortcuts = state.settings.shortcuts;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let heldCode = "";
    const clearHold = () => { clearTimeout(timer); timer = undefined; };
    const onUp = (event: KeyboardEvent) => {
      if (event.code !== heldCode) return;
      clearHold(); heldCode = "";
      if (event.key === "Escape") { setRecording(null); setShortcutFeedback(null); }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.repeat) return;
      clearHold();
      // Keep keyboard navigation and the Cancel button available while recording.
      if (!event.metaKey && !event.ctrlKey && !event.altKey &&
          (event.key === "Tab" || (event.target instanceof HTMLButtonElement && ["Enter", " "].includes(event.key)))) return;
      event.preventDefault();
      const hold = ({Fn:"Hold:Fn",AltLeft:"Hold:LeftOption",AltRight:"Hold:RightOption",Escape:"Hold:Escape"} as Record<string,string>)[event.code];
      if ((recording === "transcribe" || recording === "assistant") && hold && !event.metaKey && !event.ctrlKey && !event.shiftKey && (event.key === "Alt" || !event.altKey)) {
        heldCode = event.code;
        timer = setTimeout(() => { heldCode = ""; void saveShortcut(recording, hold); }, 350);
        return;
      }
      if (event.key === "Escape") {
        setRecording(null); setShortcutFeedback(null);
        return;
      }
      const accelerator = acceleratorFromEvent(event);
      if (!accelerator) return;
      const duplicate = (Object.keys(shortcuts) as ShortcutRole[]).find(role => role !== recording && shortcuts[role]?.toLowerCase() === accelerator.toLowerCase());
      if (duplicate) {
        setShortcutFeedback({ role: recording, error: true, message: sourceMessage("Already used by {0}. Try another combination.", [shortcutLabels[duplicate]]) });
        return;
      }
      void saveShortcut(recording, accelerator);
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onUp, true);
    window.addEventListener("mousedown", clearHold, true);
    return () => { clearHold(); window.removeEventListener("keydown", onKey, true); window.removeEventListener("keyup", onUp, true); window.removeEventListener("mousedown", clearHold, true); };
  }, [recording, saveShortcut, state]);

  const preview = useCallback(
    (voice: Voice | string | null) => {
      if (!state) return;
      void saveTail.current.then(() => invoke("preview_voice", {
        engine: state.settings.engine,
        voice: typeof voice === "string" ? voice : voice?.name ?? null,
        rate: state.settings.rate,
        text: null,
      })).catch((e) => setError(String(e)));
    },
    [state],
  );

  if (!state) {
    return (
      <div className="settings">
        <div className="sidebar" />
        <div className="vertical-rule" />
        <div className="pane">
          <div className="pane-inner">
            <span className="card-note">{t("Loading…")}</span>
          </div>
        </div>
      </div>
    );
  }

  const { settings } = state;
  const selectedVoice = voices.find((voice) => voice.name === settings.voice) ?? null;
  const selectedEngineVoice = engineVoices.find(
    (voice) => voice.id === activeEngine?.selected_voice,
  );

  /*
   * Download progress, when the Rust side is fetching weights for the engine on screen.
   * Keyed to the active engine so a background download for another one cannot post its
   * bar under the wrong row.
   */
  const installForEngine = install && install.engine === settings.engine
    && (!install.quant || install.quant === (settings.engine === "kokoro" ? settings.kokoro.quant : settings.chatterbox.quant)) ? install : null;
  const modelDownloadBusy = modelDeleting || installStarting || languageInstalling || install?.phase === "downloading";
  const installing = installForEngine?.phase === "downloading";
  const installPercent =
    installForEngine && installForEngine.total > 0
      ? Math.min(100, Math.round((installForEngine.done / installForEngine.total) * 100))
      : 0;


  const downloadSelectedModel = async () => {
    setInstallStarting(true);
    try {
      await saveTail.current;
      await invoke("install_engine", { engine: settings.engine,
        quant: settings.engine === "kokoro" ? settings.kokoro.quant : settings.chatterbox.quant });
    } finally { setInstallStarting(false); }
  };

  const shortcutControl = (role: ShortcutRole) => <div className="shortcut-control">
    <div className="task-shortcut">
      <kbd aria-live="polite">{recording === role ? t("Press a key combination…") : shortcutLabel(settings.shortcuts[role]) || t("No shortcut yet")}</kbd>
      <button className={recording === role ? "plain" : "plain accent-action"} disabled={shortcutSaving} onClick={() => { setShortcutFeedback(null); setRecording(recording === role ? null : role); }}>{recording === role ? t("Cancel") : settings.shortcuts[role] ? t("Change shortcut") : t("Create shortcut")}</button>
      {(role === "transcribe" || role === "assistant") && settings.shortcuts[role] && recording !== role ? <button className="text" disabled={shortcutSaving} onClick={() => void saveShortcut(role, "")}>{t("Clear shortcut")}</button> : null}
    </div>
    {recording === role ? <>
      {(role === "transcribe" || role === "assistant") && <div className="inline" style={{flexWrap:"wrap", marginTop:8}}>{Object.entries(holdKeys).map(([key,label]) => <button className="plain hold-key-button" key={key} disabled={shortcutSaving} onClick={() => void saveShortcut(role,key)}><span className="hold-key-symbol" aria-hidden="true">{holdKeySymbols[key]}</span>{t(label)}</button>)}</div>}
      <p className="card-note">{role === "transcribe" || role === "assistant" ? t("Choose a hold key, hold one for a moment, or press a key combination. Tap Esc to cancel.") : t("Press your new shortcut. Esc to cancel.")}</p>
    </> : null}
    {settings.shortcuts[role]?.startsWith("Hold:") && <p className="card-note">{t("Hold for 0.35 seconds to speak. Release to finish.")}{settings.shortcuts[role] === "Hold:Escape" ? t(" Quick Escape taps take effect on release.") : t(" Other keys or clicks cancel the hold.")}{settings.shortcuts[role] === "Hold:Fn" ? t(" If the Globe key also opens emoji or dictation, change its action in macOS Keyboard settings.") : ""}</p>}
    {settings.shortcuts[role]?.startsWith("Hold:") && serviceAvailability.availability.holdError && <p role="alert" className="shortcut-error">{serviceAvailability.availability.holdError}</p>}
    {shortcutFeedback?.role === role ? <p className={shortcutFeedback.error ? "shortcut-error" : "card-note"} role={shortcutFeedback.error ? "alert" : "status"}>{message(shortcutFeedback.message)}</p> : null}
  </div>;


  if (setupOpen || !settings.onboarding_completed) {
    return <SetupWizard
      key={setupRevision}
      settings={settings}
      voices={voices}
      trusted={state.trusted}
      engines={engines}
      recordingShortcut={recording}
      shortcutSaving={shortcutSaving}
      speaking={state.speaking}
      downloadBusy={modelDownloadBusy}
      install={installForEngine}
      onDownload={downloadSelectedModel}
      onStopPreview={() => void invoke("stop_speaking").catch(cause => setError(String(cause)))}
      onSave={async (patch) => { if (!await save(patch)) throw new Error(sourceMessage("Could not save your preferences. Please try again.")); }}
      externalError={error ? message(error) : null}
      onRequestAccessibility={() => { void requestAccessibility(); }}
      onOpenAccessibilitySettings={() => { void invoke("open_accessibility_settings").catch(cause => setError(String(cause))); }}
      onPreviewVoice={async () => {
        await saveTail.current;
        const current = latestState.current?.settings;
        if (!current) return;
        const voice = current.engine === "apple" ? current.voice : current.engine === "kokoro"
          ? current.kokoro.voice : current.engine === "chatterbox" ? current.chatterbox.voice : current.fish.voice_id;
        return invoke("preview_voice", { engine: current.engine, voice, rate: current.rate, text: null });
      }}
      renderShortcut={shortcutControl}
      onFinish={destination => { setSetupOpen(false); setTab(destination); }}
    />;
  }


  /**
   * Choosing a voice writes to whichever engine owns it. Apple's voices are top-level
   * because they predate the engines; the local models each own their own section.
   */
  const saveEngineVoice = (id: string) => {
    if (settings.engine === "kokoro") {
      void save({ kokoro: { ...settings.kokoro, voice: id } });
    } else if (settings.engine === "chatterbox") {
      // The id is a language code for Chatterbox, and the row renders that language's name
      // from the catalogue — so the only thing stored here is the code.
      void save({ chatterbox: { ...settings.chatterbox, voice: id } });
    } else {
      void save({ voice: id });
    }
  };

  /**
   * Add a reference voice: the window picks the file, Rust copies it into the app's own store,
   * and the catalogue comes back with the new row already in it. The dialog is the platform's
   * own, so a refused clip is reported as a sentence rather than a swallowed click.
   */
  const addVoice = async () => {
    setVoiceMessage(null);
    let path: string | null = null;
    try {
      const picked = await open({
        multiple: false,
        title: t("Add a reference voice"),
        filters: [{ name: t("WAV audio"), extensions: ["wav"] }],
      });
      path = typeof picked === "string" ? picked : null;
    } catch (e) {
      setVoiceMessage(String(e));
      return;
    }
    if (!path) return;
    try {
      await invoke<UiState>("add_chatterbox_voice", { path });
      await refresh();
      setVoiceMessage(sourceMessage("Voice added."));
    } catch (e) {
      setVoiceMessage(String(e));
      void refresh();
    }
  };

  const deleteVoice = async (file: string) => {
    setVoiceMessage(null);
    try {
      await invoke<UiState>("delete_chatterbox_voice", { file });
      await refresh();
      setVoiceMessage(sourceMessage("Voice deleted."));
    } catch (e) {
      setVoiceMessage(String(e));
      void refresh();
    }
  };

  const beginDesktopSignIn = async () => {
    setAccountBusy(true); setAccountError("");
    try {
      const url = await invoke<string>("begin_desktop_signin");
      await openUrl(url);
    } catch (cause) {
      void invoke("cancel_desktop_signin").catch(() => {});
      setAccountError(cause instanceof Error ? cause.message : String(cause));
      setAccountBusy(false);
    }
  };

  const cancelDesktopSignIn = async () => {
    try { await invoke("cancel_desktop_signin"); }
    finally {
      setAccountBusy(false);
      setAccountError("");
    }
  };

  const signOut = async () => {
    setAccountBusy(true); setAccountError("");
    try {
      await invoke("stop_speaking");
      await invoke("desktop_sign_out");
      setAccount(null); setAccountEmail(""); setHostedClones([]);
    } catch (cause) { setAccountError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setAccountBusy(false); }
  };

  const previewHostedVoice = async (voiceId: string, sampleId?: string) => {
    try {
      await invoke("desktop_voice_preview", { voiceId, sampleId });
    } catch (cause) { setAccountError(cause instanceof Error ? cause.message : String(cause)); }
  };

  const pickCloneAudio = async () => {
    const picked = await open({ multiple: false, title: t("Choose a voice recording"), filters: [{ name: t("Audio"), extensions: ["wav", "mp3", "m4a", "ogg", "flac"] }] });
    if (typeof picked === "string") setClonePath(picked);
  };

  const uploadClone = async () => {
    if (accountBusy || !clonePath || !cloneName.trim() || !cloneConsent) return;
    setAccountBusy(true); setAccountError("");
    setCloneUploadError(""); setCloneUploadSuccess("");
    try {
      const created = await invoke<HostedClone>("desktop_upload_clone", { path: clonePath, name: cloneName.trim(), consent: cloneConsent });
      setClonePath(""); setCloneName(""); setCloneConsent(false);
      setHostedClones((items) => [created, ...items.filter((item) => item.id !== created.id)]);
      setCloneUploadSuccess(sourceMessage("“{0}” was uploaded and saved. {1}", [created.name, created.status === "trained" ? sourceMessage("Ready to select in Voice → My hosted clones.") : created.status === "failed" ? sourceMessage("Training failed; see its status above.") : sourceMessage("Training is pending. It will be available in Voice → My hosted clones when ready.")]));
    } catch (cause) { setCloneUploadError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setAccountBusy(false); }
  };

  const deleteHostedClone = async (voiceId: string) => {
    const clone = hostedClones.find((item) => item.id === voiceId);
    if (!window.confirm(t("Delete “{0}” from your Fish Audio account?", [clone?.name ?? "this voice clone"]))) return;
    setAccountError("");
    try {
      await invoke("desktop_delete_clone", { voiceId });
      setHostedClones((items) => items.filter((item) => item.id !== voiceId));
      if (latestState.current?.settings.fish.voice_id === voiceId) void save({ fish: { ...latestState.current.settings.fish, voice_id: "" } });
    }
    catch (cause) { setAccountError(cause instanceof Error ? cause.message : String(cause)); }
  };

  const setHostedCloneActive = async (voiceId: string, active: boolean) => {
    setAccountError("");
    try {
      const updated = await invoke<HostedClone>("desktop_set_clone_active", { voiceId, active });
      setHostedClones((items) => items.map((item) => item.id === voiceId ? { ...item, access: updated.access } : item));
      if (!active && latestState.current?.settings.fish.voice_id === voiceId) void save({ fish: { ...latestState.current.settings.fish, voice_id: "" } });
    } catch (cause) { setAccountError(cause instanceof Error ? cause.message : String(cause)); }
  };

  const openCheckout = async (planId: string) => {
    setAccountBusy(true); setAccountError("");
    try { const url = await invoke<string>("desktop_checkout", { planId }); await openUrl(url); }
    catch (cause) { setAccountError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setAccountBusy(false); }
  };

  const buyTopup = async (packId: string) => {
    setAccountBusy(true); setAccountError("");
    try { const url = await invoke<string>("desktop_topup", { packId }); await openUrl(url); }
    catch (cause) { setAccountError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setAccountBusy(false); }
  };

  const statusLine =
    status.phase === "error"
      ? { kind: "error" as const, text: message(status.message ?? "failed") }
      : status.phase === "capturing"
        ? { kind: "info" as const, text: t("Reading selection…") }
        : status.phase === "preparing"
          ? { kind: "info" as const, text: t("Preparing speech…") }
          : status.phase === "speaking"
          ? {
              kind: "info" as const,
              text: status.chars ? t("Speaking {0} characters", [status.chars]) : t("Speaking…"),
            }
          : { kind: "secondary" as const, text: t("Ready") };

  const enableAccessibility = async () => {
    if (settings.accessibility_prompted) {
      await invoke("open_accessibility_settings");
    } else {
      const next = await invoke<UiState>("request_accessibility");
      latestState.current = next;
      setState(next);
    }
  };


  const settingsTabs: { id: Tab; title: string }[] = [
    { id: "general", title: t("General") },
    { id: "shortcuts", title: t("Shortcuts") },
    { id: "permissions", title: t("Permissions") },
    { id: "capture", title: t("Text capture") },
  ];
  const inSettings = settingsTabs.some((entry) => entry.id === tab);
  const tabs: { id: Tab; title: string; icon: React.ReactNode }[] = [
    { id: "home", title: t("Home"), icon: Icon.box() },
    { id: "voice", title: t("Voices"), icon: Icon.speaker() },
    { id: "transcription", title: t("Dictate"), icon: Icon.textCursor() },
    { id: "assistant", title: t("Assistant"), icon: Icon.sparkles() },
    { id: "history", title: t("History"), icon: Icon.history() },
  ];

  const readiness = serviceReadiness({
    availability: serviceAvailability.availability, error: serviceAvailability.error,
    trusted: state.trusted, focusedPasswordField: state.focused_password_field,
    shortcuts: settings.shortcuts, refused: state.refused_shortcuts,
    preferences: settings.assistant ?? defaultAssistantPreferences, engine: activeEngine,
  });

  const readinessIcon = (id: string, label: string) => {
    const reasons = readiness[id];
    const checking = reasons.length > 0 && reasons.every(reason => reason.startsWith(t("Checking ")));
    const statusLabel = checking ? t("Checking…") : reasons.length ? t("Not ready") : t("Ready");
    const show = (element: HTMLElement) => {
      const rect = element.getBoundingClientRect();
      setServiceTooltip({id, x: Math.max(12, Math.min(rect.left, window.innerWidth - 300)), y: Math.min(rect.bottom + 6, window.innerHeight - 240)});
    };
    return <button type="button" className={`service-status ${reasons.length ? "not-ready" : "ready"}`}
      aria-label={`${label}: ${statusLabel}`} title={reasons.join("\n") || t("Ready to use")}
      aria-description={reasons.join(" ") || t("Ready to use")}
      onMouseEnter={event => show(event.currentTarget)} onFocus={event => show(event.currentTarget)}
      onMouseLeave={() => setServiceTooltip(null)} onBlur={() => setServiceTooltip(null)}
      onKeyDown={event => { if (event.key === "Escape") setServiceTooltip(null); }}
      onClick={event => show(event.currentTarget)}>
      {reasons.length ? Icon.info() : Icon.checkCircle()}
      {reasons.length > 0 && <span>{statusLabel}</span>}
    </button>;
  };

  const paidAccountPlans = accountPlans?.filter((plan) => plan.id !== "free") ?? [];
  const activeAccountSubscription = subscriptionIsActive(account?.subscription ?? null) ? account?.subscription ?? null : null;
  const currentAccountPlanIndex = paidAccountPlans.findIndex((plan) => plan.id === activeAccountSubscription?.planId);
  const currentAccountPlan = currentAccountPlanIndex >= 0 ? paidAccountPlans[currentAccountPlanIndex] : null;
  const accountUpgradePlan = currentAccountPlanIndex >= 0
    ? paidAccountPlans[currentAccountPlanIndex + 1] ?? null
    : paidAccountPlans[paidAccountPlans.length - 1] ?? null;

  const planOptionsCard = (
    <Card title={account ? t("Plan") : t("Plans")} icon={Icon.gauge()}>
      {accountPlansLoading ? <span className="setup-plan-loading">{t("Loading plan options…")}</span> : null}
      {accountPlansError ? <><div className="card-note">{t("Plan details couldn’t be loaded.")}</div><div className="inline"><button className="plain accent-action" onClick={() => void loadAccountPlans()}>{t("Try again")}</button><button className="plain accent-action" onClick={() => void openUrl(`${websiteUrl}/pricing/`)}>{t("Open pricing page ↗")}</button></div></> : null}
      {accountPlans && !activeAccountSubscription ? <>
        {account?.subscription ? <Note kind="secondary" icon={Icon.info()}>{t("Your")} {currentAccountPlan?.name ?? account.subscription.planId}  {t("subscription is")} {account.subscription.status.replace(/_/g, " ")}{t(". Open billing to resolve it; hosted clone access is paused.")}</Note> : null}
        <p className="account-plan-intro">{t("Choose a plan. Local voices are free; hosted speech is optional.")}</p>
        <div className="account-plan-grid" aria-label={t("TextHalo plans")}>
          {accountPlans.map((plan) => <article className={`setup-plan-card${plan.id === "creator" ? " featured" : ""}`} key={plan.id}>
            <strong>{message(plan.name)}</strong><span className="setup-plan-price">{formatPlanPrice(plan.price)}</span><span>{message(plan.description)}</span>
            {plan.features.filter((feature) => feature !== "Everything in Free").slice(0, 3).map((feature) => <small key={feature}>{message(feature)}</small>)}
            {plan.id !== "free" ? <button className="plain accent-action account-plan-action" disabled={accountBusy || Boolean(account && !account.billingEnabled)} onClick={() => account ? void openCheckout(plan.id) : void beginDesktopSignIn()}>{account ? t("Choose {0}", [plan.name]) : t("Sign in for {0}", [plan.name])}</button> : null}
          </article>)}
        </div>
        <div className="inline"><button className="plain accent-action" onClick={() => void openUrl(`${websiteUrl}/pricing/`)}>{t("Compare plans ↗")}</button></div>
      </> : null}
      {accountPlans && activeAccountSubscription ? <>
        <div className="account-current-plan">
          <div className="account-current-plan-copy"><span className="credit-eyebrow">{t("CURRENT PLAN")}</span><strong>{currentAccountPlan?.name ?? activeAccountSubscription.planId}</strong><span>{activeAccountSubscription.status}{activeAccountSubscription.cancelAtPeriodEnd ? t(" · cancels at period end") : ""}{activeAccountSubscription.currentPeriodEnd ? t(" · renews {0}", [new Date(activeAccountSubscription.currentPeriodEnd).toLocaleDateString(currentLanguage())]) : ""}</span></div>
          {paidAccountPlans.length > 0 && currentAccountPlanIndex === paidAccountPlans.length - 1 ? <span className="account-plan-note">{t("You’re on the highest plan")}</span> : null}
        </div>
        {accountUpgradePlan ? <article className="setup-plan-card featured account-upgrade-card">
          <div><span className="credit-eyebrow">{t("AVAILABLE UPGRADE")}</span><strong>{accountUpgradePlan.name}</strong></div>
          <span className="setup-plan-price">{formatPlanPrice(accountUpgradePlan.price)}</span><span>{message(accountUpgradePlan.description)}</span>
          {accountUpgradePlan.features.filter((feature) => feature !== "Everything in Free").slice(0, 3).map((feature) => <small key={feature}>{message(feature)}</small>)}
          <button className="plain accent-action account-plan-action" disabled={accountBusy || !account?.billingEnabled} onClick={() => void openCheckout(accountUpgradePlan.id)}>{t("Upgrade to")} {accountUpgradePlan.name}</button>
        </article> : null}
        <div className="inline"><button className="plain accent-action" onClick={() => void openUrl(`${websiteUrl}/account/billing/`)}>{t("Manage billing")}</button></div>
      </> : null}
    </Card>
  );

  const voiceRow = (voice: Voice) => (
    <Row
      key={voice.name}
      selected={settings.voice === voice.name}
      glyph={settings.voice === voice.name ? Icon.checkCircle() : Icon.circle()}
      title={voiceLabel(voice.name)}
      subtitle={voice.novelty ? t("Not speech — a sound effect") : undefined}
      badge={settings.voice === voice.name ? t("Default") : undefined}
      onSelect={() => void save({ voice: voice.name })}
      trailing={
        <button
          className="icon"
          title={t("Preview {0}", [voiceLabel(voice.name)])}
          onClick={(event) => {
            event.stopPropagation();
            preview(voice);
          }}
        >
          {Icon.play()}
        </button>
      }
    />
  );

  return (
    <div className="settings">
      {serviceTooltip && createPortal(<div className="service-tooltip" role="tooltip" style={{left: serviceTooltip.x, top: serviceTooltip.y}}>
        {message(readiness[serviceTooltip.id]?.join("\n") || t("Ready to use"))}
      </div>, document.body)}
      <nav className="sidebar" aria-label={t("Main")}>
        <div className="sidebar-brand"><img src={appLogo} alt="" /><strong>{t("TextHalo")}</strong></div>
        {tabs.map((entry) => (
          <button
            key={entry.id}
            className={tab === entry.id ? "sidebar-row active" : "sidebar-row"}
            aria-current={tab === entry.id ? "page" : undefined}
            onClick={() => { setServiceTooltip(null); setTab(entry.id); }}
          >
            {entry.icon}
            <span>{entry.title}</span>
          </button>
        ))}
        <div className="sidebar-spacer" />
        <button className={`sidebar-row${tab === "account" ? " active" : ""}`} aria-current={tab === "account" ? "page" : undefined} onClick={() => setTab("account")}>{Icon.person()}  {t("Account")}</button>
        <button className={`sidebar-row${inSettings ? " active" : ""}`} aria-current={inSettings ? "page" : undefined} onClick={() => setTab("general")}>{Icon.gear()}  {t("Settings")}</button>
        {error ? (
          <Note kind="error" icon={Icon.xCircle()}>
            <span className="truncate" title={message(error)}>
              {message(error)}
            </span>
          </Note>
        ) : null}
        <Note kind={statusLine.kind} icon={Icon.info()}>
          <span className="truncate">{statusLine.text}</span>
        </Note>
      </nav>
      <div className="vertical-rule" />

      <main className="pane">
        {inSettings ? <nav className="section-nav" aria-label={t("Settings")}>{settingsTabs.map((entry) => <button key={entry.id} className={tab === entry.id ? "active" : ""} aria-current={tab === entry.id ? "page" : undefined} onClick={() => setTab(entry.id)}>{entry.title}</button>)}</nav> : null}
        {tab === "home" ? <div className="pane-inner home-pane">
          <header><h1 className="pane-title">{t("Home")}</h1><p className="home-intro">{t("Listen. Dictate. Get a little help.")}</p></header>
          <section className="listen-hero" aria-labelledby="listen-title">
            <div className="service-heading"><div className="home-eyebrow">{t("LISTEN")}</div>{readinessIcon("voice", t("Voices"))}</div>
            <h2 id="listen-title">{t("Turn text into speech.")}</h2>
            <p>{t("Select text in any app, then use your shortcut.")}</p>
            <div className="home-voice"><span>{message(activeEngine?.label ?? "Apple voices")}</span><span>{settings.engine === "fish" ? t("Hosted") : t("On this Mac")}</span></div>
            {!state.trusted ? <div className="home-notice"><span>{t("Allow Accessibility to read selected text.")}</span><button className="plain accent-action" onClick={() => void enableAccessibility().catch(cause => setError(String(cause)))}>{settings.accessibility_prompted ? t("Open Accessibility Settings") : t("Enable access")}</button></div>
              : activeEngine && !activeEngine.can_speak ? <div className="home-notice"><span>{message(activeEngine.status)}</span><button className="plain accent-action" onClick={() => setTab("voice")}>{t("Set up voice")}</button></div>
              : !settings.shortcuts.speak || state.refused_shortcuts.includes(settings.shortcuts.speak) ? <div className="home-notice"><span>{t("Choose an available listening shortcut.")}</span></div>
              : <p className="ready-label">{state.focused_password_field ? t("Leave the password field to start listening.") : t("Ready when you are")}</p>}
            <div className="home-voice-actions"><button className="primary-button" onClick={() => setTab("voice")}>{t("Choose a voice")}</button><button className="plain accent-action" disabled={!activeEngine?.can_speak || state.speaking} onClick={() => preview(settings.engine === "apple" ? settings.voice : activeEngine?.selected_voice ?? null)}>{t("Preview voice")}</button>{state.speaking ? <button className="plain accent-action" onClick={() => void invoke("stop_speaking").catch(cause => setError(String(cause)))}>{t("Stop")}</button> : null}</div>
            <div className="home-shortcut-section">{shortcutControl("speak")}</div>
          </section>
          <div className="home-tasks">
            <section className="task-card"><div className="service-heading"><h2>{t("Dictate")}</h2>{readinessIcon("transcription", t("Dictate"))}</div><p>{t("Speak and put your words into any text field.")}</p><div className="home-task-footer"><kbd>{shortcutLabel(settings.shortcuts.transcribe) || t("Set up a shortcut")}</kbd><button className="plain accent-action" onClick={() => setTab("transcription")}>{t("Set up dictation")}</button></div></section>
            <section className="task-card"><div className="service-heading"><h2>{t("Assistant")}</h2>{readinessIcon("assistant", t("Assistant"))}</div><p>{t("Ask to write, edit, or read something aloud.")}</p><div className="home-task-footer"><kbd>{shortcutLabel(settings.shortcuts.assistant) || t("Set up a shortcut")}</kbd><button className="plain accent-action" onClick={() => setTab("assistant")}>{t("Set up Assistant")}</button></div></section>
          </div>
          <button className="text home-history" onClick={() => setTab("history")}>{t("Open your history")}</button>
        </div> : null}

        {tab === "general" ? (
          <div className="pane-inner">
            <h1 className="pane-title">{t("General")}</h1>
            <Card title={t("Interface language")} icon={Icon.gear()}>
              <label className="field">
                <span className="field-label">{t("Language")}</span>
                <select aria-label={t("Interface language")} value={settings.ui_language ?? "system"}
                  onChange={event => void save({ ui_language: event.target.value as import("./localization").LanguagePreference })}>
                  <option value="system">{t("Follow Mac language")}</option>
                  {interfaceLanguages.map(language => <option key={language} value={language}>{languageNames[language]}</option>)}
                </select>
              </label>
              <p className="card-note">{t("Changes the interface language. Voice and dictation languages are set separately.")}</p>
            </Card>

            <Card title={t("Startup")} icon={Icon.gear()}>
              <label className="startup-option">
                <span className="startup-option-copy">
                  <strong>{t("Open TextHalo when you log in")}</strong>
                  <span>{t("It starts quietly in the menu bar, ready when you need it.")}</span>
                </span>
                <input
                  type="checkbox"
                  checked={settings.launch_at_login}
                  onChange={(event) => void save({ launch_at_login: event.target.checked })}
                />
              </label>
            </Card>

            <Card title={t("Optional features")} icon={Icon.gear()}>
              <p className="card-note">{t("Set up Dictation or Assistant when you want to use them.")}</p>
              <div className="inline">
                <button className="plain accent-action" onClick={() => setTab("transcription")}>{t("Set up Dictation")}</button>
                <button className="plain accent-action" onClick={() => setTab("assistant")}>{t("Set up Assistant")}</button>
              </div>
            </Card>

            <Card title={t("About")} icon={Icon.box()}>
              <div className="card-note">{t("Version {0}", [appVersion ?? "…"])}</div>
              <div className="inline">
                <button className="plain accent-action" onClick={() => void checkForUpdates()} disabled={checkingUpdate || installingUpdate}>
                  {checkingUpdate ? t("Checking…") : t("Check for Updates")}
                </button>
                {updateAvailable ? (
                  <button className="plain accent-action" onClick={() => void installAvailableUpdate()} disabled={installingUpdate}>
                    {installingUpdate ? t("Installing…") : t("Install {0}", [updateAvailable.version])}
                  </button>
                ) : null}
              </div>
              {updateMessage ? <div className="card-note" role="status">{message(updateMessage)}</div> : null}
              <details className="disclosure"><summary>{t("Advanced")}</summary>
              <div className="field">
                <span className="field-label">{t("Settings file")}</span>
                <div className="mono-block">{state.config_path}</div>
              </div></details>
            </Card>
          </div>
        ) : null}

        {tab === "voice" ? (
          <div className="pane-inner">
            <div className="history-heading">
              <div>
                <div className="service-heading"><h1 className="pane-title">{t("Voices")}</h1>{readinessIcon("voice", t("Voices"))}</div>
                <p className="pane-subtitle">
                  {t("Find a voice you enjoy listening to.")}</p>
              </div>
            </div>
            <Card title={t("Voice source")} icon={Icon.speaker()}>
              <div className="row-stack">
                {engines.map((engine) => (
                  <Row
                    key={engine.id}
                    selected={settings.engine === engine.id}
                    glyph={settings.engine === engine.id ? Icon.checkCircle() : Icon.circle()}
                    title={message(engine.label)}
                    subtitle={engine.id === "fish" ? t("Hosted · uses credits") : engine.id === "apple" ? t("On this Mac · no download") : t("On this Mac · free")}
                    badge={
                      engine.can_speak
                        ? t("Ready")
                        : engine.needs_download
                          ? t("Needs {0}", [formatBytes(engine.download_bytes)])
                          : t("Not ready")
                    }
                    disabled={modelDownloadBusy}
                    onSelect={() => void save({ engine: engine.id })}
                  />
                ))}
              </div>

              {activeEngine && !activeEngine.can_speak ? (
                <Note kind="warning" icon={Icon.warn()}>
                  {message(activeEngine.status)}
                </Note>
              ) : null}

              {settings.engine === "fish" && !account ? <>
                <div className="account-brand">
                  <img className="account-brand-mark" src={appLogo} alt="" />
                  <div><strong>{t("TextHalo")}</strong><div className="card-note">{t("Your account for hosted speech")}</div></div>
                  <div className="account-brand-actions">
                    <button className="plain accent-action" disabled={accountBusy} onClick={() => void beginDesktopSignIn()}>{accountBusy ? t("Waiting for browser sign-in…") : t("Sign in")}</button>
                    {accountBusy ? <button className="text" onClick={() => void cancelDesktopSignIn()}>{t("Cancel")}</button> : null}
                  </div>
                </div>
                {accountError ? <Note kind="error" icon={Icon.xCircle()}>{message(accountError)}</Note> : null}
              </> : null}


            </Card>

            {settings.engine === "kokoro" || settings.engine === "chatterbox" ? (
              <details className="disclosure" open={advancedVoiceOpen}
                onToggle={event => setAdvancedVoiceOpen(event.currentTarget.open)}>
                <summary>{t("Advanced voice settings")}</summary>
                <KokoroModelPicker engine={settings.engine === "kokoro" ? "Kokoro" : "Chatterbox"}
                  models={activeEngine?.models ?? []}
                  selected={settings.engine === "kokoro" ? settings.kokoro.quant : settings.chatterbox.quant}
                  disabled={modelDownloadBusy}
                  needsDownload={activeEngine?.needs_download}
                  downloadLabel={installStarting ? t("Starting…") : installing ? t("Downloading…") : activeEngine?.models.find(model => model.id === (settings.engine === "kokoro" ? settings.kokoro.quant : settings.chatterbox.quant))?.installed && activeEngine.needs_download ? t("Download missing files") : t("Download selected model")}
                  onDownload={() => void downloadSelectedModel().catch(cause => setError(String(cause)))}
                  deleting={modelDeleting}
                  deleteDisabled={state.speaking}
                  onDelete={async () => {
                    setModelDeleting(true);
                    try {
                      await saveTail.current;
                      await invoke<UiState>("delete_local_model", { engine: settings.engine,
                        quant: settings.engine === "kokoro" ? settings.kokoro.quant : settings.chatterbox.quant });
                      setInstall(null);
                      setError(null);
                      await refresh();
                    } catch (cause) { setError(String(cause)); }
                    finally { setModelDeleting(false); }
                  }}
                  onSelect={quant => {
                    setInstall(null);
                    void save(settings.engine === "kokoro"
                      ? { kokoro: { ...settings.kokoro, quant } }
                      : { chatterbox: { ...settings.chatterbox, quant } });
                  }} />
                {installing || installForEngine?.phase === "error" ? (
                  <div className="field">
                    <span className="field-label">{t("Model download")}</span>
                    {installing && installForEngine ? (
                      <>
                        <div className="progress">
                          <div
                            className="progress-bar"
                            style={{ width: `${installPercent}%` }}
                            role="progressbar"
                            aria-valuenow={installPercent}
                            aria-valuemin={0}
                            aria-valuemax={100}
                          />
                        </div>
                        <span className="field-hint">
                          {installPercent}% · {formatBytes(installForEngine.done)}  {t("of")}{" "}
                          {formatBytes(installForEngine.total)}
                        </span>
                      </>
                    ) : null}
                    {installForEngine?.phase === "error" ? (
                      <Note kind="error" icon={Icon.xCircle()}>
                        <span className="truncate" title={installForEngine.message ?? ""}>
                          {message(installForEngine.message ?? "")}
                        </span>
                      </Note>
                    ) : null}
                  </div>
                ) : null}
                <p className="card-note">{t("Keeping a voice in memory uses more memory but makes playback start faster.")}</p>
                {settings.engine === "kokoro" ? (
                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={settings.kokoro.keep_warm}
                      onChange={(event) =>
                        void save({
                          kokoro: { ...settings.kokoro, keep_warm: event.target.checked },
                        })
                      }
                    />
                    <span>{t("Keep Kokoro ready in memory")}</span>
                  </label>
                ) : null}
                {settings.engine === "chatterbox" ? (
                  <label className="toggle-row">
                    <input
                      type="checkbox"
                      checked={settings.chatterbox.keep_warm}
                      onChange={(event) =>
                        void save({
                          chatterbox: { ...settings.chatterbox, keep_warm: event.target.checked },
                        })
                      }
                    />
                    <span>{t("Keep Chatterbox ready in memory")}</span>
                  </label>
                ) : null}
              </details>
            ) : null}

            {settings.engine === "fish" ? (
              <>
                <Card title={t("Fish Audio hosted voices")} icon={Icon.speaker()}>
                  <div className="field">
                    <span className="field-label">{t("Language")}</span>
                    <select value={hostedLanguage} onChange={(event) => { const selected = event.target.value; setHostedLanguage(selected); void loadHostedVoices(hostedQuery, selected); }}>
                      <option value="all">{t("All languages")}</option>
                      {hostedLanguages.map((item) => <option key={item.code} value={item.code}>{hostedLanguageName(item.code)} — {item.voiceCount}</option>)}
                    </select>
                  </div>
                  <div className="field">
                    <span className="field-label">{t("Find a voice")}</span>
                    <div className="inline">
                      <input value={hostedQuery} placeholder={t("Search voices")} onChange={(event) => { setHostedQuery(event.target.value); hostedQueryRef.current = event.target.value; }} onKeyDown={(event) => { if (event.key === "Enter") void loadHostedVoices(hostedQuery, hostedLanguage); }} />
                      <button className="plain accent-action" disabled={hostedLoading} onClick={() => void loadHostedVoices(hostedQuery, hostedLanguage)}>{hostedLoading ? t("Loading…") : t("Search")}</button>
                    </div>
                    <span className="field-hint">{t("Choose a voice, then use your Speak shortcut anywhere.")}</span>
                  </div>
                  <div className="voice-list">
                    {hostedVoicesByLanguage.map(([code, voices]) => (
                      <div key={code}>
                        {hostedLanguage === "all" ? <div className="group-heading">{hostedLanguageName(code)}</div> : null}
                        <div className="row-stack" style={{ marginTop: hostedLanguage === "all" ? 6 : 0 }}>
                          {voices.map((voice) => {
                            const selected = settings.fish.voice_id === voice.id;
                            return <Row key={`${code}-${voice.id}`} selected={selected} glyph={selected ? Icon.checkCircle() : Icon.circle()} title={voice.name} subtitle={[voice.languageCodes?.join(", "), voice.description].filter(Boolean).join(" · ")} badge={selected ? t("Selected") : undefined} onSelect={() => void save({ fish: { ...settings.fish, voice_id: voice.id } })} trailing={voice.previewAvailable ? <button className="icon" title={t("Preview {0}", [voice.name])} onClick={(event) => { event.stopPropagation(); void previewHostedVoice(voice.id, voice.samples?.[0]?.id); }}>{Icon.play()}</button> : undefined} />;
                          })}
                        </div>
                      </div>
                    ))}
                  </div>
                  {hostedClones.length > 0 ? <div className="field"><span className="field-label">{t("My hosted clones")}</span><div className="row-stack">{hostedClones.map((clone) => { const inactive = (account?.cloneEntitlement?.limit ?? 0) <= 0 || clone.access === "disabled"; const ready = clone.status === "trained" && !inactive; const selected = settings.fish.voice_id === clone.id; const status = inactive ? t("Inactive slot") : clone.status === "trained" ? t("Ready") : clone.status === "failed" ? t("Training failed") : clone.status === "created" ? t("Queued") : t("Training"); return <Row key={clone.id} selected={selected} disabled={!ready} glyph={selected && ready ? Icon.checkCircle() : Icon.circle()} title={clone.name} subtitle={ready ? t("Clone · Ready to use") : inactive ? t("Clone · Stored but inactive for your current plan") : t("Clone · {0} · unavailable until training completes", [status])} badge={selected ? t("Selected") : status} onSelect={() => void save({ fish: { ...settings.fish, voice_id: clone.id } })} />; })}</div></div> : null}
                  {hostedVoices.length === 0 && !accountError && !hostedLoading ? <span className="card-note">{t("No voices loaded. Search or refresh to browse.")}</span> : null}
                  <label className="toggle-row"><input type="checkbox" checked={settings.fish.enhance_text} onChange={(event) => void save({ fish: { ...settings.fish, enhance_text: event.target.checked } })} /><span>{t("Enhance text with semantic delivery cues")}</span></label>
                </Card>
                <Card title={t("Custom voice cloning")} icon={Icon.speaker()}>
                  <p className="card-note">{t("Plus includes two saved clones, Creator includes up to five, and a first top-up starts a seven-day trial with two clones.")}</p>
                  {!account ? <Note kind="info" icon={Icon.info()}>{t("Sign in to see your clone access and create a custom voice.")}</Note> : (account.cloneEntitlement?.limit ?? 0) > 0 ? <Note kind="info" icon={Icon.info()}>{account.cloneEntitlement?.source === "topup_trial" && account.cloneEntitlement.expiresAt ? t("Your top-up trial includes {0} voice clones through {1}.", [account.cloneEntitlement.limit, new Date(account.cloneEntitlement.expiresAt).toLocaleDateString(currentLanguage())]) : t("Your current plan includes up to {0} voice clones.", [account.cloneEntitlement?.limit ?? 0])}</Note> : <Note kind="secondary" icon={Icon.info()}>{t("Voice cloning is available with Plus, Creator, or the seven-day trial after your first top-up. Visit Account to review plans.")}</Note>}
                  <button className="plain accent-action" onClick={() => { if (account && (account.cloneEntitlement?.limit ?? 0) > 0) { const panel = document.getElementById("your-voices") as HTMLDetailsElement | null; if (panel) { panel.open = true; panel.scrollIntoView({ behavior: "smooth", block: "start" }); } } else setTab("account"); }}>{!account ? t("Sign in to create a voice") : (account.cloneEntitlement?.limit ?? 0) > 0 ? t("Manage voice clones") : t("View plans")}</button>
                </Card>
                <Card title={t("Privacy for hosted speech")} icon={Icon.info()}>
                  <p className="card-note">{t("When you use hosted speech, selected text is sent to TextHalo’s service and the speech provider to generate audio. Optional text enhancement processes the text through an additional service. Local Apple, Kokoro, and Chatterbox engines process speech on your Mac.")}</p>
                  <button className="plain" onClick={() => void openUrl(`${websiteUrl}/privacy/`)}>{t("Read the privacy policy ↗")}</button>
                  <label className="toggle-row"><input type="checkbox" checked={settings.fish.privacy_accepted} onChange={(event) => void save({ fish: { ...settings.fish, privacy_accepted: event.target.checked } })} /><span>{t("I understand how hosted speech processes selected text.")}</span></label>
                </Card>
              </>
            ) : settings.engine === "apple" ? (
              <Card title={t("Spoken voice")} icon={Icon.speaker()}>
              <div className="field">
                <span className="field-label">{t("Language")}</span>
                <select
                  value={activeLanguage ?? "all"}
                  onChange={(event) =>
                    setLanguage(event.target.value === "all" ? null : event.target.value)
                  }
                >
                  <option value="all">{t("All languages ({0})", [voices.length])}</option>
                  {[...byLanguage.entries()]
                    .sort((a, b) => languageName(a[0]).localeCompare(languageName(b[0])))
                    .map(([locale, list]) => (
                      <option key={locale} value={locale}>
                        {languageName(locale)} — {list.length}
                        {locale === systemLanguage ? t(" · your language") : ""}
                      </option>
                    ))}
                </select>
                <span className="field-hint">
                  {t("This Mac is set to {0}.", [languageName(systemLanguage)])}
                </span>
              </div>

              {exactLocaleMissing && activeLanguage ? (
                <>
                  <Note kind="warning" icon={Icon.warn()}>
                    {t("No {0} voice is installed, so this shows {1} instead.", [languageName(systemLanguage), languageName(activeLanguage)])}</Note>
                  {siblingLocales.length > 0 ? (
                    <div className="field">
                      <span className="field-label">
                        {t("Other {0} regions", [familyName(family)])}</span>
                      <div className="chip-row">
                        {siblingLocales.map((locale) => (
                          <button
                            key={locale}
                            className="chip"
                            title={t("{0} voices", [byLanguage.get(locale)?.length ?? 0])}
                            onClick={() => setLanguage(locale)}
                          >
                            {languageName(locale).replace(/^[^(]*\(|\)$/g, "")}
                          </button>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </>
              ) : null}

              <div className="voice-list">
                {activeLanguage === null ? (
                  // "All languages" is the one view where a flat list is wrong.
                  [...byLanguage.entries()]
                    .sort((a, b) => languageName(a[0]).localeCompare(languageName(b[0])))
                    .map(([locale, list]) => {
                      const shown = list.filter((voice) => showNovelty || !voice.novelty);
                      if (shown.length === 0) return null;
                      return (
                        <div key={locale}>
                          <div className="group-heading">{languageName(locale)}</div>
                          <div className="row-stack" style={{ marginTop: 6 }}>
                            {shown.map(voiceRow)}
                          </div>
                        </div>
                      );
                    })
                ) : (
                  <>
                    <Row
                      selected={settings.voice === null}
                      glyph={settings.voice === null ? Icon.checkCircle() : Icon.circle()}
                      title={t("System default")}
                      subtitle={t("Whatever macOS is set to")}
                      badge={settings.voice === null ? t("Default") : undefined}
                      onSelect={() => void save({ voice: null })}
                    />
                    <div className="row-stack" style={{ marginTop: 6 }}>
                      {speechVoices.map(voiceRow)}
                      {showNovelty && noveltyVoices.length > 0 ? (
                        <>
                          <div className="group-heading">{t("Novelty / sound effects")}</div>
                          {noveltyVoices.map(voiceRow)}
                        </>
                      ) : null}
                      {speechVoices.length === 0 && !showNovelty ? (
                        <span className="card-note">
                          {t("No speaking voices in this language — only sound effects.")}</span>
                      ) : null}
                    </div>
                  </>
                )}
              </div>

              <div className="inline">
                <button className="plain accent-action" onClick={() => preview(selectedVoice)}>
                  {Icon.play()}  {t("Preview default")}</button>
                {noveltyVoices.length > 0 ? (
                  <button className="text" onClick={() => setShowNovelty(!showNovelty)}>
                    {showNovelty
                      ? t("Hide novelty voices")
                      : t("Show {0} novelty / sound-effect voices", [noveltyVoices.length])}
                  </button>
                ) : null}
              </div>
              <div className="card-note">
                {selectedVoice
                  ? t("Default: {0} ({1})", [voiceLabel(selectedVoice.name), selectedVoice.locale])
                  : t("Default: system voice")}
              </div>
              </Card>
            ) : (
              <Card title={t("Voices — {0}", [activeEngine?.label ?? ""])} icon={Icon.speaker()}>
                <div className="inline">
                  <button
                    className="plain accent-action"
                    disabled={!activeEngine?.can_speak || !selectedEngineVoice || selectedEngineVoice.unavailable !== null}
                    onClick={() => preview(selectedEngineVoice?.id ?? null)}
                  >
                    {Icon.play()}  {t("Preview")} {selectedEngineVoice?.label ?? t("selected voice")}
                  </button>
                  <button className="plain" onClick={() => void invoke("stop_speaking")}>
                    {t("Stop")}</button>
                </div>
                <div className="card-note">{t("Listen to a sample of the selected voice.")}</div>
                <div className="field">
                  <span className="field-label">{t("Voices")}</span>
                  <span className="field-hint">
                    {engineVoiceCounts.usable}  {t("usable")}{engineVoiceCounts.blocked > 0
                      ? t(" · {0} not selectable", [engineVoiceCounts.blocked])
                      : ""}
                  </span>
                </div>

                {settings.engine === "kokoro" ? <KokoroCjkSetup voices={engineVoices}
                  busy={installStarting || installing} disabled={modelDownloadBusy}
                  error={installForEngine?.phase === "error" ? installForEngine.message ?? null : null}
                  onDownload={() => { setInstall(null); void downloadSelectedModel().catch(cause => setError(String(cause))); }} /> : null}
                {settings.engine === "kokoro" ? <KokoroLanguageSetup voices={engineVoices}
                  event={languageInstall} busy={languageInstalling} disabled={modelDownloadBusy}
                  onDownload={() => {
                    setLanguageInstalling(true);
                    setLanguageInstall({ phase: "runtime", message: sourceMessage("Installing language support…") });
                    void saveTail.current.then(() => invoke("install_kokoro_languages")).catch(cause => {
                      setLanguageInstalling(false);
                      setLanguageInstall({ phase: "error", message: String(cause) });
                    });
                  }} /> : null}

                <div className="voice-list">
                  {[...engineVoicesByLanguage.entries()].map(([groupLanguage, list]) => (
                    <div key={groupLanguage}>
                      <div className="group-heading">{t(groupLanguage)}</div>
                      <div className="row-stack" style={{ marginTop: 6 }}>
                        {list.map((voice) => {
                          const chosen = activeEngine?.selected_voice === voice.id;
                          return (
                            <Row
                              key={voice.id}
                              selected={chosen}
                              glyph={chosen ? Icon.checkCircle() : Icon.circle()}
                              title={settings.engine === "chatterbox" ? t(voice.label) : voice.label}
                              subtitle={
                                voice.unavailable ? message(voice.unavailable) :
                                [voice.id, voice.note ? message(voice.note) : null].filter(Boolean).join(" · ")
                              }
                              mono={voice.unavailable === null}
                              disabled={voice.unavailable !== null}
                              badge={chosen ? t("Default") : undefined}
                              onSelect={() => saveEngineVoice(voice.id)}
                              trailing={
                                <button
                                  className="icon"
                                  title={t("Preview {0}", [voice.label])}
                                  aria-label={t("Preview {0}", [voice.label])}
                                  disabled={!activeEngine?.can_speak || voice.unavailable !== null}
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    preview(voice.id);
                                  }}
                                >
                                  {Icon.play()}
                                </button>
                              }
                            />
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>

                <div className="card-note">
                  {activeEngine?.selected_voice
                    ? t("Default: {0}", [activeEngine.selected_voice])
                    : t("No voice chosen yet.")}
                </div>
              </Card>
            )}

            {/*
              Reference voices. Chatterbox clones its speaker from a clip, so this is where the
              user's own voices live: the shipped one, plus whatever they added from disk. Only
              rendered for an engine that has any, which today means only Chatterbox.
            */}
            {(activeEngine?.ref_voices.length ?? 0) > 0 ? (
              <Card title={t("Reference voice")} icon={Icon.speaker()}>
                <div className="field">
                  <span className="field-label">{t("Voice to clone")}</span>
                  <span className="inline">
                    <button className="plain accent-action" onClick={() => void addVoice()}>
                      {Icon.download()}  {t("Add voice…")}</button>
                  </span>
                </div>

                {voiceMessage ? (
                  <Note kind="secondary" icon={Icon.info()}>
                    <span className="truncate" title={message(voiceMessage ?? "")}>
                      {message(voiceMessage ?? "")}
                    </span>
                  </Note>
                ) : null}

                <div className="row-stack">
                  {activeEngine?.ref_voices.map((voice) => {
                    const chosen =
                      (settings.chatterbox.ref_audio ?? "") ===
                      (voice.builtin ? "" : voice.id);
                    return (
                      <Row
                        key={voice.id}
                        selected={chosen}
                        glyph={chosen ? Icon.checkCircle() : Icon.circle()}
                        title={voice.builtin ? t(voice.label) : voice.label}
                        subtitle={message(voice.note)}
                        badge={chosen ? t("Default") : undefined}
                        onSelect={() =>
                          void save({
                            chatterbox: {
                              ...settings.chatterbox,
                              ref_audio: voice.builtin ? null : voice.id,
                            },
                          })
                        }
                        trailing={
                          voice.builtin ? undefined : (
                            <button
                              className="plain"
                              title={t("Delete {0}", [voice.label])}
                              onClick={(event) => {
                                event.stopPropagation();
                                void deleteVoice(voice.id);
                              }}
                            >
                              {t("Delete")}</button>
                          )
                        }
                      />
                    );
                  })}
                </div>

                <div className="card-note">{t("5–15 s of clean speech works best.")}</div>
              </Card>
            ) : null}

            {settings.engine === "apple" ? (
            <Card title={t("Speed")} icon={Icon.gauge()}>
              <div className="inline">
                <input
                  type="range"
                  min={80}
                  max={500}
                  step={5}
                  value={rateDraft ?? settings.rate}
                  onChange={(event) => {
                    const value = Number(event.target.value);
                    setRateDraft(value);
                    if (rateTimer.current) window.clearTimeout(rateTimer.current);
                    // Dragging a slider must not write the config eighty times.
                    rateTimer.current = window.setTimeout(() => {
                      void save({ rate: value });
                      setRateDraft(null);
                    }, 350);
                  }}
                />
                <span className="mono">{rateDraft ?? settings.rate}  {t("wpm")}</span>
              </div>
            </Card>
            ) : null}

            {settings.engine === "kokoro" ? (
              <Card title={t("Speed")} icon={Icon.gauge()}>


                <div className="field">
                  <span className="field-label">{t("Speed")}</span>
                  <div className="inline">
                    <input
                      type="range"
                      min={0.5}
                      max={2}
                      step={0.05}
                      value={settings.kokoro.speed}
                      onChange={(event) =>
                        void save({
                          kokoro: { ...settings.kokoro, speed: Number(event.target.value) },
                        })
                      }
                    />
                    <span className="mono">{settings.kokoro.speed.toFixed(2)}×</span>
                  </div>
                </div>


              </Card>
            ) : null}

            {settings.engine === "chatterbox" ? (
              <Card title={t("Language")} icon={Icon.gauge()}>
                <div className="field">
                  <span className="field-label">{t("Language")}</span>
                  <select
                    value={settings.chatterbox.voice}
                    onChange={(event) =>
                      void save({
                        chatterbox: { ...settings.chatterbox, voice: event.target.value },
                      })
                    }
                  >
                    {activeEngine?.voices.map((voice) => (
                      <option key={voice.id} value={voice.id}>
                        {settings.engine === "chatterbox" ? t(voice.label) : voice.label}
                      </option>
                    ))}
                  </select>
                </div>


              </Card>
            ) : null}
            {settings.engine === "fish" ? (
            <details className="disclosure" id="your-voices">
              <summary>{t("Your voices")}</summary>
              {accountError ? <Note kind="error" icon={Icon.xCircle()}>{message(accountError)}</Note> : null}
                {account && ((account.cloneEntitlement?.limit ?? 0) > 0 || hostedClones.length > 0) ? <section className="clone-panel">
                  <header className="clone-header">
                    <div className="clone-heading"><span className="clone-heading-icon">{Icon.speaker()}</span><div><h2>{t("Your voices")}</h2><p>{t("Create a personal voice from a recording you own or have permission to use.")}</p></div></div>
                    <span className="clone-count">{hostedClones.length}  {t("saved")}{(account.cloneEntitlement?.limit ?? 0) > 0 ? t(" · {0} of {1} active", [hostedClones.filter((clone) => clone.access !== "disabled" && clone.status !== "failed").length, account.cloneEntitlement?.limit ?? 0]) : t(" · access ended")}{account.cloneEntitlement?.source === "topup_trial" && account.cloneEntitlement.expiresAt ? t(" · trial ends {0}", [new Date(account.cloneEntitlement.expiresAt).toLocaleDateString(currentLanguage())]) : ""}</span>
                  </header>
                  {hostedClones.length ? <div className="clone-list" aria-label={t("Saved voices")}>{hostedClones.map((clone) => <div className="clone-item" key={clone.id}>
                    <span className="clone-avatar">{clone.name.trim().slice(0, 1).toUpperCase()}</span><div className="clone-item-copy"><strong>{clone.name}</strong><span className={`clone-status clone-status-${clone.status}`}>{clone.access === "disabled" ? t("Stored · inactive slot") : clone.status === "trained" ? t("Ready to use") : clone.status === "failed" ? t("Couldn’t train") : t("Training")}</span></div>
                    {clone.status !== "failed" && (account.cloneEntitlement?.limit ?? 0) > 0 ? <button className="clone-delete" onClick={() => void setHostedCloneActive(clone.id, clone.access === "disabled")}>{clone.access === "disabled" ? t("Activate") : t("Deactivate")}</button> : null}
                    <button className="clone-delete" aria-label={t("Delete {0}", [clone.name])} onClick={() => void deleteHostedClone(clone.id)}>{t("Delete")}</button>
                  </div>)}</div> : <div className="clone-empty"><span className="clone-empty-mark">{Icon.speaker()}</span><strong>{t("No hosted voices yet")}</strong><span>{t("Your new voice will appear here when it’s ready.")}</span></div>}
                  {(account.cloneEntitlement?.limit ?? 0) > 0 ? <div className="clone-create">
                    <div className="clone-create-title"><span className="clone-step">{t("NEW")}</span><div><h3>{t("Create a voice")}</h3><p>{t("Your recording stays on this device until you choose to upload.")}</p></div></div>
                    <div className="clone-fields">
                      <div className="field clone-name-field"><label className="field-label" htmlFor="hosted-clone-name">{t("Voice name")} <span>{t("Required")}</span></label><input id="hosted-clone-name" value={cloneName} placeholder={t("e.g. Warm narration")} maxLength={100} required aria-describedby="hosted-clone-name-hint" onChange={(event) => setCloneName(event.target.value)} /><span id="hosted-clone-name-hint" className="field-hint">{t("Up to 100 characters")}</span></div>
                      <div className="field"><span className="field-label">{t("Recording")} <span>{t("Required")}</span></span><button className={`clone-file-button${clonePath ? " has-file" : ""}`} disabled={accountBusy} onClick={() => void pickCloneAudio()}><span className="clone-file-icon">{Icon.speaker()}</span><span className="clone-file-copy"><strong>{clonePath ? clonePath.split(/[\\/]/).pop() : t("Choose an audio file")}</strong><span>{clonePath ? t("Selected locally · not uploaded") : t("WAV, MP3, M4A, OGG or FLAC · up to 25 MB")}</span></span><span className="clone-file-action">{clonePath ? t("Change") : t("Browse")}</span></button></div>
                    </div>
                    <label className="clone-consent"><input type="checkbox" checked={cloneConsent} onChange={(event) => setCloneConsent(event.target.checked)} /><span>{t("I own this voice or have permission to clone it. I understand the recording will be uploaded to Fish Audio and saved to my account.")}</span></label>
                    <div className="clone-submit-row"><div className="clone-feedback" role="status">{!cloneName.trim() ? t("Add a name to continue") : !clonePath ? t("Choose a recording to continue") : !cloneConsent ? t("Confirm permission to continue") : t("Everything’s ready to upload")}</div><button className="clone-submit" disabled={accountBusy || !cloneName.trim() || !clonePath || !cloneConsent} onClick={() => void uploadClone()}>{accountBusy ? t("Creating voice…") : t("Upload and create voice")}<span aria-hidden="true">→</span></button></div>
                    {cloneUploadError ? <div role="alert"><Note kind="error" icon={Icon.xCircle()}>{t("Clone was not created:")} {message(cloneUploadError)}</Note></div> : null}
                    {cloneUploadSuccess ? <div role="status"><Note kind="info" icon={Icon.checkCircle()}>{message(cloneUploadSuccess)}</Note></div> : null}
                    <div className="clone-privacy">{Icon.checkCircle()}<span>{t("Your recording is only sent when you select “Upload and create voice”.")}</span></div>
                  </div> : <div className="clone-create"><Note kind="secondary" icon={Icon.info()}>{t("Your clone access has ended. You can still delete saved voices here; choose Plus or Creator to create and use them again.")}</Note></div>}
                </section> : null}
              {!account || ((account.cloneEntitlement?.limit ?? 0) <= 0 && hostedClones.length === 0) ? <div className="empty-state">
                <p>{t("Create a voice from your own recording.")}</p>
                <button className="plain accent-action" onClick={() => setTab("account")}>{account ? t("View voice cloning plans") : t("Sign in to create a voice")}</button>
              </div> : null}
            </details>
            ) : null}
            <Card title={t("Listen shortcut")} icon={Icon.command()}>
              {shortcutControl("speak")}
            </Card>
          </div>
        ) : null}

        {tab === "account" ? (
          <div className="pane-inner">
            <h1 className="pane-title">{t("Account")}</h1>
            <p className="pane-subtitle">{t("Manage your account, credits, and plan.")}</p>
            {accountError ? <Note kind="error" icon={Icon.xCircle()}>{message(accountError)}</Note> : null}
            {account ? (
              <>
                <Card title={t("Signed in")} icon={Icon.checkCircle()}>
                  <div className="inline"><strong>{accountEmail || t("TextHalo account")}</strong><button className="plain" disabled={accountBusy} onClick={() => void refreshHostedAccount()}>{t("Refresh")}</button><button className="plain" disabled={accountBusy} onClick={() => void signOut()}>{t("Sign out")}</button></div>
                </Card>
                <Card title={t("Credits")} icon={Icon.gauge()}>
                  <div className="credit-balance">
                    <div><span className="credit-eyebrow">{t("AVAILABLE TO USE")}</span><strong>{account.availableCredits.toLocaleString(currentLanguage())}</strong><span className="credit-unit">{t("credits")}</span></div>
                  </div>
                  <div className="credit-breakdown">
                    <div className="credit-breakdown-heading"><strong>{t("Where your balance comes from")}</strong><span>{t("Remaining")}</span></div>
                    <div className="credit-source"><span><i className="credit-dot plan" />{t("Subscription credits")}</span><strong>{account.creditBreakdown.planCredits.toLocaleString(currentLanguage())}</strong></div>
                    <div className="credit-source"><span><i className="credit-dot topup" />{t("Purchased top-ups")}</span><strong>{account.creditBreakdown.topupCredits.toLocaleString(currentLanguage())}</strong></div>
                    {account.creditBreakdown.otherCredits ? <div className="credit-source"><span><i className="credit-dot other" />{t("Other credits")}</span><strong>{account.creditBreakdown.otherCredits.toLocaleString(currentLanguage())}</strong></div> : null}
                    {(account.creditBreakdown.debtCredits ?? 0) > 0 ? <div className="credit-source"><span><i className="credit-dot other" />{t("Refund balance due")}</span><strong>{account.creditBreakdown.debtCredits!.toLocaleString(currentLanguage())}</strong></div> : null}
                    <div className="credit-total"><span>{t("Total available")}</span><strong>{account.creditBreakdown.totalCredits.toLocaleString(currentLanguage())}</strong></div>
                  </div>
                  <p className="credit-explainer">{t("The source amounts above are included in your available balance. They are not extra credits on top of it.")}</p>
                </Card>
                {planOptionsCard}
                <Card title={t("One-time credit packs")} icon={Icon.gauge()}>
                  <div className="card-note">{t("Top-up credits are separate from monthly plan credits. Your first top-up includes seven days of voice cloning with two active slots; the trial waits while Plus or Creator already covers cloning.")}</div>
                  <div className="inline"><button className="plain accent-action" disabled={accountBusy} onClick={() => void buyTopup("topup-5")}>{t("$5 · 30,000 credits")}</button><button className="plain accent-action" disabled={accountBusy} onClick={() => void buyTopup("topup-10")}>{t("$10 · 60,000 credits")}</button><button className="plain accent-action" disabled={accountBusy} onClick={() => void buyTopup("topup-20")}>{t("$20 · 120,000 credits")}</button></div>
                </Card>

              </>
            ) : (
              <>
                <Card title={t("Sign in to TextHalo")} icon={Icon.gear()}>
                  <div className="account-brand">
                    <img className="account-brand-mark" src={appLogo} alt="" />
                    <div><strong>{t("TextHalo")}</strong><div className="card-note">{t("Your account for hosted speech")}</div></div>
                    <div className="account-brand-actions">
                      <button className="plain accent-action" disabled={accountBusy} onClick={() => void beginDesktopSignIn()}>{accountBusy ? t("Waiting for browser sign-in…") : t("Sign in")}</button>
                      {accountBusy ? <button className="text" onClick={() => void cancelDesktopSignIn()}>{t("Cancel")}</button> : null}
                    </div>
                  </div>
                  <div className="inline"><button className="text" onClick={() => void openUrl(`${websiteUrl}/sign-in/?mode=sign-up`)}>{t("Create an account")}</button><button className="text" onClick={() => void openUrl(`${websiteUrl}/sign-in/?mode=forgot-password`)}>{t("Forgot password?")}</button></div>
                </Card>
                {planOptionsCard}
              </>
            )}
          </div>
        ) : null}

        {tab === "permissions" ? <div className="pane-inner">
          <h1 className="pane-title">{t("Permissions")}</h1>
          <p className="pane-subtitle">{t("You control what TextHalo can access.")}</p>
            <Card title={t("Accessibility")} icon={Icon.textCursor()}>
              {state.trusted ? (
                <>
                  <Note kind="secondary" icon={Icon.checkCircle()}>
                    {t("Accessibility access granted. You can read and edit text in other apps.")}</Note>

                </>
              ) : (
                <>
                  <Note kind="warning" icon={Icon.warn()}>
                    {t("Allow TextHalo to read selected text and insert your words in other apps.")}</Note>
                  <div className="card-note">{t("Text is captured only when you use a shortcut. Clipboard preferences are in Text capture.")}</div>
                  <div className="inline">
                    {state.settings.accessibility_prompted ? (
                      <button
                        className="plain accent-action"
                        onClick={() => void invoke("open_accessibility_settings")}
                      >
                        {t("Open Accessibility Settings…")}</button>
                    ) : (
                      <button
                        className="plain accent-action"
                        onClick={() =>
                          void invoke<UiState>("request_accessibility")
                            .then((next) => {
                              latestState.current = next;
                              setState(next);
                            })
                            .catch((cause) => setError(String(cause)))
                        }
                      >
                        {t("Enable Accessibility")}</button>
                    )}
                  </div>
                  {state.settings.accessibility_prompted ? (
                    <div className="card-note">
                      {t("In System Settings, switch")} <strong>{t("TextHalo")}</strong>  {t("on under Privacy & Security → Accessibility. This panel notices by itself once you do.")}</div>
                  ) : (
                    <div className="card-note">
                      {t("macOS will show its standard permission prompt. Afterward, enable")}<strong>  {t("TextHalo")}</strong>  {t("under Privacy & Security → Accessibility.")}</div>
                  )}

                </>
              )}
              {state.focused_password_field ? (
                <Note kind="warning" icon={Icon.warn()}>
                  {t("Leave the password field to use TextHalo.")}</Note>
              ) : null}
            </Card>

          <Card title={t("Voice and screen access")} icon={Icon.gear()}><PermissionsSettings /></Card>
        </div> : null}
        {tab === "transcription" ? <div className="pane-inner">
          <div className="service-heading"><h1 className="pane-title">{t("Dictate")}</h1>{readinessIcon("transcription", t("Dictate"))}</div>
          <p className="pane-subtitle">{t("Hold your shortcut, speak, then release.")}</p>
          {shortcutControl("transcribe")}
          <Card title={t("Transcription source")} icon={Icon.textCursor()}><TranscriptionSource value={settings.assistant ?? defaultAssistantPreferences} onChange={assistant => void save({ assistant })} availability={serviceAvailability.availability} warningIcon={Icon.warn()} downloadIcon={Icon.download()} /></Card>
          <TaskReadiness mode="dictate" preferences={settings.assistant ?? defaultAssistantPreferences} needsAccessibility={(settings.assistant ?? defaultAssistantPreferences).dictation_output === "insert"} accessibilityPrompted={settings.accessibility_prompted} onAccessibility={enableAccessibility} />
          <Card title={t("Preferences")} icon={Icon.textCursor()}><TranscriptionSettings value={settings.assistant ?? defaultAssistantPreferences} onChange={assistant => void save({ assistant })} /></Card>
          <button className="text" onClick={() => { setHistoryView("transcripts"); setTab("history"); }}>{t("View transcripts")}</button>
        </div> : null}
        {tab === "assistant" ? <div className="pane-inner">
          <div className="service-heading"><h1 className="pane-title">{t("Assistant")}</h1>{readinessIcon("assistant", t("Assistant"))}</div>
          <p className="pane-subtitle">{t("Hold your shortcut and ask to write, edit, or read aloud.")}</p>
          {shortcutControl("assistant")}
          <TaskReadiness mode="assistant" preferences={settings.assistant ?? defaultAssistantPreferences} accessibilityPrompted={settings.accessibility_prompted} onAccessibility={enableAccessibility} />
          <Card title={t("Make it yours")} icon={Icon.textCursor()}><AssistantSettings value={settings.assistant ?? defaultAssistantPreferences} onChange={assistant => void save({ assistant })} onSignIn={() => void beginDesktopSignIn()} onCancelSignIn={() => void cancelDesktopSignIn()} signingIn={accountBusy} warningIcon={Icon.warn()} accountMark={appLogo} signInError={message(accountError)} /></Card>
          <button className="text" onClick={() => { setHistoryView("assistant"); setTab("history"); }}>{t("View Assistant history")}</button>
          {(settings.assistant?.provider ?? "local") !== "local" && <button className="text" onClick={() => setTab("account")}>{t("Account and credits")}</button>}
        </div> : null}

        {tab === "shortcuts" ? (
          <div className="pane-inner">
            <h1 className="pane-title">{t("Shortcuts")}</h1>
            <p className="pane-subtitle">
              {t("Hold Dictate or Assistant to speak. Press Listen to read selected text.")}</p>

            {([
              {title:t("Listen"),role:"speak"}, {title:t("Dictate"),role:"transcribe"},
              {title:t("Assistant"),role:"assistant"}, {title:t("Stop / cancel"),role:"stop"},
            ] as const).map(({title,role}) => <Card key={role} title={title} icon={Icon.command()}>
              {shortcutControl(role)}
            </Card>)}
            <Card title={t("Shortcut availability")} icon={Icon.keyboard()}>
              {state.refused_shortcuts.length > 0 ? (
                <Note kind="error" icon={Icon.xCircle()}>
                  {t("macOS refused")} {state.refused_shortcuts.join(", ")}  {t("— another app already owns it. Choose another shortcut.")}</Note>
              ) : null}
              <div className="card-note">
                {t("A combination needs at least one modifier (⌘ ⌃ ⌥ ⇧); function keys may be used on their own.")}</div>
            </Card>
          </div>
        ) : null}

        {tab === "capture" ? (
          <div className="pane-inner">
            <h1 className="pane-title">{t("Text capture")}</h1>
            <p className="pane-subtitle">{t("Choose how selected text is read.")}</p>

            <Card title={t("Capture method")} icon={Icon.textCursor()}>
              <div className="row-stack">
                {(
                  [
                    {
                      mode: "copy_only" as CaptureMode,
                      title: t("Copy selected text (recommended)"),
                      subtitle:
                        t("Uses the clipboard. Requires Accessibility access."),
                    },
                    {
                      mode: "ax_then_copy" as CaptureMode,
                      title: t("Read directly, then copy"),
                      subtitle:
                        t("Uses the clipboard only when direct reading is unavailable."),
                    },
                    {
                      mode: "ax_only" as CaptureMode,
                      title: t("Read directly"),
                      subtitle: t("Never touches the clipboard"),
                    },
                  ] as const
                ).map((option) => (
                  <Row
                    key={option.mode}
                    selected={settings.capture_mode === option.mode}
                    glyph={
                      settings.capture_mode === option.mode
                        ? Icon.checkCircle()
                        : Icon.circle()
                    }
                    title={option.title}
                    subtitle={option.subtitle}
                    onSelect={() => void save({ capture_mode: option.mode })}
                  />
                ))}
              </div>
            </Card>

            <Card title={t("Limits")} icon={Icon.warn()}>
              <label className="toggle-row">
                <input
                  type="checkbox"
                  checked={settings.restore_clipboard}
                  onChange={(event) => void save({ restore_clipboard: event.target.checked })}
                />
                <span>
                  <span>{t("Put the clipboard back after a copy-mode capture")}</span>
                  <span className="field-hint" style={{ display: "block" }}>
                    {t("Without this, capturing overwrites whatever you had copied.")}</span>
                </span>
              </label>
              <div className="field">
                <span className="field-label">{t("Speak at most")}</span>
                <div className="inline">
                  <input
                    type="number"
                    min={100}
                    max={100000}
                    step={100}
                    value={settings.max_chars}
                    onChange={(event) => {
                      const value = Number(event.target.value);
                      if (Number.isFinite(value) && value >= 100) {
                        void save({ max_chars: Math.min(value, 100000) });
                      }
                    }}
                  />
                  <span className="card-note">{t("characters per selection")}</span>
                </div>
                <span className="field-hint">
                  {t("Guards against capturing a whole document by accident.")}</span>
              </div>
            </Card>
          </div>
        ) : null}

        {tab === "history" ? (
          <div className="pane-inner">
            <div className="section-nav history-tabs" role="group" aria-label={t("History type")}>
              <button aria-pressed={historyView === "audio"} className={historyView === "audio" ? "active" : ""} onClick={() => setHistoryView("audio")}>{t("Audio")}</button>
              <button aria-pressed={historyView === "transcripts"} className={historyView === "transcripts" ? "active" : ""} onClick={() => setHistoryView("transcripts")}>{t("Transcripts")}</button>
              <button aria-pressed={historyView === "assistant"} className={historyView === "assistant" ? "active" : ""} onClick={() => setHistoryView("assistant")}>{t("Assistant")}</button>
            </div>
            {historyView === "assistant" ? <GenerationHistory /> : historyView === "transcripts" ? <><h1 className="pane-title">{t("History")}</h1><TranscriptionHistory /></> : <>
            <div className="history-heading">
              <div>
                <h1 className="pane-title">{t("History")}</h1>
                <p className="pane-subtitle">{t("Saved privately on this Mac · keeps the latest 50.")}</p>
              </div>
              <div className="inline">
                <button className="plain" onClick={() => void refreshHistory()} disabled={historyLoading}>
                  {t("Refresh")}</button>
                <button className="plain" onClick={() => void clearHistory()} disabled={!historyEntries.length}>
                  {t("Clear all")}</button>
              </div>
            </div>
            {historyError ? <Note kind="error" icon={Icon.xCircle()}>{message(historyError)}</Note> : null}
            {historyLoading && !historyEntries.length ? (
              <Card title={t("Loading audio history")} icon={Icon.history()}>
                <div className="card-note">{t("Reading saved recordings…")}</div>
              </Card>
            ) : historyEntries.length ? (
              <div className="history-list">
                {Array.from(historyGroups, ([appName, titles]) => (
                  <section className="history-group" key={appName}>
                  <h2>{appName}</h2>
                  {Array.from(titles, ([title, entries]) => (
                  <div className="history-subgroup" key={title}>
                  {title ? <h3>{title}</h3> : null}
                  {entries.map((entry) => (
                  <article className="history-item" key={entry.id}>
                    <div className="history-copy">
                      <div className="history-meta">
                        <strong>{entry.engine}</strong>
                        {entry.partial ? <span title={t("Only received audio was saved; the text shows the original request")}>{t("Partial")}</span> : null}
                        <span>{entry.voice}</span>
                        <time dateTime={new Date(entry.createdAt).toISOString()}>
                          {new Intl.DateTimeFormat(currentLanguage(), {
                            dateStyle: "medium",
                            timeStyle: "short",
                          }).format(entry.createdAt)}
                        </time>
                      </div>
                      <p className="history-text">{entry.text}</p>
                    </div>
                    <div className="history-actions">
                      <button
                        className="plain accent-action"
                        onClick={() =>
                          void invoke("play_audio_history", { id: entry.id }).catch((e) =>
                            setHistoryError(String(e)),
                          )
                        }
                      >
                        {Icon.play()}  {t("Play")}</button>
                      <button
                        className="plain"
                        onClick={() => void deleteHistoryEntry(entry.id)}
                        aria-label={t("Delete saved audio")}
                      >
                        {t("Delete")}</button>
                    </div>
                  </article>
                  ))}
                  </div>
                  ))}
                  </section>
                ))}
              </div>
            ) : (
              <Card title={t("No saved audio yet")} icon={Icon.history()}>
                <div className="card-note">
                  {t("TextHalo will save completed and partial hosted recordings here. Voice preview samples are not saved.")}</div>
              </Card>
            )}
            </> }
          </div>
        ) : null}
      </main>
    </div>
  );
}
