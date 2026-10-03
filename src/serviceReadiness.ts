import type { Availability, AssistantPreferences } from "./AssistantSettings";

type Input = {
  availability: Availability;
  error: string;
  trusted: boolean;
  secureInput: boolean;
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
      return [a.holdError || "Allow Accessibility to use hold keys."];
    }
    return !value ? [`Set ${label === "Assistant" ? "an" : "a"} ${label} shortcut.`] : input.refused.includes(value)
      ? [`Choose another ${label} shortcut; this one is unavailable.`] : [];
  };
  const access = (name: string, status: number | undefined, denied: number) =>
    status === 3 ? [] : [status === undefined ? `Checking ${name} access…`
      : status === 0 ? `Allow ${name} access.`
      : status === denied ? `${name} access is denied. Enable it in System Settings.`
      : `${name} access is restricted by macOS.`];
  const voiceInput = input.error ? ["Could not check readiness. Open the feature to retry."] : [
    ...access("Microphone", a.microphone, 2),
    ...access("Speech Recognition", a.recognition, 1),
  ];
  const accessibility = input.trusted ? [] : ["Allow Accessibility access."];
  const secureInput = input.secureInput ? ["Leave the password field to use shortcuts."] : [];
  const usesCloud = p.provider === "cloud" || (p.provider === "automatic" && a.model !== undefined && a.model !== "available");
  const cloud = [
    ...(p.cloud_consent_version === 1 ? [] : ["Enable cloud processing in Assistant settings."]),
    ...(a.cloudConfigured ? [] : ["Cloud Assistant is not configured in this build."]),
    ...(a.cloudSignedIn ? [] : ["Sign in from Account to use Cloud Assistant."]),
  ];
  const model = usesCloud ? cloud : a.model === "available" ? [] : [!a.model ? "Checking Apple Intelligence…"
    : a.model.includes("appleIntelligenceNotEnabled") ? "Turn on Apple Intelligence in System Settings."
    : a.model.includes("modelNotReady") ? "Apple Intelligence is still downloading or preparing."
    : "Apple Intelligence is unavailable on this Mac. Check its requirements in System Settings."];
  const screenInput = usesCloud ? a.cloudScreenInput ?? a.imageInput : a.imageInput;
  const visualContext = screenInput === undefined ? ["Checking visual context support…"]
    : screenInput && a.screen !== true
      ? [a.screen === undefined ? "Checking Screen Recording access…" : "Allow Screen Recording to use Assistant."]
      : [];
  return {
    voice: [...shortcut("speak", "Listen"), ...accessibility, ...secureInput,
      ...(input.engine?.can_speak ? [] : [input.engine?.status || "Checking the selected voice…"])],
    transcription: [...shortcut("transcribe", "Dictate"), ...voiceInput,
      ...(p.dictation_output === "insert" ? accessibility : []), ...secureInput],
    assistant: [...shortcut("assistant", "Assistant"), ...voiceInput, ...accessibility, ...secureInput, ...model, ...visualContext],
  };
}
