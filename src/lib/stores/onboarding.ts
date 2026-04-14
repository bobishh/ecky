import { writable } from 'svelte/store';
import { config } from './domainState';
import { saveConfig } from '../boot/restore';
import type { WindowId } from './windowStore';

export interface OnboardingStep {
  id: string;
  text: string;
  highlightTarget: OnboardingHighlightTarget | null;
  windowIdToOpen?: WindowId;
}

export type OnboardingHighlightTarget = 'dialogue' | 'viewport' | 'params' | 'projects';

type OnboardingState = {
  isActive: boolean;
  currentStepIndex: number;
  currentStepId: string | null;
  highlightTarget: OnboardingHighlightTarget | null;
  windowIdToOpen: WindowId | null;
  text: string;
};

type OnboardingDeps = {
  configStore?: {
    update: (updater: (value: any) => any) => void;
  };
  saveConfig?: () => Promise<void>;
};

export const ONBOARDING_STEPS: OnboardingStep[] = [
  {
    id: 'intro',
    text: "Welcome to Ecky CAD! I'm Ecky, your AI design assistant. Let me give you a quick tour of how we can build things together.",
    highlightTarget: null,
  },
  {
    id: 'dialogue',
    text: "This is the Dialogue window. Tell me what you want to create or change, and I'll work the thread from here with you.",
    highlightTarget: 'dialogue',
    windowIdToOpen: 'dialogue',
  },
  {
    id: 'viewport',
    text: "This is the Viewport. Your 3D model shows up here, and you can rotate, pan, and zoom to inspect it.",
    highlightTarget: 'viewport',
  },
  {
    id: 'params',
    text: "This is the Parameters window. When a model exposes controls, you can tweak dimensions and options here without writing another prompt.",
    highlightTarget: 'params',
    windowIdToOpen: 'params',
  },
  {
    id: 'projects',
    text: "This is the Projects window. Your active threads, archived work, and trash live here, so you can return to old ideas or branch new ones.",
    highlightTarget: 'projects',
    windowIdToOpen: 'projects',
  },
  {
    id: 'finish',
    text: "That's it! Why don't we start by typing something simple below, like 'make a coffee cup'?",
    highlightTarget: null,
  }
];

function stateForStep(stepIndex: number): OnboardingState {
  const step = ONBOARDING_STEPS[stepIndex];
  return {
    isActive: false,
    currentStepIndex: stepIndex,
    currentStepId: step?.id ?? null,
    highlightTarget: step?.highlightTarget ?? null,
    windowIdToOpen: step?.windowIdToOpen ?? null,
    text: step?.text ?? '',
  };
}

export function createOnboardingStore(deps: OnboardingDeps = {}) {
  const configStore = deps.configStore ?? config;
  const persistOnboarding = deps.saveConfig ?? saveConfig;
  const { subscribe, set, update } = writable<OnboardingState>({
    ...stateForStep(0),
    isActive: false,
  });

  return {
    subscribe,
    start: () => {
      set({
        ...stateForStep(0),
        isActive: true,
      });
    },
    next: async () => {
      let isFinished = false;
      update(state => {
        const nextIndex = state.currentStepIndex + 1;
        if (nextIndex >= ONBOARDING_STEPS.length) {
          isFinished = true;
          return {
            ...stateForStep(0),
            isActive: false,
          };
        }
        return {
          ...stateForStep(nextIndex),
          isActive: true,
        };
      });

      if (isFinished) {
        await finishOnboarding(configStore, persistOnboarding);
      }
    },
    skip: async () => {
      set({
        ...stateForStep(0),
        isActive: false,
      });
      await finishOnboarding(configStore, persistOnboarding);
    }
  };
}

async function finishOnboarding(
  configStore: OnboardingDeps['configStore'],
  persistOnboarding: () => Promise<void>,
) {
  configStore?.update((current) => ({ ...current, hasSeenOnboarding: true }));
  await persistOnboarding();
}

export const onboarding = createOnboardingStore();
