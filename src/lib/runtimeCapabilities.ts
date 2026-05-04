import type {
  AppConfig,
  ArtifactBundle,
  EngineKind,
  GeometryBackend,
  Message,
  ModelManifest,
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

type AuthoringContextSource = Partial<
  Pick<RuntimeAuthoringContext, 'engineKind' | 'sourceLanguage' | 'geometryBackend'>
>;

type ActiveAuthoringContextInput = {
  config: Pick<AppConfig, 'defaultEngineKind' | 'defaultSourceLanguage' | 'defaultGeometryBackend'>;
  activeVersionMessage?: Pick<Message, 'output' | 'artifactBundle' | 'modelManifest'> | null;
  sessionArtifactBundle?: ArtifactBundle | null;
  sessionModelManifest?: ModelManifest | null;
};

function engineKindForSourceLanguage(sourceLanguage: SourceLanguage | undefined): EngineKind | null {
  if (sourceLanguage === 'ecky') return 'ecky';
  if (sourceLanguage === 'build123d') return 'build123d';
  if (sourceLanguage === 'legacyPython') return 'freecad';
  return null;
}

function applyAuthoringContextSource(
  context: RuntimeAuthoringContext,
  source: AuthoringContextSource | null | undefined,
): RuntimeAuthoringContext {
  if (!source) return context;
  const sourceLanguage = source.sourceLanguage ?? context.sourceLanguage;
  return {
    engineKind:
      source.engineKind ??
      (source.sourceLanguage ? engineKindForSourceLanguage(source.sourceLanguage) : null) ??
      context.engineKind,
    sourceLanguage,
    geometryBackend: source.geometryBackend ?? context.geometryBackend,
  };
}

function sameRuntimeModel(
  selectedModelId: string | null,
  artifactBundle: ArtifactBundle | null | undefined,
  modelManifest: ModelManifest | null | undefined,
): boolean {
  if (!selectedModelId) return true;
  return artifactBundle?.modelId === selectedModelId || modelManifest?.modelId === selectedModelId;
}

export function resolveActiveAuthoringContext({
  config,
  activeVersionMessage,
  sessionArtifactBundle,
  sessionModelManifest,
}: ActiveAuthoringContextInput): RuntimeAuthoringContext {
  const selectedModelId =
    activeVersionMessage?.artifactBundle?.modelId ??
    activeVersionMessage?.modelManifest?.modelId ??
    null;
  const useSessionRuntime = sameRuntimeModel(
    selectedModelId,
    sessionArtifactBundle,
    sessionModelManifest,
  );

  let context = authoringContextFromConfig(config);
  context = applyAuthoringContextSource(context, activeVersionMessage?.output);
  context = applyAuthoringContextSource(context, activeVersionMessage?.modelManifest);
  context = applyAuthoringContextSource(context, activeVersionMessage?.artifactBundle);
  if (useSessionRuntime) {
    context = applyAuthoringContextSource(context, sessionModelManifest);
    context = applyAuthoringContextSource(context, sessionArtifactBundle);
  }
  return context;
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
