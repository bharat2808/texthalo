import { t, message } from "./i18n";
import type { Availability, AssistantPreferences } from "./AssistantSettings";
import { localAssistantReason } from "./assistantAvailability";

type Input = {
  availability: Availability;
  error: string;
  trusted: boolean;
  focusedPasswordField: boolean;
  shortcuts: { speak: string; transcribe: string; assistant: string };
  refused: string[];
  preferences: AssistantPreferences;
  engine: { can_speak: boolean; status: string } | null;
};

export function serviceReadiness(input: Input): Record<string, string[]> {
  const { availability: a, preferences: p } = input;
  const shortcut = (key: keyof Input["shortcuts"], label: string) => {
    const value = input.shortcuts[key];
    if (value?.startsWith("Hold:") && (!input.trusted || a.holdError)) {
      return [a.holdError ? message(a.holdError) : t("Allow Accessibility to use hold keys.")];
    }
    return !value ? [t("Set {0} shortcut.", [label])] : input.refused.includes(value)
      ? [t("Choose another {0} shortcut; this one is unavailable.", [label])] : [];
  };
  const access = (name: string, status: number | undefined, denied: number) =>
    status === 3 ? [] : [status === undefined ? t("Checking {0} access…", [name])
      : status === 0 ? t("Allow {0} access.", [name])
      : status === denied ? t("{0} access is denied. Enable it in System Settings.", [name])
      : t("{0} access is restricted by macOS.", [name])];
  const voiceInput = input.error ? [t("Could not check readiness. Open the feature to retry.")] : [
    ...access(t("Microphone"), a.microphone, 2),
    ...(p.transcription_engine === "whisper" ? a.whisperReady ? [] : [a.whisperReady === undefined ? t("Checking the selected Whisper model…") : t("Download the selected Whisper model in Dictate settings.")] : access(t("Speech Recognition"), a.recognition, 1)),
  ];
  const accessibility = input.trusted ? [] : [t("Allow Accessibility access.")];
  const focusedPasswordField = input.focusedPasswordField ? [t("Leave the password field to use shortcuts.")] : [];
  const usesCloud = p.provider === "cloud" || (p.provider === "automatic" && a.model !== undefined && a.model !== "available");
  const cloud = [
    ...(p.cloud_consent_version === 1 ? [] : [t("Enable cloud processing in Assistant settings.")]),
    ...(a.cloudConfigured ? [] : [t("Cloud Assistant is not configured in this build.")]),
    ...(a.cloudSignedIn ? [] : [t("Sign in from Account to use Cloud Assistant.")]),
  ];
  const model = usesCloud ? cloud : a.model === "available" ? [] : [localAssistantReason(a.model)];
  const screenInput = usesCloud ? a.cloudScreenInput ?? a.imageInput : a.imageInput;
  const visualContext = screenInput === undefined ? [t("Checking visual context support…")]
    : screenInput && a.screen !== true
      ? [a.screen === undefined ? t("Checking Screen Recording access…") : t("Allow Screen Recording to use Assistant.")]
      : [];
  return {
    voice: [...shortcut("speak", t("Listen")), ...accessibility, ...focusedPasswordField,
      ...(input.engine?.can_speak ? [] : [message(input.engine?.status || t("Checking the selected voice…"))])],
    transcription: [...shortcut("transcribe", t("Dictate")), ...voiceInput,
      ...(p.dictation_output === "insert" ? accessibility : []), ...focusedPasswordField],
    assistant: [...shortcut("assistant", t("Assistant")), ...voiceInput, ...accessibility, ...focusedPasswordField, ...model, ...visualContext],
  };
}
