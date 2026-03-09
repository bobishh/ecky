import { convertFileSrc } from '@tauri-apps/api/core';
import type { AppConfig, AssetConfig } from '../types/domain';

// ---------------------------------------------------------------------------
// STL Cafeteria — N microwaves running concurrently
// Each microwave has its own hum layer. Dings fire independently.
// The ambient bed scales intensity with active microwave count.
// ---------------------------------------------------------------------------

let audioCtx: AudioContext | null = null;
let masterGain: GainNode | null = null;

type MicrowaveNode = AudioBufferSourceNode | OscillatorNode | HTMLMediaElement;
type AudioCapableWindow = Window & typeof globalThis & { webkitAudioContext?: typeof AudioContext };
type MicrowaveAppConfig = AppConfig | null | undefined;

// Per-microwave hum tracking: requestId → { nodes, gainNode, threadId }
interface MicrowaveEntry {
  nodes: MicrowaveNode[];
  gain: GainNode;
  threadId: string | null;
}
const microwaveHums: Map<string, MicrowaveEntry> = new Map();

// Shared ambient bed (always-on low drone when ≥1 microwave active)
let ambientNodes: (AudioBufferSourceNode | OscillatorNode)[] = [];
let ambientGain: GainNode | null = null;
let currentAudibleThreadId: string | null = null;

export function ensureContext(): AudioContext | null {
  if (audioCtx) {
    if (audioCtx.state === 'suspended') {
      audioCtx.resume();
    }
    return audioCtx;
  }
  try {
    const audioContextCtor =
      window.AudioContext || (window as AudioCapableWindow).webkitAudioContext;
    if (!audioContextCtor) {
      return null;
    }
    audioCtx = new audioContextCtor();
    masterGain = audioCtx.createGain();
    masterGain.gain.value = 1;
    masterGain.connect(audioCtx.destination);
    
    if (audioCtx.state === 'suspended') {
      audioCtx.resume();
    }
    
    return audioCtx;
  } catch (e) {
    console.warn('Audio not available:', e);
    return null;
  }
}

function startAmbientBed(ctx: AudioContext) {
  if (ambientNodes.length > 0) return;
  if (!masterGain) return;

  ambientGain = ctx.createGain();
  ambientGain.gain.value = 0.015;
  ambientGain.connect(masterGain);

  // Very low sub-hum — the cafeteria background
  const sub = ctx.createOscillator();
  sub.type = 'sine';
  sub.frequency.value = 50;
  const subGain = ctx.createGain();
  subGain.gain.value = 0.4;
  sub.connect(subGain);
  subGain.connect(ambientGain);
  sub.start();

  ambientNodes = [sub];
}

function stopAmbientBed() {
  for (const node of ambientNodes) {
    try { node.stop(); } catch (e) {}
  }
  ambientNodes = [];
  ambientGain = null;
}

function scaleAmbientIntensity() {
  if (!ambientGain) return;
  const count = microwaveHums.size;
  // Scale from 0.015 (1 microwave) to 0.06 (4+ microwaves)
  let target = Math.min(0.015 + count * 0.012, 0.06);
  
  // If the active thread has no active microwaves, dim the ambient bed too
  const hasActiveInThread = Array.from(microwaveHums.values()).some(h => h.threadId === currentAudibleThreadId);
  if (!hasActiveInThread && count > 0) {
    target *= 0.3; // Ghostly background
  }
  
  ambientGain.gain.setTargetAtTime(target, audioCtx!.currentTime, 0.1);
}

// ---------------------------------------------------------------------------
// Per-microwave hum — unique pitch per slot for spatial separation
// ---------------------------------------------------------------------------

const HUM_BASE_FREQ = 58;
const HUM_FREQ_SPREAD = 4; // Hz between each microwave's hum

export function setAudibleThread(threadId: string | null) {
  currentAudibleThreadId = threadId;
  const now = audioCtx ? audioCtx.currentTime : 0;

  for (const entry of microwaveHums.values()) {
    const isAudible = entry.threadId === threadId;
    const targetGain = isAudible ? 1.0 : 0.005; // Ghosting
    if (audioCtx) {
      entry.gain.gain.setTargetAtTime(targetGain, now, 0.1);
    } else {
      entry.gain.gain.value = targetGain;
    }
  }
  scaleAmbientIntensity();
}

export function startMicrowaveHum(requestId: string, config: MicrowaveAppConfig, threadId: string | null = null) {
  if (!config || config.microwave?.muted) return;
  if (microwaveHums.has(requestId)) return;

  const ctx = ensureContext();
  if (!ctx || !masterGain) return;

  // Start ambient bed on first microwave
  if (microwaveHums.size === 0) {
    startAmbientBed(ctx);
  }

  const slot = microwaveHums.size;
  const humAssetId = config.microwave?.humId;
  const humAsset = config.assets?.find((a: AssetConfig) => a.id === humAssetId);

  // The base container gain for this microwave
  const perMicGain = ctx.createGain();
  const isAudible = threadId === currentAudibleThreadId;
  perMicGain.gain.value = isAudible ? 1.0 : 0.005; 
  perMicGain.connect(masterGain);

  // Inner gain for slot-based volume scaling (avoid clipping)
  const slotGain = ctx.createGain();
  slotGain.gain.value = Math.max(0.03, 0.08 - slot * 0.012);
  slotGain.connect(perMicGain);

  const nodes: MicrowaveNode[] = [];

  if (humAsset) {
    const audio = new Audio(convertFileSrc(humAsset.path));
    audio.loop = true;
    const source = ctx.createMediaElementSource(audio);
    source.connect(slotGain);
    audio.play();
    nodes.push(audio);
  } else {
    // Procedural hum: unique frequency per slot
    const freq = HUM_BASE_FREQ + slot * HUM_FREQ_SPREAD;

    // Brown noise layer
    const bufferSize = ctx.sampleRate * 2;
    const noiseBuffer = ctx.createBuffer(1, bufferSize, ctx.sampleRate);
    const data = noiseBuffer.getChannelData(0);
    let brown = 0;
    for (let i = 0; i < bufferSize; i++) {
      const white = Math.random() * 2 - 1;
      brown = (brown + (0.02 * white)) / 1.02;
      data[i] = (brown * 0.7 + white * 0.3) * 3.5;
    }
    const noise = ctx.createBufferSource();
    noise.buffer = noiseBuffer;
    noise.loop = true;

    const noiseFilter = ctx.createBiquadFilter();
    noiseFilter.type = 'lowpass';
    noiseFilter.frequency.value = 350 + slot * 50;
    noiseFilter.Q.value = 0.5;

    const noiseGain = ctx.createGain();
    noiseGain.gain.value = 0.6;

    noise.connect(noiseFilter);
    noiseFilter.connect(noiseGain);
    noiseGain.connect(slotGain);
    noise.start();

    // Sine hum
    const hum = ctx.createOscillator();
    hum.type = 'sine';
    hum.frequency.value = freq;
    const humGain = ctx.createGain();
    humGain.gain.value = 0.3;
    hum.connect(humGain);
    humGain.connect(slotGain);
    hum.start();

    nodes.push(noise, hum);
  }

  microwaveHums.set(requestId, { nodes, gain: perMicGain, threadId });
  scaleAmbientIntensity();
}

export function stopMicrowaveHum(requestId: string) {
  const entry = microwaveHums.get(requestId);
  if (!entry) return;

  const now = audioCtx ? audioCtx.currentTime : 0;
  // Ramp down immediately to prevent clicking or trailing noise
  if (audioCtx) {
    entry.gain.gain.setTargetAtTime(0, now, 0.05);
  }

  // Delay actual node stopping slightly to allow the ramp
  setTimeout(() => {
    for (const node of entry.nodes) {
      try {
        if (node instanceof HTMLMediaElement) {
          node.pause();
          node.currentTime = 0;
        } else {
          node.stop();
        }
      } catch (e) {}
    }
  }, 100);

  microwaveHums.delete(requestId);
  scaleAmbientIntensity();

  // Kill ambient bed when last microwave stops
  if (microwaveHums.size === 0) {
    stopAmbientBed();
  }
}

// ---------------------------------------------------------------------------
// Legacy compat — stop ALL microwaves
// ---------------------------------------------------------------------------

export function stopMicrowaveAudio(closeContext = true) {
  for (const [id] of microwaveHums) {
    stopMicrowaveHum(id);
  }
  stopAmbientBed();

  if (closeContext && audioCtx) {
    try { audioCtx.close(); } catch (e) {}
    audioCtx = null;
    masterGain = null;
  }
}

// Legacy compat — start a single hum (uses 'global' id)
export function startMicrowaveAudio(config: MicrowaveAppConfig) {
  startMicrowaveHum('__global__', config);
}

// ---------------------------------------------------------------------------
// Ding — per-microwave completion chime with slight pitch variation
// ---------------------------------------------------------------------------

const DING_BASE_FREQ = 1200;
const DING_FREQ_SPREAD = 80;

export function playDing(config: MicrowaveAppConfig, slot = 0) {
  if (!audioCtx || !masterGain || config?.microwave?.muted) return;

  try {
    const dingAssetId = config?.microwave?.dingId;
    const dingAsset = config?.assets?.find((a: AssetConfig) => a.id === dingAssetId);

    if (dingAsset) {
      const ding = new Audio(convertFileSrc(dingAsset.path));
      const source = audioCtx.createMediaElementSource(ding);
      source.connect(masterGain);
      ding.play();
    } else {
      const now = audioCtx.currentTime;
      const g = audioCtx.createGain();
      g.gain.setValueAtTime(0, now);
      g.gain.linearRampToValueAtTime(0.2, now + 0.02);
      g.gain.exponentialRampToValueAtTime(0.001, now + 0.8);
      g.connect(masterGain);

      // Slightly different pitch per slot for spatial differentiation
      const freq = DING_BASE_FREQ + slot * DING_FREQ_SPREAD;
      const o = audioCtx.createOscillator();
      o.type = 'sine';
      o.frequency.setValueAtTime(freq, now);
      o.frequency.exponentialRampToValueAtTime(freq - 20, now + 0.8);
      o.connect(g);
      o.start(now);
      o.stop(now + 0.8);
    }
  } catch (e) {}
}

// Error buzz — short dissonant tone
export function playErrorBuzz(config: MicrowaveAppConfig) {
  if (!audioCtx || !masterGain || config?.microwave?.muted) return;

  try {
    const now = audioCtx.currentTime;
    const g = audioCtx.createGain();
    g.gain.setValueAtTime(0, now);
    g.gain.linearRampToValueAtTime(0.12, now + 0.01);
    g.gain.exponentialRampToValueAtTime(0.001, now + 0.4);
    g.connect(masterGain);

    const o = audioCtx.createOscillator();
    o.type = 'sawtooth';
    o.frequency.setValueAtTime(180, now);
    o.frequency.linearRampToValueAtTime(120, now + 0.4);
    o.connect(g);
    o.start(now);
    o.stop(now + 0.4);
  } catch (e) {}
}

export function getAudioCtx() {
  return audioCtx;
}

export function startRequestHum(requestId: string, config: MicrowaveAppConfig, threadId: string | null = null) {
  startMicrowaveHum(requestId, config, threadId);
}

export function stopRequestHum(requestId: string, success: boolean, config: MicrowaveAppConfig, slot = 0) {
  stopMicrowaveHum(requestId);
  if (!config?.microwave?.muted) {
    if (success) playDing(config, slot);
    else playErrorBuzz(config);
  }
}

export function getActiveMicrowaveCount(): number {
  return microwaveHums.size;
}

export function setMuted(muted: boolean) {
  if (muted) {
    stopMicrowaveAudio(false); // Stop all but keep context? Actually stopMicrowaveAudio closes context by default.
  }
}
