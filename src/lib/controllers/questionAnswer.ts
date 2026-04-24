function normalizeText(value: string | null | undefined): string {
  return `${value ?? ''}`.trim();
}

export function needsGeneratedQuestionAnswer(finalResponse: string | null | undefined): boolean {
  return normalizeText(finalResponse).length === 0;
}

export function pendingQuestionCopy(finalResponse: string | null | undefined): string {
  const finalAnswer = normalizeText(finalResponse);
  return finalAnswer || 'Answering question...';
}
