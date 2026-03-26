import { writable } from 'svelte/store';
import { session } from './sessionStore';

export const activePhrase = writable('');

const COOKING_PHRASES = [
  "Packing constraints and dimensions into a fresh build plan.",
  "Tracing connector paths and locking wall thickness.",
  "Balancing tolerances so parts print clean and snap right.",
  "Checking manifold integrity and shell continuity.",
  "Projecting cuts and bores onto stable reference axes.",
  "Compiling a safer BRep sequence for FreeCAD execution.",
  "Revalidating clearances to avoid accidental intersections.",
  "Aligning param ranges with current geometry intent.",
  "Running edge cleanup before final mesh output.",
  "Rebuilding topology around your latest parameter edits.",
  "Testing the draft against connector and ring constraints."
];

const LIGHT_REASONING_PHRASES = [
  "Thinking not deep enough. Deciding if this is a question or a geometry change.",
  "Running a quick intent check before heavy generation.",
  "Light pass active: classifying request type.",
  "Checking whether to explain or to modify geometry.",
  "Fast reasoning mode: routing request.",
  "Consulting the goblin responsible for causality."
];

let phraseInterval: ReturnType<typeof setInterval> | null = null;

function pickPhrase(pool: string[]) {
  const phrase = pool[Math.floor(Math.random() * pool.length)];
  activePhrase.set(phrase);
  session.setCookingPhrase(phrase); // Also update session store for backwards compatibility if needed
}

export function startLightReasoningPhraseLoop() {
  if (phraseInterval) clearInterval(phraseInterval);
  pickPhrase(LIGHT_REASONING_PHRASES);
  phraseInterval = setInterval(() => pickPhrase(LIGHT_REASONING_PHRASES), 2600);
}

export function startCookingPhraseLoop() {
  if (phraseInterval) clearInterval(phraseInterval);
  pickPhrase(COOKING_PHRASES);
  phraseInterval = setInterval(() => pickPhrase(COOKING_PHRASES), 4000);
}

export function stopPhraseLoop() {
  if (phraseInterval) {
    clearInterval(phraseInterval);
    phraseInterval = null;
  }
}
