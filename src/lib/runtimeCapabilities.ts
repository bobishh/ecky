import type {
  AppConfig,
  GeometryBackend,
  RuntimeAuthoringContext,
  RuntimeBackendCapability,
  RuntimeCapabilities,
  SourceLanguage,
} from './types/domain';

export function authoringContextFromConfig(config: Pick<
  AppConfig,
  'defaultEngineKind' | 'defaultSourceLanguage' | 'defaultGeometryBackend'
>): RuntimeAuthoringContext {
  return {
    engineKind: config.defaultEngineKind,
    sourceLanguage: config.defaultSourceLanguage,
    geometryBackend: config.defaultGeometryBackend,
  };
}

export function capabilityForAuthoringContext(
  capabilities: RuntimeCapabilities | null | undefined,
  sourceLanguage: SourceLanguage,
  geometryBackend: GeometryBackend,
): RuntimeBackendCapability | null {
  if (!capabilities) return null;
  if (sourceLanguage === 'legacyPython') return capabilities.freecad;
  if (sourceLanguage === 'build123d') return capabilities.build123d;
  if (sourceLanguage === 'ecky') {
    return geometryBackend === 'build123d'
      ? capabilities.build123d
      : geometryBackend === 'freecad'
        ? capabilities.freecad
        : capabilities.mesh;
  }
  if (geometryBackend === 'build123d') return capabilities.build123d;
  if (geometryBackend === 'freecad') return capabilities.freecad;
  return capabilities.mesh;
}

export function repairDefaultAuthoringContext(
  config: AppConfig,
  capabilities: RuntimeCapabilities,
): { config: AppConfig; repaired: boolean } {
  const currentContext = authoringContextFromConfig(config);
  const currentCapability = capabilityForAuthoringContext(
    capabilities,
    currentContext.sourceLanguage,
    currentContext.geometryBackend,
  );

  if (currentCapability?.available) {
    return { config, repaired: false };
  }

  return {
    repaired: true,
    config: {
      ...config,
      defaultEngineKind: capabilities.recommendedAuthoringContext.engineKind,
      defaultSourceLanguage: capabilities.recommendedAuthoringContext.sourceLanguage,
      defaultGeometryBackend: capabilities.recommendedAuthoringContext.geometryBackend,
    },
  };
}
