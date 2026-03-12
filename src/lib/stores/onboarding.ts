import { writable } from 'svelte/store';
import { config } from './domainState';
import { saveConfig } from '../boot/restore';
import { get } from 'svelte/store';

export interface OnboardingStep {
  id: string;
  text: string;
  target: string | null;
}

const STEPS: OnboardingStep[] = [
  {
    id: 'intro',
    text: "Welcome to Ecky CAD! I'm Ecky, your AI design assistant. Let me give you a quick tour of how we can build things together.",
    target: null,
  },
  {
    id: 'dialogue',
    text: "This is the Dialogue area. Just tell me what you want to create or change, and I'll write the FreeCAD Python code to make it happen.",
    target: 'dialogue',
  },
  {
    id: 'viewport',
    text: "Here is the Viewport. You'll see your 3D models appear here instantly. You can rotate, pan, and zoom to inspect the details.",
    target: 'viewport',
  },
  {
    id: 'params',
    text: "This is the Parameters panel. When I create a model, I'll often expose sliders and inputs here so you can tweak the design in real-time without writing new prompts.",
    target: 'params',
  },
  {
    id: 'history',
    text: "And finally, the Thread History. Every design variation is saved here automatically. You can always go back in time or branch off into new ideas.",
    target: 'history',
  },
  {
    id: 'finish',
    text: "That's it! Why don't we start by typing something simple below, like 'make a coffee cup'?",
    target: null,
  }
];

function createOnboardingStore() {
  const { subscribe, set, update } = writable({
    isActive: false,
    currentStepIndex: 0,
    target: null as string | null,
    text: ''
  });

  return {
    subscribe,
    start: () => {
      set({
        isActive: true,
        currentStepIndex: 0,
        target: STEPS[0].target,
        text: STEPS[0].text
      });
    },
    next: async () => {
      let isFinished = false;
      update(state => {
        const nextIndex = state.currentStepIndex + 1;
        if (nextIndex >= STEPS.length) {
          isFinished = true;
          return { isActive: false, currentStepIndex: 0, target: null, text: '' };
        }
        return {
          isActive: true,
          currentStepIndex: nextIndex,
          target: STEPS[nextIndex].target,
          text: STEPS[nextIndex].text
        };
      });

      if (isFinished) {
        await finishOnboarding();
      }
    },
    skip: async () => {
      set({ isActive: false, currentStepIndex: 0, target: null, text: '' });
      await finishOnboarding();
    }
  };
}

async function finishOnboarding() {
  config.update(c => ({ ...c, hasSeenOnboarding: true }));
  await saveConfig();
}

export const onboarding = createOnboardingStore();
