export type SketchPreviewStepState = 'blocked' | 'idle' | 'queued' | 'generating' | 'accepted' | 'failed';

export type SketchPreviewStepInput = {
  hasClosedProfile: boolean;
  hasDraft: boolean;
  generating: boolean;
  errorText: string;
  autoQueued: boolean;
};

export type SketchPreviewStepSummary = {
  state: SketchPreviewStepState;
  label: string;
  detail: string;
};

export function summarizeSketchPreviewStep(input: SketchPreviewStepInput): SketchPreviewStepSummary {
  if (input.errorText) {
    return {
      state: 'failed',
      label: 'PREVIEW FAILED',
      detail: input.errorText,
    };
  }

  if (input.generating) {
    return {
      state: 'generating',
      label: 'GENERATING PREVIEW',
      detail: 'Preview request in flight.',
    };
  }

  if (input.hasDraft) {
    return {
      state: 'accepted',
      label: 'PREVIEW READY',
      detail: 'Draft accepted; preview ready.',
    };
  }

  if (input.autoQueued) {
    return {
      state: 'queued',
      label: 'AUTO-PREVIEW QUEUED',
      detail: 'Closed profile queued for preview.',
    };
  }

  if (!input.hasClosedProfile) {
    return {
      state: 'blocked',
      label: 'PROFILE OPEN',
      detail: 'Close profile before preview.',
    };
  }

  return {
    state: 'idle',
    label: 'READY FOR PREVIEW',
    detail: 'Closed profile ready for preview.',
  };
}
