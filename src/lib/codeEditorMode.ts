export function usesPythonEditorMode(sourceLanguage: string | null | undefined): boolean {
  return sourceLanguage === 'legacyPython' || sourceLanguage === 'build123d';
}
