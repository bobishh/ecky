type SpeechWindow = Window &
  typeof globalThis & {
    speechSynthesis?: SpeechSynthesis;
    SpeechSynthesisUtterance?: typeof SpeechSynthesisUtterance;
  };

export interface EckySpeechOptions {
  muted?: boolean;
  rate?: number;
  pitch?: number;
  volume?: number;
}

let speechMuted = false;

function speechWindow(): SpeechWindow | null {
  if (typeof window === 'undefined') return null;
  return window as SpeechWindow;
}

function normalizeSpeechText(text: string): string {
  return text
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 600);
}

export function setSpeechMuted(muted: boolean) {
  speechMuted = muted;
  if (muted) stopEckySpeech();
}

export function stopEckySpeech() {
  const synth = speechWindow()?.speechSynthesis;
  try {
    synth?.cancel();
  } catch {
    // WebView speech synthesis can vanish while a window is closing.
  }
}

export function speakEckyText(text: string, options: EckySpeechOptions = {}): boolean {
  const cleanText = normalizeSpeechText(text);
  if (!cleanText || speechMuted || options.muted) return false;

  const win = speechWindow();
  if (!win?.speechSynthesis || !win.SpeechSynthesisUtterance) return false;

  const utterance = new win.SpeechSynthesisUtterance(cleanText);
  utterance.rate = options.rate ?? 0.94;
  utterance.pitch = options.pitch ?? 1.04;
  utterance.volume = options.volume ?? 1;

  try {
    win.speechSynthesis.cancel();
    win.speechSynthesis.speak(utterance);
    return true;
  } catch {
    return false;
  }
}
