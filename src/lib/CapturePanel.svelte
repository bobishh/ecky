<script lang="ts">
  import Viewer from './Viewer.svelte';
  import './externalShapes.css';
  import AsyncActionButton from './components/AsyncActionButton.svelte';
  import PuppetBuilder from './puppet/PuppetBuilder.svelte';
  import { formatBackendError } from './tauri/client';
  import type {
    CaptureCropBounds,
    CaptureExpectedGeometryKind,
    CaptureGuideResultProvenance,
    CaptureLandmarkRole,
    CaptureMeshPreview,
    CaptureObservedDeviationReport,
    CaptureProfileKind,
    CaptureProfileOperationHint,
    CaptureReconstructionGuide,
    CaptureReconstructionGuideState,
    CaptureRequiredBrepTopologyKind,
    CaptureSelectorCardinality,
    CaptureSurfaceAnchor,
    ExternalShapeSource,
    ExternalShapeSurfaceTrim,
    SurfaceTrimCapMode,
    SurfaceTrimLoopPreviewResponse,
    SurfaceTrimLoopSegmentPreview,
    SurfaceTrimPathMode,
    SurfaceTrimPathPreviewResponse,
    SurfaceTrimRegionPreviewResponse,
  } from './tauri/contracts';
  import type {
    CaptureFeatureExpectationEdit,
    CaptureLandmarkEdit,
    CaptureProfileEdit,
  } from './capture/captureGuideDraft';
  import {
    addSurfaceTrimAnchor,
    beginSurfaceTrimRegionSelection,
    canApplySurfaceTrim,
    canCloseSurfaceTrimBoundary,
    cancelSurfaceTrim,
    chooseSurfaceTrimKeepSeed,
    closeSurfaceTrimBoundary,
    createSurfaceTrimInteraction,
    markSurfaceTrimApplying,
    markSurfaceTrimPreviewReady,
    moveSurfaceTrimAnchor,
    removeSurfaceTrimAnchor,
    setSurfaceTrimError,
    undoSurfaceTrimAnchor,
    type SurfaceTrimInteraction,
  } from './capture/surfaceTrimInteraction';
  type CaptureFrameStat = {
    label: string;
    value: string;
  };
  type ExternalShapeStep = 'import' | 'capture' | 'crop' | 'puppet';
  type CaptureStep = 'scan' | 'guides';

  let {
    sessionState = 'pairing',
    sessionActive = false,
    reconstructionProgress = 0,
    pairingUrl = 'No pairing session yet',
    trustUrl = '',
    cameraStatus = 'Camera permission pending',
    guidance = 'PAIR PHONE',
    stats = [
      { label: 'Light', value: 'WAIT' },
      { label: 'Motion', value: 'HOLD' },
      { label: 'Coverage', value: '0%' },
    ],
    onStartCapture = () => {},
    visible = true,
    captureRunId = '',
    currentModelPath = '',
    onOpenLastCapture = () => {},
    onCancelCapture = () => {},
    onApplyPreview = () => {},
    onCommitPreview = () => {},
    onRetryReconstruction = () => {},
    onAddPhotos = () => {},
    onPreviewLoadError = () => {},
    meshPreview = null,
    previewModelKey = null,
    previewUrl = null,
    externalShapeSources = [],
    selectedExternalShapeNodeId = null,
    externalShapePreviewUrl = null,
    externalShapeRawPreviewUrl = null,
    externalShapeTargetMessageId = null,
    externalShapePreviewIsCropped = false,
    externalShapeError = '',
    onSelectExternalShape = () => {},
    onApplyExternalPlaneCrop = async () => {},
    onRemoveExternalPlaneCrop = async () => {},
    onPreviewExternalSurfaceTrimPath = async () => { throw new Error('Surface trim path preview unavailable.'); },
    onPreviewExternalSurfaceTrimLoop = async () => { throw new Error('Surface trim loop preview unavailable.'); },
    onPreviewExternalSurfaceTrimRegion = async () => { throw new Error('Surface trim region preview unavailable.'); },
    onApplyExternalSurfaceTrim = async () => {},
    onRemoveExternalSurfaceTrim = async () => {},
    previewApplied = false,
    applyPending = false,
    previewScale = 0.05,
    cropEnabled = false,
    cropMode = 'scale',
    cropBounds = null,
    cropDirty = false,
    onPreviewScaleChange = () => {},
    onCropEnabledChange = () => {},
    onCropModeChange = () => {},
    onCropBoundsChange = () => {},
    onPreviewCrop = () => {},
    onResetCrop = () => {},
    guideMode = false,
    guide = null,
    guideState = null,
    guidePickRole = 'calibrationEndpoint',
    guideReady = false,
    guideReadinessReasons = [],
    guideKnownDistanceMm = 40,
    guideFeatureDepthMm = 18,
    guideInstruction = '',
    guideError = '',
    guideComparisonError = '',
    guideCanUndo = false,
    guideSelectedLandmarkId = null,
    guideComparisonModelKey = null,
    guideComparisonUrl = null,
    guideResult = null,
    guideDeviation = null,
    guideDeviationVisible = true,
    guideReferenceVisible = true,
    guideReferenceOpacity = 0.28,
    guideGeneratedVisible = true,
    guideGeneratedOpacity = 1,
    onStartGuidedCad = () => {},
    onGuidePickRoleChange = () => {},
    onGuideAnchor = () => {},
    onGuideAnchorError = () => {},
    onGuideSelectLandmark = () => {},
    onGuideEditLandmark = () => {},
    onGuideDeleteLandmark = () => {},
    onGuideUndo = () => {},
    onGuideEditProfile = () => {},
    onGuideMoveProfileLandmark = () => {},
    onGuideEditExpectation = () => {},
    onGuideSelectFeaturePlan = () => {},
    onGuideReferenceVisibleChange = () => {},
    onGuideReferenceOpacityChange = () => {},
    onGuideGeneratedVisibleChange = () => {},
    onGuideGeneratedOpacityChange = () => {},
    onGuideDeviationVisibleChange = () => {},
    onGuideKnownDistanceChange = () => {},
    onGuideFeatureDepthChange = () => {},
    onGuideInstructionChange = () => {},
    onValidateGuide = () => {},
    onBuildCadFromGuide = () => {},
  }: {
    sessionState?: 'pairing' | 'capturing' | 'reconstructing' | 'preview' | 'failed' | 'cancelled';
    pairingUrl?: string;
    trustUrl?: string;
    cameraStatus?: string;
    guidance?: string;
    stats?: CaptureFrameStat[];
    sessionActive?: boolean;
    reconstructionProgress?: number;
    onStartCapture?: () => void | Promise<void>;
    visible?: boolean;
    captureRunId?: string;
    currentModelPath?: string;
    onOpenLastCapture?: () => void;
    onCancelCapture?: () => void;
    onApplyPreview?: () => void;
    onCommitPreview?: () => void;
    onRetryReconstruction?: () => void;
    onAddPhotos?: () => void;
    onPreviewLoadError?: (message: string) => void;
    meshPreview?: CaptureMeshPreview | null;
    previewModelKey?: string | null;
    previewUrl?: string | null;
    externalShapeSources?: ExternalShapeSource[];
    selectedExternalShapeNodeId?: number | null;
    externalShapePreviewUrl?: string | null;
    externalShapeRawPreviewUrl?: string | null;
    externalShapeTargetMessageId?: string | null;
    externalShapePreviewIsCropped?: boolean;
    externalShapeError?: string;
    onSelectExternalShape?: (nodeId: number) => void;
    onApplyExternalPlaneCrop?: (
      anchors: CaptureSurfaceAnchor[],
      keepPositive: boolean,
      replaceCropNodeId: number | null,
    ) => Promise<void>;
    onRemoveExternalPlaneCrop?: (cropNodeId: number) => Promise<void>;
    onPreviewExternalSurfaceTrimPath?: (
      fromAnchor: CaptureSurfaceAnchor,
      toAnchor: CaptureSurfaceAnchor,
      pathMode: SurfaceTrimPathMode,
      previewId: number,
      targetMessageId: string | null,
    ) => Promise<SurfaceTrimPathPreviewResponse>;
    onPreviewExternalSurfaceTrimLoop?: (
      loopAnchors: CaptureSurfaceAnchor[],
      pathMode: SurfaceTrimPathMode,
      previewId: number,
      targetMessageId: string | null,
    ) => Promise<SurfaceTrimLoopPreviewResponse>;
    onPreviewExternalSurfaceTrimRegion?: (
      loopAnchors: CaptureSurfaceAnchor[],
      keepSeed: CaptureSurfaceAnchor,
      pathMode: SurfaceTrimPathMode,
      capMode: SurfaceTrimCapMode,
      previewId: number,
      targetMessageId: string | null,
    ) => Promise<SurfaceTrimRegionPreviewResponse>;
    onApplyExternalSurfaceTrim?: (
      loopAnchors: CaptureSurfaceAnchor[],
      keepSeed: CaptureSurfaceAnchor,
      pathMode: SurfaceTrimPathMode,
      capMode: SurfaceTrimCapMode,
      replaceTrimNodeId: number | null,
      targetMessageId: string | null,
    ) => Promise<void>;
    onRemoveExternalSurfaceTrim?: (trimNodeId: number) => Promise<void>;
    previewApplied?: boolean;
    applyPending?: boolean;
    previewScale?: number;
    cropEnabled?: boolean;
    cropMode?: 'translate' | 'scale';
    cropBounds?: CaptureCropBounds | null;
    cropDirty?: boolean;
    onPreviewScaleChange?: (scale: number) => void;
    onCropEnabledChange?: (enabled: boolean) => void;
    onCropModeChange?: (mode: 'translate' | 'scale') => void;
    onCropBoundsChange?: (bounds: CaptureCropBounds) => void;
    onPreviewCrop?: () => void;
    onResetCrop?: () => void;
    guideMode?: boolean;
    guide?: CaptureReconstructionGuide | null;
    guideState?: CaptureReconstructionGuideState | null;
    guidePickRole?: CaptureLandmarkRole;
    guideReady?: boolean;
    guideReadinessReasons?: string[];
    guideKnownDistanceMm?: number;
    guideFeatureDepthMm?: number;
    guideInstruction?: string;
    guideError?: string;
    guideComparisonError?: string;
    guideCanUndo?: boolean;
    guideSelectedLandmarkId?: string | null;
    guideComparisonModelKey?: string | null;
    guideComparisonUrl?: string | null;
    guideResult?: CaptureGuideResultProvenance | null;
    guideDeviation?: CaptureObservedDeviationReport | null;
    guideDeviationVisible?: boolean;
    guideReferenceVisible?: boolean;
    guideReferenceOpacity?: number;
    guideGeneratedVisible?: boolean;
    guideGeneratedOpacity?: number;
    onStartGuidedCad?: () => void;
    onGuidePickRoleChange?: (role: CaptureLandmarkRole) => void;
    onGuideAnchor?: (anchor: CaptureSurfaceAnchor) => void;
    onGuideAnchorError?: (message: string) => void;
    onGuideSelectLandmark?: (landmarkId: string) => void;
    onGuideEditLandmark?: (landmarkId: string, edit: CaptureLandmarkEdit) => void;
    onGuideDeleteLandmark?: (landmarkId: string) => void;
    onGuideUndo?: () => void;
    onGuideEditProfile?: (profileId: string, edit: CaptureProfileEdit) => void;
    onGuideMoveProfileLandmark?: (profileId: string, landmarkId: string, targetIndex: number) => void;
    onGuideEditExpectation?: (expectationId: string, edit: CaptureFeatureExpectationEdit) => void;
    onGuideSelectFeaturePlan?: (planId: string) => void;
    onGuideReferenceVisibleChange?: (visible: boolean) => void;
    onGuideReferenceOpacityChange?: (opacity: number) => void;
    onGuideGeneratedVisibleChange?: (visible: boolean) => void;
    onGuideGeneratedOpacityChange?: (opacity: number) => void;
    onGuideDeviationVisibleChange?: (visible: boolean) => void;
    onGuideKnownDistanceChange?: (value: number) => void;
    onGuideFeatureDepthChange?: (value: number) => void;
    onGuideInstructionChange?: (value: string) => void;
    onValidateGuide?: () => void;
    onBuildCadFromGuide?: () => void;
  } = $props();

  const landmarkRoles: CaptureLandmarkRole[] = [
    'calibrationEndpoint',
    'frameOrigin',
    'frameDirection',
    'symmetrySample',
    'rotationAxisEndpoint',
    'profileVertex',
    'matingSurfaceSample',
    'boreSample',
    'outerExtent',
    'clearanceBoundary',
    'ignoredDamagedRegion',
    'namedReference',
  ];
  const profileKinds: CaptureProfileKind[] = ['open', 'closed'];
  const profileOperations: CaptureProfileOperationHint[] = [
    'extrude', 'revolve', 'sweep', 'referenceOnly', 'agentDecide',
  ];
  const expectedGeometryKinds: CaptureExpectedGeometryKind[] = [
    'point', 'curve', 'plane', 'cylinder', 'profile',
  ];
  const topologyKinds: CaptureRequiredBrepTopologyKind[] = [
    'vertex', 'edge', 'face', 'orderedEdges',
  ];
  const cardinalities: CaptureSelectorCardinality[] = ['one', 'oneOrMore'];

  let previewStatus = $state<'idle' | 'loading' | 'loaded' | 'failed'>('idle');
  let previewError = $state('');
  let workflowStep = $state<ExternalShapeStep>('capture');
  let captureStep = $state<CaptureStep>('scan');
  let planePickerActive = $state(false);
  let planeAnchors = $state<CaptureSurfaceAnchor[]>([]);
  let planeKeepPositive = $state(true);
  let planeApplying = $state(false);
  let planeError = $state('');
  let editingCropNodeId = $state<number | null>(null);
  let surfaceTrimActive = $state(false);
  let surfaceTrimState = $state<SurfaceTrimInteraction>(createSurfaceTrimInteraction());
  let surfaceTrimCommittedSegments = $state<SurfaceTrimLoopSegmentPreview[]>([]);
  let surfaceTrimLoopSegments = $state<SurfaceTrimLoopSegmentPreview[]>([]);
  let surfaceTrimRetainedTriangleIndices = $state<number[]>([]);
  let surfaceTrimPreview = $state<SurfaceTrimRegionPreviewResponse | null>(null);
  let surfaceTrimSelectedAnchorIndex = $state<number | null>(null);
  let surfaceTrimMoveAnchorIndex = $state<number | null>(null);
  let surfaceTrimApplying = $state(false);
  let surfaceTrimPreviewId = 0;
  let surfaceTrimLatestRequestedPreviewId = 0;
  let surfaceTrimOperationToken = 0;
  let surfaceTrimTargetMessageId = $state<string | null>(null);
  const workflowSteps: Array<{ id: ExternalShapeStep; label: string }> = [
    { id: 'import', label: 'IMPORT' },
    { id: 'capture', label: 'CAPTURE' },
    { id: 'crop', label: 'CROP' },
    { id: 'puppet', label: 'PUPPET' },
  ];
  const captureSteps: Array<{ id: CaptureStep; label: string }> = [
    { id: 'scan', label: 'SCAN' },
    { id: 'guides', label: 'GUIDED BREP' },
  ];
  const captureScanActive = $derived(workflowStep === 'capture' && captureStep === 'scan');
  const captureGuidesActive = $derived(workflowStep === 'capture' && captureStep === 'guides');
  let actionError = $state('');
  const idleCapture = $derived(!sessionActive && (sessionState === 'pairing' || sessionState === 'cancelled'));
  const frameCount = $derived(stats.find(stat => stat.label === 'Frames')?.value ?? '0');
  const captureHeading = $derived(sessionState === 'failed' ? 'Capture needs attention'
    : idleCapture ? 'Scan a real object'
    : sessionState === 'reconstructing' ? 'Building your scan'
    : !sessionActive ? 'Starting capture…'
    : frameCount !== '0' ? 'Capture your object' : 'Connect your phone');

  const selectedExternalShape = $derived(
    externalShapeSources.find(source => source.nodeId === selectedExternalShapeNodeId) ?? null,
  );
  const selectedPlaneCrops = $derived(selectedExternalShape?.planeCrops ?? []);
  const selectedSurfaceTrims = $derived(selectedExternalShape?.surfaceTrims ?? []);
  const surfaceTrimCapReport = $derived(
    surfaceTrimPreview?.capReports.find(report => report.mode === surfaceTrimState.capMode) ?? null,
  );
  const activePreviewUrl = $derived(
    workflowStep === 'capture'
      ? previewUrl
      : ((planePickerActive || surfaceTrimActive ? externalShapeRawPreviewUrl : externalShapePreviewUrl) ?? previewUrl),
  );
  const scanSetup = $derived(captureScanActive && !activePreviewUrl);
  const activePreviewModelKey = $derived(
    workflowStep !== 'capture' && selectedExternalShape
      ? `external-shape:${selectedExternalShape.nodeId}:${selectedExternalShape.sourceDigest}:${selectedPlaneCrops.length}:${selectedSurfaceTrims.length}`
      : previewModelKey,
  );
  const activeSourceName = $derived(
    workflowStep !== 'capture' && selectedExternalShape
      ? selectedExternalShape.displayName
      : (meshPreview ? 'CAPTURE MESH' : ''),
  );

  $effect(() => {
    previewStatus = activePreviewUrl ? 'loading' : 'idle';
    previewError = '';
  });

  $effect(() => {
    void selectedExternalShapeNodeId;
    planePickerActive = false;
    planeAnchors = [];
    planeKeepPositive = true;
    planeError = '';
    editingCropNodeId = null;
    surfaceTrimActive = false;
    surfaceTrimState = createSurfaceTrimInteraction();
    surfaceTrimCommittedSegments = [];
    surfaceTrimLoopSegments = [];
    surfaceTrimRetainedTriangleIndices = [];
    surfaceTrimPreview = null;
    surfaceTrimSelectedAnchorIndex = null;
    surfaceTrimMoveAnchorIndex = null;
    surfaceTrimApplying = false;
    surfaceTrimTargetMessageId = null;
    surfaceTrimLatestRequestedPreviewId = ++surfaceTrimPreviewId;
  });

  function startPlanePicker(cropNodeId: number | null = null, keepPositive = true) {
    cancelSurfaceTrimInteraction();
    planePickerActive = true;
    planeAnchors = [];
    planeKeepPositive = keepPositive;
    planeError = '';
    editingCropNodeId = cropNodeId;
  }

  function cancelSurfaceTrimInteraction() {
    surfaceTrimActive = false;
    surfaceTrimState = cancelSurfaceTrim(surfaceTrimState);
    surfaceTrimCommittedSegments = [];
    surfaceTrimLoopSegments = [];
    surfaceTrimRetainedTriangleIndices = [];
    surfaceTrimPreview = null;
    surfaceTrimSelectedAnchorIndex = null;
    surfaceTrimMoveAnchorIndex = null;
    surfaceTrimApplying = false;
    surfaceTrimTargetMessageId = null;
    surfaceTrimLatestRequestedPreviewId = ++surfaceTrimPreviewId;
    surfaceTrimOperationToken += 1;
  }

  function restoreSurfaceTrimAnchor(
    trim: ExternalShapeSurfaceTrim,
    anchor: ExternalShapeSurfaceTrim['loopAnchors'][number],
  ): CaptureSurfaceAnchor | null {
    if (!anchor.sourcePosition || !anchor.sourceNormal) return null;
    return {
      sourceMeshContentDigest: trim.sourceDigest,
      triangleIndex: anchor.triangleIndex,
      barycentric: [...anchor.barycentric] as [number, number, number],
      sourcePosition: [...anchor.sourcePosition] as [number, number, number],
      sourceNormal: [...anchor.sourceNormal] as [number, number, number],
    };
  }

  async function startSurfaceTrim(trim: ExternalShapeSurfaceTrim | null = null) {
    planePickerActive = false;
    planeAnchors = [];
    planeError = '';
    editingCropNodeId = null;
    surfaceTrimActive = true;
    surfaceTrimTargetMessageId = externalShapeTargetMessageId;
    const fresh = createSurfaceTrimInteraction(
      trim?.nodeId ?? null,
      trim?.pathMode ?? 'feature',
      trim?.capMode ?? 'open',
    );
    surfaceTrimState = fresh;
    surfaceTrimCommittedSegments = [];
    surfaceTrimLoopSegments = [];
    surfaceTrimRetainedTriangleIndices = [];
    surfaceTrimPreview = null;
    surfaceTrimSelectedAnchorIndex = null;
    surfaceTrimMoveAnchorIndex = null;
    surfaceTrimApplying = false;
    surfaceTrimLatestRequestedPreviewId = ++surfaceTrimPreviewId;
    surfaceTrimOperationToken += 1;
    if (!trim) return;

    const restoredAnchors = trim.loopAnchors.map(anchor => restoreSurfaceTrimAnchor(trim, anchor));
    const restoredSeed = restoreSurfaceTrimAnchor(trim, trim.keepSeed);
    if (restoredAnchors.some(anchor => !anchor) || !restoredSeed) {
      surfaceTrimState = setSurfaceTrimError(
        fresh,
        'Applied surface trim anchors cannot be reconstructed from the current source mesh.',
      );
      return;
    }
    const anchors = restoredAnchors as CaptureSurfaceAnchor[];
    const restoredState: SurfaceTrimInteraction = {
      ...fresh,
      anchors,
      keepSeed: restoredSeed,
      phase: 'boundaryClosed',
    };
    const previewId = ++surfaceTrimPreviewId;
    surfaceTrimLatestRequestedPreviewId = previewId;
    try {
      const response = await onPreviewExternalSurfaceTrimLoop(
        anchors,
        restoredState.pathMode as SurfaceTrimPathMode,
        previewId,
        surfaceTrimTargetMessageId,
      );
      if (response.previewId !== surfaceTrimLatestRequestedPreviewId || !surfaceTrimActive) return;
      surfaceTrimCommittedSegments = response.loopSegments;
      surfaceTrimLoopSegments = response.loopSegments;
      const selectingState = beginSurfaceTrimRegionSelection(restoredState);
      surfaceTrimState = selectingState;
      await requestSurfaceTrimRegionPreview(selectingState);
    } catch (error) {
      surfaceTrimState = setSurfaceTrimError(restoredState, formatBackendError(error));
    }
  }

  function requireSurfaceTrimTargetSnapshot() {
    if (externalShapeTargetMessageId !== surfaceTrimTargetMessageId) {
      throw new Error('Target snapshot changed while editing surface trim. Cancel and restart the trim.');
    }
  }

  function surfaceTrimSegmentFromPath(
    response: SurfaceTrimPathPreviewResponse,
    segmentIndex: number,
  ): SurfaceTrimLoopSegmentPreview {
    return {
      segmentIndex,
      fromTriangleIndex: response.path.startTriangleIndex,
      toTriangleIndex: response.path.endTriangleIndex,
      trianglePath: response.path.triangleCorridor,
      edgeSegments: response.path.edgeSegments,
      continuousPolyline: response.path.continuousPolyline,
    };
  }

  async function refreshSurfaceTrimOpenPaths(anchors = surfaceTrimState.anchors) {
    const operationToken = ++surfaceTrimOperationToken;
    if (anchors.length < 2) {
      surfaceTrimCommittedSegments = [];
      surfaceTrimLoopSegments = [];
      return;
    }
    try {
      requireSurfaceTrimTargetSnapshot();
      const segments = await Promise.all(
        anchors.slice(1).map(async (anchor, index) => {
          const previewId = ++surfaceTrimPreviewId;
          const response = await onPreviewExternalSurfaceTrimPath(
            anchors[index],
            anchor,
            surfaceTrimState.pathMode as SurfaceTrimPathMode,
            previewId,
            surfaceTrimTargetMessageId,
          );
          if (response.previewId !== previewId) {
            throw new Error(`Surface trim path preview ${response.previewId} does not match request ${previewId}.`);
          }
          return surfaceTrimSegmentFromPath(response, index);
        }),
      );
      if (operationToken !== surfaceTrimOperationToken || !surfaceTrimActive) return;
      surfaceTrimCommittedSegments = segments;
      surfaceTrimLoopSegments = segments;
      surfaceTrimState = setSurfaceTrimError(surfaceTrimState, '');
    } catch (error) {
      if (operationToken !== surfaceTrimOperationToken) return;
      surfaceTrimState = setSurfaceTrimError(surfaceTrimState, formatBackendError(error));
    }
  }

  async function handleSurfaceTrimHover(anchor: CaptureSurfaceAnchor | null) {
    if (
      !surfaceTrimActive
      || surfaceTrimState.phase !== 'placingBoundary'
      || surfaceTrimState.anchors.length === 0
      || !anchor
    ) {
      surfaceTrimLoopSegments = surfaceTrimCommittedSegments;
      return;
    }
    const previewId = ++surfaceTrimPreviewId;
    surfaceTrimLatestRequestedPreviewId = previewId;
    const from = surfaceTrimState.anchors[surfaceTrimState.anchors.length - 1];
    try {
      requireSurfaceTrimTargetSnapshot();
      const response = await onPreviewExternalSurfaceTrimPath(
        from,
        anchor,
        surfaceTrimState.pathMode as SurfaceTrimPathMode,
        previewId,
        surfaceTrimTargetMessageId,
      );
      if (response.previewId !== surfaceTrimLatestRequestedPreviewId || !surfaceTrimActive) return;
      surfaceTrimLoopSegments = [
        ...surfaceTrimCommittedSegments,
        surfaceTrimSegmentFromPath(response, surfaceTrimCommittedSegments.length),
      ];
    } catch (error) {
      if (previewId !== surfaceTrimLatestRequestedPreviewId) return;
      surfaceTrimState = setSurfaceTrimError(surfaceTrimState, formatBackendError(error));
    }
  }

  async function closeCurrentSurfaceTrimLoop() {
    if (!canCloseSurfaceTrimBoundary(surfaceTrimState) || surfaceTrimApplying) return;
    const closed = closeSurfaceTrimBoundary(surfaceTrimState);
    if (closed.error) {
      surfaceTrimState = closed;
      return;
    }
    surfaceTrimState = closed;
    const previewId = ++surfaceTrimPreviewId;
    surfaceTrimLatestRequestedPreviewId = previewId;
    try {
      requireSurfaceTrimTargetSnapshot();
      const response = await onPreviewExternalSurfaceTrimLoop(
        closed.anchors,
        closed.pathMode as SurfaceTrimPathMode,
        previewId,
        surfaceTrimTargetMessageId,
      );
      if (response.previewId !== surfaceTrimLatestRequestedPreviewId || !surfaceTrimActive) return;
      surfaceTrimCommittedSegments = response.loopSegments;
      surfaceTrimLoopSegments = response.loopSegments;
      surfaceTrimState = beginSurfaceTrimRegionSelection(closed);
      surfaceTrimSelectedAnchorIndex = null;
      surfaceTrimMoveAnchorIndex = null;
    } catch (error) {
      surfaceTrimState = setSurfaceTrimError({ ...closed, phase: 'placingBoundary' }, formatBackendError(error));
    }
  }

  async function requestSurfaceTrimRegionPreview(state: SurfaceTrimInteraction) {
    if (!state.keepSeed) return;
    const previewId = ++surfaceTrimPreviewId;
    surfaceTrimLatestRequestedPreviewId = previewId;
    try {
      requireSurfaceTrimTargetSnapshot();
      const response = await onPreviewExternalSurfaceTrimRegion(
        state.anchors,
        state.keepSeed,
        state.pathMode as SurfaceTrimPathMode,
        state.capMode as SurfaceTrimCapMode,
        previewId,
        surfaceTrimTargetMessageId,
      );
      if (response.previewId !== surfaceTrimLatestRequestedPreviewId || !surfaceTrimActive) return;
      surfaceTrimPreview = response;
      surfaceTrimRetainedTriangleIndices = response.preview.retainedTriangleIndices;
      surfaceTrimLoopSegments = response.preview.loopSegments;
      surfaceTrimState = markSurfaceTrimPreviewReady({ ...state, phase: 'selectingRegion' });
    } catch (error) {
      surfaceTrimPreview = null;
      surfaceTrimRetainedTriangleIndices = [];
      surfaceTrimState = setSurfaceTrimError(
        { ...state, phase: 'selectingRegion' },
        formatBackendError(error),
      );
    }
  }

  async function selectSurfaceTrimCapMode(capMode: SurfaceTrimCapMode) {
    surfaceTrimState = { ...surfaceTrimState, capMode, error: '' };
    if (surfaceTrimState.keepSeed) {
      await requestSurfaceTrimRegionPreview({ ...surfaceTrimState, phase: 'selectingRegion' });
    }
  }

  async function applyCurrentSurfaceTrim() {
    const keepSeed = surfaceTrimState.keepSeed;
    if (!canApplySurfaceTrim(surfaceTrimState) || !keepSeed || surfaceTrimApplying) return;
    const readyState = surfaceTrimState;
    surfaceTrimApplying = true;
    surfaceTrimState = markSurfaceTrimApplying(surfaceTrimState);
    try {
      requireSurfaceTrimTargetSnapshot();
      await onApplyExternalSurfaceTrim(
        readyState.anchors,
        keepSeed,
        readyState.pathMode as SurfaceTrimPathMode,
        readyState.capMode as SurfaceTrimCapMode,
        readyState.editingTrimNodeId,
        surfaceTrimTargetMessageId,
      );
      cancelSurfaceTrimInteraction();
    } catch (error) {
      surfaceTrimState = setSurfaceTrimError(
        { ...readyState, phase: 'previewReady' },
        formatBackendError(error),
      );
      surfaceTrimApplying = false;
    }
  }

  async function removeSurfaceTrim(trimNodeId: number) {
    if (surfaceTrimApplying || surfaceTrimActive) return;
    surfaceTrimApplying = true;
    try {
      await onRemoveExternalSurfaceTrim(trimNodeId);
    } catch (error) {
      surfaceTrimState = setSurfaceTrimError(surfaceTrimState, formatBackendError(error));
    } finally {
      surfaceTrimApplying = false;
    }
  }

  function handleSurfaceAnchor(anchor: CaptureSurfaceAnchor) {
    if (workflowStep === 'crop' && surfaceTrimActive) {
      if (surfaceTrimState.phase === 'placingBoundary') {
        if (surfaceTrimMoveAnchorIndex !== null) {
          surfaceTrimState = moveSurfaceTrimAnchor(
            surfaceTrimState,
            surfaceTrimMoveAnchorIndex,
            anchor,
          );
          if (!surfaceTrimState.error) {
            surfaceTrimSelectedAnchorIndex = surfaceTrimMoveAnchorIndex;
            surfaceTrimMoveAnchorIndex = null;
            void refreshSurfaceTrimOpenPaths();
          }
        } else {
          const previousCount = surfaceTrimState.anchors.length;
          surfaceTrimState = addSurfaceTrimAnchor(surfaceTrimState, anchor);
          if (!surfaceTrimState.error && surfaceTrimState.anchors.length === previousCount + 1) {
            void refreshSurfaceTrimOpenPaths();
          }
        }
      } else if (surfaceTrimState.phase === 'selectingRegion') {
        surfaceTrimState = chooseSurfaceTrimKeepSeed(surfaceTrimState, anchor);
        void requestSurfaceTrimRegionPreview(surfaceTrimState);
      }
      return;
    }
    if (workflowStep === 'crop' && planePickerActive) {
      if (planeAnchors.length < 3) planeAnchors = [...planeAnchors, anchor];
      return;
    }
    onGuideAnchor(anchor);
  }

  function handleSurfaceAnchorError(message: string) {
    if (workflowStep === 'crop' && surfaceTrimActive) {
      surfaceTrimState = setSurfaceTrimError(surfaceTrimState, message);
    } else if (workflowStep === 'crop' && planePickerActive) {
      planeError = message;
    } else {
      onGuideAnchorError(message);
    }
  }

  async function applyPlaneCrop() {
    if (planeAnchors.length !== 3 || planeApplying) return;
    planeApplying = true;
    planeError = '';
    try {
      await onApplyExternalPlaneCrop(planeAnchors, planeKeepPositive, editingCropNodeId);
      planePickerActive = false;
      planeAnchors = [];
      editingCropNodeId = null;
    } catch (error) {
      planeError = formatBackendError(error);
    } finally {
      planeApplying = false;
    }
  }

  async function removePlaneCrop(cropNodeId: number) {
    if (planeApplying) return;
    planeApplying = true;
    planeError = '';
    try {
      await onRemoveExternalPlaneCrop(cropNodeId);
    } catch (error) {
      planeError = formatBackendError(error);
    } finally {
      planeApplying = false;
    }
  }

  function handlePreviewLoaded() {
    previewStatus = 'loaded';
    previewError = '';
  }

  function handlePreviewLoadError(message: string) {
    previewStatus = 'failed';
    previewError = message;
    onPreviewLoadError(message);
  }

  const bootstrapBaseUrl = $derived(trustUrl.replace(/\/trust$/, ''));
  const pairingToken = $derived(pairingUrl.split('/').filter(Boolean).pop() ?? '');
  const pairingQrUrl = $derived(
    bootstrapBaseUrl && pairingToken ? `${bootstrapBaseUrl}/capture/${pairingToken}/qr.svg` : '',
  );
  const trustQrUrl = $derived(bootstrapBaseUrl ? `${bootstrapBaseUrl}/trust/qr.svg` : '');
  const scaledBoundsMm = $derived(
    meshPreview?.boundsMm.map((value) => Number((value * previewScale).toFixed(1))) ?? [],
  );
  const guideLocalBounds = $derived.by(() => {
    if (!guide?.landmarks.length) return null;
    const min: [number, number, number] = [Infinity, Infinity, Infinity];
    const max: [number, number, number] = [-Infinity, -Infinity, -Infinity];
    for (const landmark of guide.landmarks) {
      landmark.localPositionMm.forEach((value, axis) => {
        min[axis] = Math.min(min[axis], value);
        max[axis] = Math.max(max[axis], value);
      });
    }
    return { min, max };
  });

  function formatVector(value: [number, number, number]): string {
    return value.map(item => Number(item.toFixed(3))).join(', ');
  }

  function handlePreviewScaleInput(event: Event) {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    if (Number.isFinite(value) && value > 0) onPreviewScaleChange(value);
  }

  function handleKnownDistanceInput(event: Event) {
    onGuideKnownDistanceChange(Number((event.currentTarget as HTMLInputElement).value));
  }

  function handleFeatureDepthInput(event: Event) {
    onGuideFeatureDepthChange(Number((event.currentTarget as HTMLInputElement).value));
  }

  function handleInstructionInput(event: Event) {
    onGuideInstructionChange((event.currentTarget as HTMLTextAreaElement).value);
  }

  function selectWorkflowStep(step: ExternalShapeStep) {
    if (step !== 'crop') {
      cancelSurfaceTrimInteraction();
      planePickerActive = false;
      planeAnchors = [];
      planeError = '';
      editingCropNodeId = null;
    }
    workflowStep = step;
  }

</script>

<div class="capture-panel external-workspace" data-testid="capture-panel" data-session-state={sessionState}>
  <div class="capture-panel__navigation">
    <div class="capture-panel__workflow" role="tablist" aria-label="External shapes workflow">
      {#each workflowSteps as step (step.id)}
        <button
          type="button"
          role="tab"
          aria-selected={workflowStep === step.id}
          class:active={workflowStep === step.id}
          title={`Open ${step.label.toLowerCase()} step.`}
          onclick={() => selectWorkflowStep(step.id)}
        >{step.label}</button>
      {/each}
    </div>
  </div>
  <PuppetBuilder active={visible && workflowStep === 'puppet'} {captureRunId} {currentModelPath} />
  {#if workflowStep !== 'puppet'}
  <div class="capture-panel__body" class:scan-setup={scanSetup}>
  <section class="capture-panel__viewport" aria-label="Capture view">
    {#if activePreviewUrl}
      <div
        class="capture-panel__mesh-viewport"
        data-testid={workflowStep === 'capture' ? 'capture-preview-viewport' : 'external-shape-viewport'}
        data-preview-status={previewStatus}
      >
        <Viewer
          modelKey={activePreviewModelKey}
          stlUrl={activePreviewUrl}
          showContextOverlay={false}
          cropBoxEnabled={captureScanActive && cropEnabled && !guideMode && !planePickerActive && !surfaceTrimActive}
          cropBoxMode={cropMode}
          {cropBounds}
          {onCropBoundsChange}
          captureLandmarkMode={(workflowStep === 'crop' && (planePickerActive || surfaceTrimActive)) || (captureGuidesActive && guideMode && guideState?.status !== 'stale')}
          captureSourceMeshContentDigest={workflowStep === 'crop' ? (selectedExternalShape?.contentDigest ?? null) : (guide?.sourceMesh.contentDigest ?? null)}
          capturePlaneAnchors={workflowStep === 'crop' && !surfaceTrimActive ? planeAnchors : []}
          {surfaceTrimActive}
          surfaceTrimAnchors={surfaceTrimState.anchors}
          surfaceTrimKeepSeed={surfaceTrimState.keepSeed}
          {surfaceTrimLoopSegments}
          {surfaceTrimRetainedTriangleIndices}
          surfaceTrimCapPreview={surfaceTrimPreview?.capPreview ?? null}
          {surfaceTrimSelectedAnchorIndex}
          captureGuide={surfaceTrimActive ? null : guide}
          captureSelectedLandmarkId={guideSelectedLandmarkId}
          captureComparisonStlUrl={guideComparisonUrl}
          captureDeviation={guideDeviation}
          captureDeviationVisible={guideDeviationVisible}
          captureReferenceVisible={guideReferenceVisible}
          captureReferenceOpacity={guideReferenceOpacity}
          captureGeneratedVisible={guideGeneratedVisible}
          captureGeneratedOpacity={guideGeneratedOpacity}
          onCaptureSurfaceAnchor={handleSurfaceAnchor}
          onCaptureSurfaceHover={handleSurfaceTrimHover}
          onCaptureSurfaceAnchorError={handleSurfaceAnchorError}
          onSurfaceTrimPointSelect={(index) => surfaceTrimSelectedAnchorIndex = index}
          onCaptureSelectLandmark={onGuideSelectLandmark}
          onModelLoaded={handlePreviewLoaded}
          onModelLoadError={handlePreviewLoadError}
        />
        <div class="capture-panel__mesh-status">
          <strong>{workflowStep === 'capture' ? guidance : activeSourceName}</strong>
          <span>{previewStatus === 'loading' ? 'LOADING MESH' : (workflowStep === 'capture' ? cameraStatus : (externalShapePreviewIsCropped ? 'CROPPED RESULT' : 'RAW STL'))}</span>
        </div>
        {#if previewError}<div class="capture-panel__mesh-error" role="alert">{previewError}</div>{/if}
      </div>
    {:else if captureScanActive}
      <div class="capture-panel__viewfinder">
        <div class="capture-panel__setup-content">
          <span class="capture-panel__eyebrow">{idleCapture ? 'Phone photogrammetry' : 'Phone capture'}</span>
          <h2>{captureHeading}</h2>
          {#if idleCapture}
            <p class="capture-panel__intro">Turn photos of a real object into a 3D model. Start here, then connect your phone on the same Wi-Fi.</p>
            <div class="capture-panel__setup-actions">
              <AsyncActionButton className="shape-primary" label="Start capture" pendingLabel="Starting…" action={onStartCapture} onerror={(error) => actionError = formatBackendError(error)} />
              <AsyncActionButton label="Open last capture" pendingLabel="Opening…" action={onOpenLastCapture} onerror={(error) => actionError = formatBackendError(error)} />
            </div>
            {#if sessionState === 'cancelled'}<p class="capture-panel__camera-status">Session cancelled</p>{/if}
            <ol class="capture-panel__preparation">
              <li><strong>Prepare the object</strong><span>Use soft light and a plain background. Keep the object still.</span></li>
              <li><strong>Photograph every side</strong><span>Move around it slowly, with overlapping views from several heights.</span></li>
              <li><strong>Inspect the result</strong><span>Check the mesh, adjust its size, then use it in CAD or a puppet.</span></li>
            </ol>
          {:else if sessionState === 'failed'}
            <p class="capture-panel__source-error" role="alert">{cameraStatus}</p>
            <div class="capture-panel__setup-actions">
              {#if sessionActive}
                <AsyncActionButton className="shape-primary" label="Retry reconstruction" pendingLabel="Retrying…" action={onRetryReconstruction} onerror={(error) => actionError = formatBackendError(error)} />
                <button type="button" onclick={onCancelCapture}>Cancel capture</button>
              {:else}
                <AsyncActionButton className="shape-primary" label="Start capture" pendingLabel="Starting…" action={onStartCapture} onerror={(error) => actionError = formatBackendError(error)} />
                <AsyncActionButton label="Open last capture" pendingLabel="Opening…" action={onOpenLastCapture} />
              {/if}
            </div>
          {:else if sessionState === 'reconstructing'}
            <p class="capture-panel__intro">Processing {frameCount} accepted photos. The model will appear here when it is ready.</p>
            <progress aria-label="Scan reconstruction" max="1" value={reconstructionProgress}></progress>
            <p class="capture-panel__camera-status">{cameraStatus}</p>
            <button type="button" onclick={onCancelCapture}>Cancel capture</button>
          {:else if !sessionActive}
            <p role="status" class="capture-panel__intro">Preparing the phone connection. Your QR code will appear here.</p>
          {:else}
            <p class="capture-panel__intro">Open the camera link on your phone and allow camera access. Keep both devices on the same Wi-Fi.</p>
            <div class="capture-panel__connection">
              {#if pairingQrUrl}<img class="capture-panel__qr" src={pairingQrUrl} alt="Capture pairing QR code" />{/if}
              <div class="capture-panel__connection-info">
                <strong>Scan with your phone camera</strong>
                <a class="capture-panel__pairing-url" href={pairingUrl} target="_blank" rel="noreferrer">{pairingUrl}</a>
                <p class="capture-panel__camera-status" role="status">{cameraStatus}</p>
                <span>{frameCount} photos accepted</span>
              </div>
            </div>
            {#if trustUrl}
              <details class="capture-panel__trust-disclosure">
                <summary>First connection on iPhone</summary>
                <div class="capture-panel__trust-steps">
                  <p>If Safari cannot open the camera, install and trust Ecky's local certificate, then open the camera link again.</p>
                  {#if trustQrUrl}<img class="capture-panel__qr" src={trustQrUrl} alt="Phone certificate QR code" />{/if}
                  <a class="capture-panel__trust-url" href={trustUrl} target="_blank" rel="noreferrer">Install phone certificate</a>
                  <strong>Enable full trust on the phone</strong>
                  <code>Settings &gt; General &gt; About &gt; Certificate Trust Settings</code>
                </div>
              </details>
            {/if}
            <div class="capture-panel__setup-actions"><button type="button" onclick={onCancelCapture}>Cancel capture</button></div>
          {/if}
          {#if actionError}<p class="capture-panel__source-error" role="alert">{actionError}</p>{/if}
        </div>
      </div>
    {:else}
      <div class="capture-panel__empty-step">
        <h2>{workflowStep === 'import' ? 'Source shapes' : captureGuidesActive ? 'Start with a scan' : 'Select a shape to crop'}</h2>
        <p>{workflowStep === 'import' ? 'Shapes referenced by this project appear here. You can also scan an object with your phone.' : 'Choose a source shape or finish a phone scan before editing its geometry.'}</p>
        <button type="button" onclick={() => { selectWorkflowStep('capture'); captureStep = 'scan'; }}>Go to capture</button>
      </div>
    {/if}
  </section>

  {#if !scanSetup}

  <aside class="capture-panel__side">
    <header class="capture-panel__header">
      <strong>{workflowStep === 'capture' ? 'Scan tools' : workflowStep === 'import' ? 'Project sources' : 'Crop tools'}</strong>
      {#if workflowStep === 'capture' && meshPreview}
        <div class="capture-panel__capture-workflow" role="tablist" aria-label="Capture workflow">
          {#each captureSteps as step (step.id)}
            <button type="button" role="tab" aria-selected={captureStep === step.id} class:active={captureStep === step.id} onclick={() => captureStep = step.id}>{step.label}</button>
          {/each}
        </div>
      {/if}
    </header>

    {#if workflowStep === 'import'}
      <section class="capture-panel__sources" aria-label="Imported source shapes">
        {#if externalShapeError}
          <div class="capture-panel__source-error" role="alert">{externalShapeError}</div>
        {:else if externalShapeSources.length === 0}
          <div class="capture-panel__source-empty">This project has no imported STL shapes yet.</div>
        {:else}
          {#each externalShapeSources as source (source.nodeId)}
            <button
              type="button"
              class:active={source.nodeId === selectedExternalShapeNodeId}
              aria-pressed={source.nodeId === selectedExternalShapeNodeId}
              title={source.exists ? `Use ${source.path} as external source.` : `Missing bound source: ${source.path}`}
              onclick={() => onSelectExternalShape(source.nodeId)}
            >
              <strong>{source.displayName}</strong>
              <span>{source.partKey} · {source.exists ? `${((source.byteLength ?? 0) / 1_000_000).toFixed(1)} MB` : 'MISSING'}{source.planeCrops?.length ? ` · ${source.planeCrops.length} PLANE CUT${source.planeCrops.length === 1 ? '' : 'S'}` : ''}{source.surfaceTrims?.length ? ` · ${source.surfaceTrims.length} SURFACE TRIM${source.surfaceTrims.length === 1 ? '' : 'S'}` : ''}</span>
            </button>
          {/each}
        {/if}
      </section>
    {:else if workflowStep !== 'capture' && activeSourceName}
      <div class="capture-panel__active-source">
        <span>SOURCE</span>
        <strong>{activeSourceName}</strong>
      </div>
    {/if}

    {#if workflowStep === 'crop' && selectedExternalShape}
      <section class="capture-panel__plane-crop" aria-label="External shape crop">
        {#if selectedPlaneCrops.length > 0}
          <div class="capture-panel__existing-crops" role="region" aria-label="Existing plane crops">
            <header>
              <strong>APPLIED CUTS</strong>
              <span>{selectedPlaneCrops.length}</span>
            </header>
            {#each selectedPlaneCrops as crop, index}
              <div class="capture-panel__existing-crop" title={`Origin ${formatVector(crop.origin)}; normal ${formatVector(crop.normal)}`}>
                <div>
                  <strong>PLANE {index + 1}</strong>
                  <span>{crop.keepPositive ? 'KEEP ABOVE' : 'KEEP BELOW'} · N {formatVector(crop.normal)}</span>
                </div>
                <div class="capture-panel__existing-crop-actions">
                  <button
                    type="button"
                    title={`Pick three new points and replace plane ${index + 1}.`}
                    aria-label={`Edit plane ${index + 1}`}
                    disabled={planeApplying || planePickerActive}
                    onclick={() => startPlanePicker(crop.nodeId, crop.keepPositive)}
                  >EDIT</button>
                  <AsyncActionButton
                    className="compact"
                    title={`Remove plane ${index + 1} while preserving other cuts.`}
                    ariaLabel={`Remove plane ${index + 1}`}
                    disabled={planeApplying || planePickerActive}
                    label="REMOVE"
                    pendingLabel="REMOVING…"
                    action={() => removePlaneCrop(crop.nodeId)}
                  />
                </div>
              </div>
            {/each}
          </div>
        {/if}
        {#if selectedSurfaceTrims.length > 0}
          <div class="capture-panel__existing-crops" role="region" aria-label="Existing surface trims">
            <header>
              <strong>SURFACE TRIMS</strong>
              <span>{selectedSurfaceTrims.length}</span>
            </header>
            {#each selectedSurfaceTrims as trim, index}
              <div class="capture-panel__existing-crop">
                <div>
                  <strong>TRIM {index + 1}</strong>
                  <span>{trim.loopAnchors.length} POINTS · {trim.pathMode.toUpperCase()} · {trim.capMode.replace('surfaceFill', 'SURFACE FILL').toUpperCase()}</span>
                </div>
                <div class="capture-panel__existing-crop-actions">
                  <button
                    type="button"
                    title={`Replace surface trim ${index + 1} with a newly traced contour.`}
                    aria-label={`Edit surface trim ${index + 1}`}
                    disabled={surfaceTrimApplying || surfaceTrimActive || planePickerActive}
                    onclick={() => void startSurfaceTrim(trim)}
                  >EDIT</button>
                  <AsyncActionButton
                    className="compact"
                    title={`Remove surface trim ${index + 1} while preserving its source child.`}
                    ariaLabel={`Remove surface trim ${index + 1}`}
                    disabled={surfaceTrimApplying || surfaceTrimActive || planePickerActive}
                    label="REMOVE"
                    pendingLabel="REMOVING…"
                    action={() => removeSurfaceTrim(trim.nodeId)}
                  />
                </div>
              </div>
            {/each}
          </div>
        {/if}
        {#if !planePickerActive && !surfaceTrimActive}
          <div class="capture-panel__crop-methods" role="group" aria-label="External shape crop tools">
            <button type="button" title="Pick three mesh points to define a canonical crop plane." onclick={() => startPlanePicker()}>CUT PLANE</button>
            <button
              type="button"
              title={selectedSurfaceTrims.length > 0
                ? 'Edit or remove the existing canonical surface trim before tracing a replacement.'
                : 'Trace an ordered non-planar contour directly on the source mesh.'}
              disabled={selectedSurfaceTrims.length > 0}
              onclick={() => void startSurfaceTrim()}
            >TRACE SURFACE</button>
          </div>
          {#if surfaceTrimState.error}<div class="capture-panel__source-error" role="alert">{surfaceTrimState.error}</div>{/if}
        {:else if planePickerActive}
          <div class="capture-panel__plane-status">
            <strong>{editingCropNodeId === null ? 'NEW PLANE' : `EDIT PLANE ${selectedPlaneCrops.findIndex(crop => crop.nodeId === editingCropNodeId) + 1}`} · POINTS {planeAnchors.length}/3</strong>
            <span>{planeKeepPositive ? 'KEEP ABOVE PLANE' : 'KEEP BELOW PLANE'}</span>
          </div>
          <div class="capture-panel__plane-actions">
            <button type="button" title="Remove the most recently selected plane point." disabled={planeAnchors.length === 0 || planeApplying} onclick={() => planeAnchors = planeAnchors.slice(0, -1)}>UNDO POINT</button>
            <button type="button" title="Keep the opposite side of the selected plane." disabled={planeAnchors.length !== 3 || planeApplying} onclick={() => planeKeepPositive = !planeKeepPositive}>FLIP SIDE</button>
            <button type="button" title="Write this plane as canonical clip-plane source and render it." disabled={planeAnchors.length !== 3 || planeApplying} onclick={applyPlaneCrop}>{planeApplying ? 'APPLYING' : 'APPLY PLANE'}</button>
            <button type="button" title="Discard these plane points without changing source." disabled={planeApplying} onclick={() => { planePickerActive = false; planeAnchors = []; planeError = ''; editingCropNodeId = null; }}>CANCEL</button>
          </div>
          {#if planeError}<div class="capture-panel__source-error" role="alert">{planeError}</div>{/if}
        {:else if surfaceTrimActive}
          <div class="capture-panel__plane-status" data-testid="surface-trim-status">
            <strong>{surfaceTrimState.editingTrimNodeId === null ? 'TRACE SURFACE' : 'EDIT SURFACE TRIM'} · POINTS {surfaceTrimState.anchors.length}</strong>
            <span>
              {surfaceTrimState.phase === 'placingBoundary'
                ? (surfaceTrimMoveAnchorIndex === null ? 'PLACE BOUNDARY POINTS' : `MOVE POINT ${surfaceTrimMoveAnchorIndex + 1}`)
                : surfaceTrimState.phase === 'boundaryClosed'
                  ? 'BOUNDARY CLOSED'
                : surfaceTrimState.phase === 'selectingRegion'
                  ? 'CLICK REGION TO KEEP'
                  : surfaceTrimState.phase === 'previewReady'
                    ? 'PREVIEW READY'
                    : surfaceTrimState.phase.toUpperCase()}
            </span>
          </div>
          {#if surfaceTrimState.phase === 'placingBoundary'}
            <div class="capture-panel__trim-choice" role="group" aria-label="Surface path mode">
              <button
                type="button"
                class:active={surfaceTrimState.pathMode === 'shortest'}
                aria-pressed={surfaceTrimState.pathMode === 'shortest'}
                title="Use deterministic shortest paths between hard points."
                onclick={() => { surfaceTrimState = { ...surfaceTrimState, pathMode: 'shortest', error: '' }; void refreshSurfaceTrimOpenPaths(); }}
              >SHORTEST</button>
              <button
                type="button"
                class:active={surfaceTrimState.pathMode === 'feature'}
                aria-pressed={surfaceTrimState.pathMode === 'feature'}
                title="Prefer nearby creases when path lengths are similar."
                onclick={() => { surfaceTrimState = { ...surfaceTrimState, pathMode: 'feature', error: '' }; void refreshSurfaceTrimOpenPaths(); }}
              >FEATURE</button>
            </div>
            <div class="capture-panel__plane-actions">
              <button type="button" title="Remove the last boundary point." disabled={surfaceTrimState.anchors.length === 0 || surfaceTrimApplying} onclick={() => { surfaceTrimState = undoSurfaceTrimAnchor(surfaceTrimState); surfaceTrimSelectedAnchorIndex = null; void refreshSurfaceTrimOpenPaths(); }}>UNDO POINT</button>
              <button type="button" title="Move the selected boundary point with the next mesh click." disabled={surfaceTrimSelectedAnchorIndex === null || surfaceTrimApplying} onclick={() => surfaceTrimMoveAnchorIndex = surfaceTrimSelectedAnchorIndex}>MOVE SELECTED</button>
              <button type="button" title="Remove the selected boundary point." disabled={surfaceTrimSelectedAnchorIndex === null || surfaceTrimApplying} onclick={() => { if (surfaceTrimSelectedAnchorIndex !== null) { surfaceTrimState = removeSurfaceTrimAnchor(surfaceTrimState, surfaceTrimSelectedAnchorIndex); surfaceTrimSelectedAnchorIndex = null; surfaceTrimMoveAnchorIndex = null; void refreshSurfaceTrimOpenPaths(); } }}>REMOVE SELECTED</button>
              <button type="button" title="Compute and validate the closing path from the last point to the first." disabled={!canCloseSurfaceTrimBoundary(surfaceTrimState) || surfaceTrimApplying} onclick={() => void closeCurrentSurfaceTrimLoop()}>CLOSE LOOP</button>
            </div>
          {:else}
            <div class="capture-panel__trim-choice" role="group" aria-label="Surface trim cap mode">
              <button type="button" class:active={surfaceTrimState.capMode === 'open'} aria-pressed={surfaceTrimState.capMode === 'open'} title="Leave the traced boundary open. Open output cannot enter solidify until another canonical operation closes it." onclick={() => void selectSurfaceTrimCapMode('open')}>OPEN</button>
              <button type="button" class:active={surfaceTrimState.capMode === 'flat'} aria-pressed={surfaceTrimState.capMode === 'flat'} title="Fit and validate one least-squares plane, then triangulate a planar cap without fallback." onclick={() => void selectSurfaceTrimCapMode('flat')}>FLAT</button>
              <button type="button" class:active={surfaceTrimState.capMode === 'surfaceFill'} aria-pressed={surfaceTrimState.capMode === 'surfaceFill'} title="Build a constrained non-planar patch and reject foldovers without falling back to Flat." onclick={() => void selectSurfaceTrimCapMode('surfaceFill')}>SURFACE FILL</button>
            </div>
            {#if surfaceTrimPreview}
              <div class="capture-panel__trim-report">
                <span>KEEP {surfaceTrimPreview.preview.retainedTriangleCount.toLocaleString()} TRIANGLES</span>
                <span>DROP {surfaceTrimPreview.preview.excludedTriangleCount.toLocaleString()}</span>
                {#if surfaceTrimCapReport?.maxPlanarityDeviation !== null && surfaceTrimCapReport?.maxPlanarityDeviation !== undefined}
                  <span>FLAT MAX {surfaceTrimCapReport.maxPlanarityDeviation.toFixed(3)} mm · RMS {(surfaceTrimCapReport.rmsPlanarityDeviation ?? 0).toFixed(3)} mm</span>
                {/if}
              </div>
            {/if}
            <div class="capture-panel__plane-actions">
              <button type="button" title="Return to the numbered hard points and edit the traced boundary." disabled={surfaceTrimApplying} onclick={() => { surfaceTrimState = { ...surfaceTrimState, phase: 'placingBoundary', error: '' }; surfaceTrimPreview = null; surfaceTrimRetainedTriangleIndices = []; void refreshSurfaceTrimOpenPaths(); }}>EDIT BOUNDARY</button>
              <button type="button" title="Choose another retained region on the source mesh." disabled={surfaceTrimApplying} onclick={() => { surfaceTrimState = { ...surfaceTrimState, phase: 'selectingRegion', keepSeed: null, error: '' }; surfaceTrimPreview = null; surfaceTrimRetainedTriangleIndices = []; }}>CHANGE REGION</button>
              <button type="button" title="Write the validated contour and cap policy as canonical surface-trim source, render, then save only on success." disabled={!canApplySurfaceTrim(surfaceTrimState) || surfaceTrimApplying} onclick={() => void applyCurrentSurfaceTrim()}>{surfaceTrimApplying ? 'APPLYING' : 'APPLY SURFACE TRIM'}</button>
              <button type="button" title="Discard surface trim interaction without changing source." disabled={surfaceTrimApplying} onclick={cancelSurfaceTrimInteraction}>CANCEL</button>
            </div>
          {/if}
          {#if surfaceTrimState.error}<div class="capture-panel__source-error" role="alert">{surfaceTrimState.error}</div>{/if}
        {/if}
      </section>
    {/if}

    {#if captureScanActive && sessionActive}<details class="capture-panel__diagnostics"><summary>Capture details</summary><div class="capture-panel__stats">
      {#each stats as stat (stat.label)}
        <div class="capture-panel__stat">
          <span>{stat.label}</span>
          <strong>{stat.value}</strong>
        </div>
      {/each}
    </div></details>{/if}

    {#if meshPreview}
      <div class="capture-panel__preview" data-testid="capture-mesh-preview">
        <strong>{meshPreview.triangleCount.toLocaleString()} triangles</strong>
        <span>{scaledBoundsMm.join(' x ')} mm</span>
        {#if captureScanActive && !guideMode && !planePickerActive && !surfaceTrimActive}
        <label class="capture-panel__scale">
          <span>Capture scale</span>
          <input
            type="number"
            aria-label="Capture scale"
            min="0.001"
            max="2"
            step="0.001"
            value={previewScale}
            disabled={previewApplied}
            oninput={handlePreviewScaleInput}
          />
        </label>
        <div class="capture-panel__crop-tools">
          <button
            type="button"
            title="Show or hide the crop box used to select source scan geometry."
            class:active={cropEnabled}
            aria-pressed={cropEnabled}
            disabled={previewApplied || previewStatus === 'loading'}
            onclick={() => onCropEnabledChange(!cropEnabled)}
          >BOX CROP</button>
          {#if cropEnabled}
            <div class="capture-panel__crop-modes" role="group" aria-label="Crop box transform">
              <button
                type="button"
                title="Move the crop box without resizing it."
                class:active={cropMode === 'translate'}
                aria-pressed={cropMode === 'translate'}
                onclick={() => onCropModeChange('translate')}
              >MOVE BOX</button>
              <button
                type="button"
                title="Resize the crop box around observed scan geometry."
                class:active={cropMode === 'scale'}
                aria-pressed={cropMode === 'scale'}
                onclick={() => onCropModeChange('scale')}
              >RESIZE BOX</button>
            </div>
            <div class="capture-panel__crop-actions">
              <button
                type="button"
                title="Build a new immutable preview mesh from the current crop box."
                disabled={!cropBounds || previewStatus === 'loading'}
                onclick={onPreviewCrop}
              >PREVIEW CROP</button>
              <button type="button" title="Restore the full uncropped source mesh." disabled={previewStatus === 'loading'} onclick={onResetCrop}>RESET CROP</button>
            </div>
          {/if}
        </div>
        <span>{meshPreview.scaleLabel} x {previewScale}</span>
        {/if}
        {#each meshPreview.warnings as warning}
          <span class="capture-panel__warning">{warning}</span>
        {/each}
      </div>
    {/if}

    {#if captureGuidesActive && meshPreview && !guideMode}
      <button type="button" class="capture-panel__guided-entry" title="Lock the selected source mesh and start placing reconstruction evidence." onclick={onStartGuidedCad}>START GUIDES</button>
    {/if}

    {#if captureGuidesActive && guideMode && guide}
      <section class="capture-panel__guide" aria-label="Guided CAD evidence">
        <header>
          <strong>GUIDED BREP</strong>
          <span>{guideState?.status?.toUpperCase() ?? 'DRAFT'} · REV {guide.revision}</span>
        </header>
        <div class="capture-panel__guide-role-grid" role="group" aria-label="Landmark role">
          <button type="button" aria-label="PICK CALIBRATION ENDPOINT" title="Pick two scan points whose physical distance is known." class:active={guidePickRole === 'calibrationEndpoint'} onclick={() => onGuidePickRoleChange('calibrationEndpoint')}>CALIBRATION</button>
          <button type="button" aria-label="PICK FRAME ORIGIN" title="Pick the local coordinate-system origin on the scan." class:active={guidePickRole === 'frameOrigin'} onclick={() => onGuidePickRoleChange('frameOrigin')}>FRAME ORIGIN</button>
          <button type="button" aria-label="PICK FRAME DIRECTION" title="Pick X then XY direction evidence for a right-handed local frame." class:active={guidePickRole === 'frameDirection'} onclick={() => onGuidePickRoleChange('frameDirection')}>FRAME DIRECTION</button>
          <button type="button" aria-label="PICK SYMMETRY PLANE" title="Pick at least three samples on a symmetry plane." class:active={guidePickRole === 'symmetrySample'} onclick={() => onGuidePickRoleChange('symmetrySample')}>SYMMETRY PLANE</button>
          <button type="button" aria-label="PICK PROFILE VERTEX" title="Pick profile vertices in intended edge order." class:active={guidePickRole === 'profileVertex'} onclick={() => onGuidePickRoleChange('profileVertex')}>PROFILE VERTEX</button>
        </div>
        <details class="capture-panel__guide-disclosure">
          <summary>ADVANCED EVIDENCE</summary>
          <div class="capture-panel__guide-role-grid" role="group" aria-label="Advanced landmark roles">
            <button type="button" aria-label="PICK ROTATION AXIS" title="Pick two or more samples defining a rotational or cylindrical axis." class:active={guidePickRole === 'rotationAxisEndpoint'} onclick={() => onGuidePickRoleChange('rotationAxisEndpoint')}>ROTATION AXIS</button>
            <button type="button" aria-label="PICK MATING SURFACE" title="Sample a physical mating surface that generated CAD must preserve." class:active={guidePickRole === 'matingSurfaceSample'} onclick={() => onGuidePickRoleChange('matingSurfaceSample')}>MATING SURFACE</button>
            <button type="button" aria-label="PICK BORE" title="Sample visible cylindrical bore evidence without assigning BRep identity to scan triangles." class:active={guidePickRole === 'boreSample'} onclick={() => onGuidePickRoleChange('boreSample')}>BORE</button>
            <button type="button" aria-label="PICK OUTER EXTENT" title="Mark an observed outer extent used for envelope validation." class:active={guidePickRole === 'outerExtent'} onclick={() => onGuidePickRoleChange('outerExtent')}>OUTER EXTENT</button>
            <button type="button" aria-label="PICK CLEARANCE" title="Mark a fit-critical clearance boundary; name its constraint before build." class:active={guidePickRole === 'clearanceBoundary'} onclick={() => onGuidePickRoleChange('clearanceBoundary')}>CLEARANCE</button>
            <button type="button" aria-label="PICK DAMAGE" title="Mark damaged scan evidence that reconstruction must ignore." class:active={guidePickRole === 'ignoredDamagedRegion'} onclick={() => onGuidePickRoleChange('ignoredDamagedRegion')}>DAMAGE REGION</button>
            <button type="button" aria-label="PICK NAMED REFERENCE" title="Add a generic labeled scan reference for later correspondence." class:active={guidePickRole === 'namedReference'} onclick={() => onGuidePickRoleChange('namedReference')}>NAMED REFERENCE</button>
          </div>
        </details>
        <details class="capture-panel__guide-disclosure">
          <summary>LANDMARKS · {guide.landmarks.length}</summary>
          <div class="capture-panel__guide-points" aria-label="Guide landmarks">
            {#each guide.landmarks as landmark, index (landmark.landmarkId)}
              <div
                class="capture-panel__guide-point"
                class:selected={guideSelectedLandmarkId === landmark.landmarkId}
                data-role={landmark.role}
                data-landmark-id={landmark.landmarkId}
              >
                <button
                  type="button"
                  class="capture-panel__guide-point-focus"
                  aria-label={`Select landmark ${index + 1}`}
                  title={`Select landmark ${index + 1} in the scan view.`}
                  onclick={() => onGuideSelectLandmark(landmark.landmarkId)}
                ><strong>{index + 1}</strong></button>
                <input
                  aria-label={`Landmark ${index + 1} label`}
                  value={landmark.label}
                  onchange={(event) => onGuideEditLandmark(landmark.landmarkId, {
                    label: event.currentTarget.value,
                    role: landmark.role,
                  })}
                />
                <select
                  aria-label={`Landmark ${index + 1} role`}
                  value={landmark.role}
                  onchange={(event) => onGuideEditLandmark(landmark.landmarkId, {
                    label: landmark.label,
                    role: event.currentTarget.value as CaptureLandmarkRole,
                  })}
                >
                  {#each landmarkRoles as role}<option value={role}>{role}</option>{/each}
                </select>
                <code>T{landmark.anchor.triangleIndex}</code>
                <button
                  type="button"
                  aria-label={`Delete landmark ${index + 1}`}
                  title={`Delete landmark ${index + 1}.`}
                  onclick={() => onGuideDeleteLandmark(landmark.landmarkId)}
                >×</button>
              </div>
            {/each}
          </div>
          <button type="button" title="Undo the latest landmark, profile, or target edit." class="capture-panel__guide-undo" disabled={!guideCanUndo} onclick={onGuideUndo}>UNDO GUIDE EDIT</button>
        </details>
        <details class="capture-panel__guide-disclosure">
          <summary>PROFILE · {guide.profiles[0]?.landmarkIds.length ?? 0} POINTS</summary>
          {#each guide.profiles as profile (profile.profileId)}
            <fieldset class="capture-panel__guide-editor">
            <legend>ORDERED PROFILE</legend>
            <label>
              <span>Kind</span>
              <select
                aria-label="Profile kind"
                value={profile.kind}
                onchange={(event) => onGuideEditProfile(profile.profileId, {
                  kind: event.currentTarget.value as CaptureProfileKind,
                })}
              >
                {#each profileKinds as kind}<option value={kind}>{kind}</option>{/each}
              </select>
            </label>
            <label>
              <span>Operation</span>
              <select
                aria-label="Profile operation"
                value={profile.operationHint}
                onchange={(event) => onGuideEditProfile(profile.profileId, {
                  operationHint: event.currentTarget.value as CaptureProfileOperationHint,
                })}
              >
                {#each profileOperations as operation}<option value={operation}>{operation}</option>{/each}
              </select>
            </label>
            <div class="capture-panel__profile-order" aria-label="Profile point order">
              {#each profile.landmarkIds as landmarkId, profileIndex (landmarkId)}
                {@const landmarkIndex = guide.landmarks.findIndex(item => item.landmarkId === landmarkId)}
                <div>
                  <strong>{profileIndex + 1}</strong>
                  <span>{guide.landmarks[landmarkIndex]?.label ?? landmarkId}</span>
                  <button
                    type="button"
                    aria-label={`Move profile point ${profileIndex + 1} up`}
                    title={`Move profile point ${profileIndex + 1} earlier in edge order.`}
                    disabled={profileIndex === 0}
                    onclick={() => onGuideMoveProfileLandmark(profile.profileId, landmarkId, profileIndex - 1)}
                  >↑</button>
                  <button
                    type="button"
                    aria-label={`Move profile point ${profileIndex + 1} down`}
                    title={`Move profile point ${profileIndex + 1} later in edge order.`}
                    disabled={profileIndex === profile.landmarkIds.length - 1}
                    onclick={() => onGuideMoveProfileLandmark(profile.profileId, landmarkId, profileIndex + 1)}
                  >↓</button>
                </div>
              {/each}
            </div>
            </fieldset>
          {/each}
        </details>
        <details class="capture-panel__guide-disclosure">
          <summary>EXACT BREP TARGETS · {guide.featureExpectations.length}</summary>
          {#each guide.featureExpectations as expectation (expectation.expectationId)}
            <fieldset class="capture-panel__guide-editor">
            <legend>{expectation.label}</legend>
            <label>
              <span>Geometry</span>
              <select
                aria-label={`${expectation.expectationId} geometry kind`}
                value={expectation.expectedGeometryKind}
                onchange={(event) => onGuideEditExpectation(expectation.expectationId, {
                  expectedGeometryKind: event.currentTarget.value as CaptureExpectedGeometryKind,
                })}
              >
                {#each expectedGeometryKinds as kind}<option value={kind}>{kind}</option>{/each}
              </select>
            </label>
            <label>
              <span>Topology</span>
              <select
                aria-label={`${expectation.expectationId} topology kind`}
                value={expectation.requiredBrepTopologyKind}
                onchange={(event) => onGuideEditExpectation(expectation.expectationId, {
                  requiredBrepTopologyKind: event.currentTarget.value as CaptureRequiredBrepTopologyKind,
                })}
              >
                {#each topologyKinds as kind}<option value={kind}>{kind}</option>{/each}
              </select>
            </label>
            <label>
              <span>Cardinality</span>
              <select
                aria-label={`${expectation.expectationId} cardinality`}
                value={expectation.cardinality}
                onchange={(event) => onGuideEditExpectation(expectation.expectationId, {
                  cardinality: event.currentTarget.value as CaptureSelectorCardinality,
                })}
              >
                {#each cardinalities as cardinality}<option value={cardinality}>{cardinality}</option>{/each}
              </select>
            </label>
            <label>
              <span>Part ID</span>
              <input
                aria-label={`${expectation.expectationId} part id`}
                value={expectation.partId}
                onchange={(event) => onGuideEditExpectation(expectation.expectationId, {
                  partId: event.currentTarget.value,
                })}
              />
            </label>
            <label>
              <span>Selector</span>
              <select
                aria-label={`${expectation.expectationId} selector kind`}
                value={expectation.expectedAuthoredSelector.kind}
                onchange={(event) => onGuideEditExpectation(expectation.expectationId, {
                  expectedAuthoredSelector: {
                    kind: event.currentTarget.value as 'binding' | 'tag',
                    name: expectation.expectedAuthoredSelector.name,
                  },
                })}
              >
                <option value="binding">binding</option>
                <option value="tag">tag</option>
              </select>
              <input
                aria-label={`${expectation.expectationId} selector name`}
                value={expectation.expectedAuthoredSelector.name}
                onchange={(event) => onGuideEditExpectation(expectation.expectationId, {
                  expectedAuthoredSelector: {
                    kind: expectation.expectedAuthoredSelector.kind,
                    name: event.currentTarget.value,
                  },
                })}
              />
            </label>
            </fieldset>
          {/each}
        </details>
        <label class="capture-panel__guide-field">
          <span>Known distance, mm</span>
          <input type="number" min="0.001" step="0.1" value={guideKnownDistanceMm} oninput={handleKnownDistanceInput} />
        </label>
        <label class="capture-panel__guide-field">
          <span>Named feature depth, mm</span>
          <input aria-label="Named feature depth, mm" type="number" min="0.001" step="0.1" value={guideFeatureDepthMm} oninput={handleFeatureDepthInput} />
        </label>
        {#if guideLocalBounds}
          <details class="capture-panel__guide-disclosure">
            <summary>GUIDE METRICS</summary>
            <dl class="capture-panel__guide-derived" aria-label="Derived guide frame and bounds">
            <div><dt>Scale</dt><dd>{guide.calibration.millimetresPerSourceUnit} mm/source</dd></div>
            <div><dt>Local min</dt><dd>{formatVector(guideLocalBounds.min)}</dd></div>
            <div><dt>Local max</dt><dd>{formatVector(guideLocalBounds.max)}</dd></div>
            <div><dt>Origin</dt><dd>{formatVector(guide.reconstructionFrame.originMm)}</dd></div>
            <div><dt>X / Y / Z</dt><dd>{formatVector(guide.reconstructionFrame.xAxis)} · {formatVector(guide.reconstructionFrame.yAxis)} · {formatVector(guide.reconstructionFrame.zAxis)}</dd></div>
            {#each guide.axes as axis (axis.axisId)}
              <div><dt>{axis.label}</dt><dd>RMS {axis.fit.rmsMm} · MAX {axis.fit.maxMm} mm</dd></div>
            {/each}
            {#each guide.planes as plane (plane.planeId)}
              <div><dt>{plane.label}</dt><dd>RMS {plane.fit.rmsMm} · MAX {plane.fit.maxMm} mm</dd></div>
            {/each}
            </dl>
          </details>
        {/if}
        {#if guideComparisonUrl}
          <section class="capture-panel__comparison" aria-label="Scan and generated BRep comparison">
            <header>
              <strong>OBSERVED SCAN ↔ GENERATED BREP</strong>
              <code>{guideComparisonModelKey ?? 'generated-model'}</code>
            </header>
            <div class="capture-panel__comparison-controls">
              <label>
                <input
                  type="checkbox"
                  aria-label="Show reference scan"
                  checked={guideReferenceVisible}
                  onchange={(event) => onGuideReferenceVisibleChange(event.currentTarget.checked)}
                />
                <span>Reference scan</span>
              </label>
              <input
                type="range"
                aria-label="Reference scan opacity"
                min="0.02"
                max="1"
                step="0.02"
                value={guideReferenceOpacity}
                oninput={(event) => onGuideReferenceOpacityChange(Number(event.currentTarget.value))}
              />
              <label>
                <input
                  type="checkbox"
                  aria-label="Show generated BRep"
                  checked={guideGeneratedVisible}
                  onchange={(event) => onGuideGeneratedVisibleChange(event.currentTarget.checked)}
                />
                <span>Generated BRep</span>
              </label>
              <input
                type="range"
                aria-label="Generated BRep opacity"
                min="0.02"
                max="1"
                step="0.02"
                value={guideGeneratedOpacity}
                oninput={(event) => onGuideGeneratedOpacityChange(Number(event.currentTarget.value))}
              />
              <label>
                <input
                  type="checkbox"
                  aria-label="Show deviation colors"
                  checked={guideDeviationVisible}
                  onchange={(event) => onGuideDeviationVisibleChange(event.currentTarget.checked)}
                />
                <span>Deviation colors</span>
              </label>
            </div>
            {#if guideDeviation}
              <dl class="capture-panel__comparison-metrics">
                <div><dt>Scope</dt><dd>OBSERVED REGION ONLY</dd></div>
                <div><dt>Samples</dt><dd>{guideDeviation.sampleCount} / {guideDeviation.sourceVertexCount}</dd></div>
                <div><dt>Outliers</dt><dd>{guideDeviation.outlierCount} @ {guideDeviation.outlierThresholdMm} mm</dd></div>
                <div><dt>Max / RMS / P95</dt><dd>{guideDeviation.maximumMm} / {guideDeviation.rmsMm} / {guideDeviation.percentile95Mm} mm</dd></div>
              </dl>
            {/if}
            {#if guideResult?.correspondences.some(item => item.residual)}
              <dl class="capture-panel__comparison-metrics" aria-label="Exact guide-to-BRep residuals">
                {#each guideResult.correspondences as correspondence (correspondence.expectationId)}
                  {#if correspondence.residual}
                    <div>
                      <dt>{correspondence.expectationId}</dt>
                      <dd>{correspondence.residual.metric} · MAX {correspondence.residual.maximum} · RMS {correspondence.residual.rms} {correspondence.residual.unit}</dd>
                    </div>
                  {/if}
                {/each}
              </dl>
            {/if}
            {#if guideResult?.inferredRegions.length}
              <div class="capture-panel__comparison-inferred">
                <strong>INFERRED / UNVERIFIED</strong>
                {#each guideResult.inferredRegions as region}<span>{region}</span>{/each}
              </div>
            {/if}
          </section>
        {/if}
        {#if guideComparisonError}<div class="capture-panel__guide-error" role="alert">{guideComparisonError}</div>{/if}
        {#if (guide.featurePlanCandidates?.length ?? 0) > 0 || (guide.primitiveHypotheses?.some(item => item.status === 'rejected') ?? false)}
          <section class="capture-panel__deterministic-plan" aria-label="Deterministic reconstruction plan">
            <header>
              <strong>DETERMINISTIC PLAN</strong>
              <span>{guide.reconstructionReadiness?.ready ? 'READY' : 'RESOLUTION REQUIRED'}</span>
            </header>
            {#each guide.reconstructionReadiness?.stages ?? [] as stage (stage.stage)}
              <div class="capture-panel__stage">
                <strong>{stage.stage.replace(/([A-Z])/g, ' $1').toUpperCase()} · {stage.status.toUpperCase()}</strong>
                <span>{stage.detail}</span>
                {#if stage.affectedEvidenceIds.length}<code>{stage.affectedEvidenceIds.join(' · ')}</code>{/if}
              </div>
            {/each}
            {#each guide.featurePlanCandidates ?? [] as plan (plan.planId)}
              <label class="capture-panel__plan-choice">
                <input
                  type="radio"
                  name="capture-feature-plan"
                  aria-label={`Select ${plan.label}`}
                  value={plan.planId}
                  checked={guide.selectedFeaturePlanId === plan.planId}
                  disabled={plan.status === 'rejected'}
                  onchange={() => onGuideSelectFeaturePlan(plan.planId)}
                />
                <span><strong>{plan.label}</strong><small>{plan.status.toUpperCase()} · SCORE {plan.score.toFixed(2)}</small></span>
              </label>
            {/each}
            {#each guide.primitiveHypotheses?.filter(item => item.status === 'rejected') ?? [] as hypothesis (hypothesis.hypothesisId)}
              <div class="capture-panel__rejected-hypothesis">
                <strong>{hypothesis.kind.toUpperCase()} · REJECTED</strong>
                <span>{hypothesis.reason}</span>
                <code>{hypothesis.guideItemIds.join(' · ')}</code>
              </div>
            {/each}
          </section>
        {/if}
        <label class="capture-panel__guide-field">
          <span>Reconstruction instruction</span>
          <textarea
            rows="2"
            aria-label="Reconstruction instruction"
            placeholder="Describe only intended geometry, constraints, symmetry, and uncertain regions."
            value={guideInstruction}
            oninput={handleInstructionInput}
          ></textarea>
        </label>
        {#if !guideReady}
          <details class="capture-panel__guide-disclosure capture-panel__guide-readiness">
            <summary>{guideReadinessReasons.length} EVIDENCE REQUIREMENTS MISSING</summary>
            <div>{#each guideReadinessReasons as reason}<span>{reason}</span>{/each}</div>
          </details>
        {/if}
        {#if guideError}<div class="capture-panel__guide-error" role="alert">{guideError}</div>{/if}
        <div class="capture-panel__guide-actions">
          <button type="button" title="Validate calibration, frame, profiles, and exact BRep target contracts." disabled={!guideReady || guideState?.status === 'stale'} onclick={onValidateGuide}>VALIDATE GUIDE</button>
          <button type="button" title="Queue reconstruction in this capture's owning task after successful validation." disabled={guideState?.status !== 'ready'} onclick={onBuildCadFromGuide}>BUILD CAD FROM GUIDE</button>
        </div>
      </section>
    {/if}

    {#if captureScanActive}<div class="capture-panel__actions">
      {#if !sessionActive && (sessionState === 'pairing' || sessionState === 'cancelled')}
        <button
          type="button"
          class="capture-panel__primary"
          title="Start a phone capture session for the current task."
          onclick={onStartCapture}
        >START CAPTURE</button>
        <button
          type="button"
          class="capture-panel__secondary"
          title="Reopen the latest saved capture owned by the current task."
          onclick={onOpenLastCapture}
        >OPEN LAST CAPTURE</button>
      {/if}
      {#if sessionActive}<button
        type="button"
        class="capture-panel__secondary"
        title="Cancel the active capture session."
        onclick={onCancelCapture}
      >CANCEL</button>{/if}
      {#if meshPreview}
        <button type="button" class="capture-panel__secondary" title="Return to phone capture and add more source photos." onclick={onAddPhotos}>ADD PHOTOS</button>
        <button type="button" class="capture-panel__primary" title="Apply the prepared scan preview to the capture draft." onclick={onApplyPreview} disabled={applyPending}>{applyPending ? 'APPLYING' : 'APPLY'}</button>
        <button type="button" class="capture-panel__primary" title="Commit the applied capture preview to model history." onclick={onCommitPreview} disabled={!previewApplied}>COMMIT</button>
      {/if}
      {#if sessionState === 'failed'}
        <button type="button" class="capture-panel__primary" title="Retry mesh reconstruction using the accepted source frames." onclick={onRetryReconstruction}>RETRY RECONSTRUCTION</button>
      {/if}
    </div>{/if}
  </aside>
  {/if}
  </div>
  {/if}
</div>

<style>
  .capture-panel { display: flex; flex-direction: column; gap: 0; width: 100%; height: 100%; min-height: 0; min-width: 0; overflow: hidden; background: var(--bg-100); }
  .capture-panel__navigation { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px; padding: 10px 14px; flex: none; border-bottom: 1px solid var(--bg-300); }
  .capture-panel__workflow, .capture-panel__capture-workflow { display: flex; gap: 4px; min-width: 0; }
  .capture-panel__workflow button, .capture-panel__capture-workflow button { padding: 5px 12px; border: 0; border-bottom: 2px solid transparent; color: var(--text-dim); background: transparent; font-size: 11px; letter-spacing: .04em; }
  .capture-panel__workflow button.active, .capture-panel__capture-workflow button.active { border-bottom-color: var(--secondary); color: var(--text); background: color-mix(in srgb, var(--secondary) 8%, transparent); }
  .capture-panel__body { display: grid; grid-template-columns: minmax(0, 1fr) 300px; flex: 1; min-height: 0; min-width: 0; overflow: hidden; }
  .capture-panel__body.scan-setup { grid-template-columns: minmax(0, 1fr); }
  .capture-panel__viewport { min-height: 0; min-width: 0; overflow: auto; }
  .capture-panel__empty-step { display: flex; flex-direction: column; justify-content: center; align-items: flex-start; gap: 12px; min-height: 100%; max-width: 620px; padding: 32px; margin: auto; color: var(--text-dim); }
  .capture-panel__empty-step p { margin: 0; }
  .capture-panel__empty-step button { padding: 6px 12px; background: var(--bg-200); color: var(--text); border: 1px solid var(--bg-300); }
  .capture-panel__viewfinder { min-height: 100%; display: grid; align-content: center; padding: clamp(20px, 5cqi, 56px); }
  .capture-panel__setup-content { width: 100%; max-width: 660px; margin: auto; display: flex; flex-direction: column; align-items: flex-start; gap: 16px; }
  .capture-panel__eyebrow { color: var(--secondary); font-size: 11px; letter-spacing: .08em; text-transform: uppercase; }
  .capture-panel__intro { margin: 0; max-width: 550px; color: var(--text-dim); font-size: 14px; }
  .capture-panel__setup-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; }
  .capture-panel__setup-content :global(button) { padding: 8px 14px; border: 1px solid var(--bg-300); color: var(--text); background: var(--bg-200); font-size: 13px; letter-spacing: 0; }
  .capture-panel__setup-content :global(button.shape-primary) { color: var(--bg-100); background: var(--secondary); border-color: var(--secondary); }
  .capture-panel__preparation { list-style: none; counter-reset: scan-step; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 22px; padding: 24px 0; margin: 8px 0 0; border-top: 1px solid var(--bg-300); }
  .capture-panel__preparation li { display: flex; flex-direction: column; gap: 6px; counter-increment: scan-step; }
  .capture-panel__preparation li::before { content: '0' counter(scan-step); font-family: var(--font-mono); font-size: 11px; color: var(--secondary); }
  .capture-panel__preparation strong { font-size: 13px; font-weight: 600; }
  .capture-panel__preparation span { font-size: 12px; color: var(--text-dim); }
  .capture-panel__connection { display: flex; gap: 24px; align-items: center; width: 100%; }
  .capture-panel__connection-info { display: flex; flex-direction: column; gap: 10px; min-width: 0; font-size: 12px; }
  .capture-panel__connection-info > span { color: var(--text-dim); }
  .capture-panel__setup-content progress { width: 100%; accent-color: var(--secondary); }
  .capture-panel__mesh-viewport { position: relative; width: 100%; height: 100%; min-height: 260px; overflow: hidden; background: #090c12; }

  .capture-panel__mesh-status,
  .capture-panel__mesh-error {
    position: absolute;
    left: 10px;
    right: 10px;
    z-index: 5;
    padding: 8px 10px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 90%, transparent);
    font-family: var(--font-mono);
  }

  .capture-panel__mesh-status {
    top: 10px;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    color: var(--text-dim);
  }

  .capture-panel__mesh-status strong { color: var(--primary); }
  .capture-panel__mesh-error { bottom: 10px; color: var(--danger); white-space: normal; }

  .capture-panel__camera-status { margin: 0; color: var(--text-dim); overflow-wrap: anywhere; }
  .capture-panel__pairing-url { color: var(--secondary); overflow-wrap: anywhere; font-family: var(--font-mono); font-size: 12px; }
  .capture-panel__trust-url { color: var(--secondary); }
  .capture-panel__trust-disclosure { width: 100%; border-top: 1px solid var(--bg-300); }
  .capture-panel__trust-steps { display: grid; gap: 10px; padding: 0 0 16px; color: var(--text-dim); font-size: 12px; }
  .capture-panel__trust-steps p { margin: 0; }
  .capture-panel__trust-steps code { white-space: normal; overflow-wrap: anywhere; }
  .capture-panel__qr { width: 148px; height: 148px; flex: none; image-rendering: pixelated; border: 6px solid white; background: white; }

  .capture-panel__side {
    min-height: 0;
    min-width: 0;
    border-left: 1px solid var(--bg-300);
    background: var(--bg-200);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
  }

  .capture-panel__header {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-family: var(--font-mono);
  }

  .capture-panel__header,
  .capture-panel__stats,
  .capture-panel__preview,
  .capture-panel__guide,
  .capture-panel__guided-entry,
  .capture-panel__actions {
    flex: 0 0 auto;
  }

  .capture-panel__header strong {
    color: var(--primary);
    font-size: 0.9rem;
  }

  .capture-panel__stats {
    display: grid;
    gap: 8px;
    overflow: hidden;
  }

  .capture-panel__sources {
    display: grid;
    gap: 6px;
    overflow: hidden auto;
  }

  .capture-panel__sources button,
  .capture-panel__active-source,
  .capture-panel__source-empty,
  .capture-panel__source-error {
    min-width: 0;
    padding: 8px 10px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    border-radius: 0;
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--text-dim);
    font-family: var(--font-mono);
    text-align: left;
  }

  .capture-panel__sources button {
    display: grid;
    gap: 3px;
  }

  .capture-panel__sources button strong,
  .capture-panel__active-source strong {
    overflow: hidden;
    color: var(--secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .capture-panel__sources button span,
  .capture-panel__active-source span {
    overflow: hidden;
    font-size: 0.68rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .capture-panel__sources button.active {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 14%, var(--bg-100));
  }

  .capture-panel__active-source {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .capture-panel__source-error { color: var(--danger); white-space: normal; }

  .capture-panel__plane-crop,
  .capture-panel__plane-actions {
    display: grid;
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__plane-actions { grid-template-columns: 1fr 1fr; }

  .capture-panel__crop-methods,
  .capture-panel__trim-choice {
    display: grid;
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__crop-methods { grid-template-columns: 1fr 1fr; }
  .capture-panel__trim-choice { grid-template-columns: repeat(3, minmax(0, 1fr)); }

  .capture-panel__trim-choice button.active {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-100));
    color: var(--secondary);
  }

  .capture-panel__trim-report {
    display: grid;
    gap: 4px;
    padding: 8px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--secondary) 50%, var(--bg-300));
    color: var(--text-dim);
    font: 700 0.64rem/1.25 var(--font-mono);
  }

  .capture-panel__trim-report span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .capture-panel__existing-crops {
    display: grid;
    gap: 4px;
    max-height: 168px;
    overflow: auto;
  }

  .capture-panel__existing-crops header,
  .capture-panel__existing-crops > div {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    padding: 7px 8px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--primary) 36%, var(--bg-300));
    color: var(--text-dim);
    font: 700 0.66rem/1.15 var(--font-mono);
  }

  .capture-panel__existing-crops header strong,
  .capture-panel__existing-crops > div strong {
    color: var(--secondary);
  }

  .capture-panel__existing-crops > div span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .capture-panel__existing-crop > div:first-child {
    display: grid;
    min-width: 0;
    gap: 3px;
    overflow: hidden;
  }

  .capture-panel__existing-crop-actions {
    display: flex;
    flex: 0 0 auto;
    gap: 4px;
    overflow: hidden;
  }

  .capture-panel__existing-crop-actions button {
    min-height: 26px;
    padding: 4px 6px;
    font-size: 0.58rem;
  }

  .capture-panel__plane-crop button {
    min-width: 0;
    min-height: 32px;
    padding: 6px 8px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    border-radius: 0;
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    color: var(--primary);
    font: 700 0.7rem/1 var(--font-mono);
    text-overflow: ellipsis;
  }

  .capture-panel__plane-crop button:disabled { opacity: 0.4; }

  .capture-panel__plane-status {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 8px;
    overflow: hidden;
    border: 1px solid var(--primary);
    color: var(--text-dim);
    font: 700 0.68rem/1 var(--font-mono);
  }

  .capture-panel__plane-status strong { color: var(--secondary); }

  .capture-panel__preview {
    display: grid;
    gap: 6px;
    padding: 10px;
    overflow: hidden;
    border: 1px solid var(--primary);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  .capture-panel__preview strong { color: var(--secondary); }
  .capture-panel__warning { color: var(--primary); }

  .capture-panel__scale {
    display: grid;
    grid-template-columns: 1fr minmax(72px, 96px);
    align-items: center;
    gap: 8px;
    overflow: hidden;
  }

  .capture-panel__scale input {
    min-width: 0;
    height: 30px;
    padding: 0 7px;
    border: 1px solid var(--bg-300);
    border-radius: 0;
    background: var(--bg-100);
    color: var(--secondary);
    font: inherit;
  }

  .capture-panel__crop-tools,
  .capture-panel__crop-modes,
  .capture-panel__crop-actions {
    display: grid;
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__crop-modes,
  .capture-panel__crop-actions {
    grid-template-columns: 1fr 1fr;
  }

  .capture-panel__crop-tools button {
    min-width: 0;
    min-height: 30px;
    padding: 4px 6px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    border-radius: 0;
    background: color-mix(in srgb, var(--bg-100) 92%, transparent);
    color: var(--text-dim);
    font: inherit;
    font-weight: 700;
  }

  .capture-panel__crop-tools button.active {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 18%, var(--bg-100));
    color: var(--primary);
  }

  .capture-panel__crop-tools button:disabled {
    opacity: 0.4;
  }

  .capture-panel__stat {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 88%, transparent);
    font-family: var(--font-mono);
    text-transform: uppercase;
  }

  .capture-panel__stat span {
    color: var(--text-dim);
    font-size: 0.7rem;
  }

  .capture-panel__stat strong {
    color: var(--secondary);
    font-size: 0.74rem;
  }

  .capture-panel__actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: auto;
    padding-top: 12px;
    position: sticky;
    bottom: -16px;
    background: var(--bg-200);
  }

  .capture-panel__guided-entry,
  .capture-panel__guide button,
  .capture-panel__guide input,
  .capture-panel__guide textarea {
    border: 1px solid var(--bg-300);
    border-radius: 0;
    background: var(--bg-100);
    color: var(--text);
    font: inherit;
  }

  .capture-panel__guided-entry {
    min-height: 36px;
    color: var(--secondary);
    font-family: var(--font-mono);
    font-weight: 700;
  }

  .capture-panel__guide {
    display: grid;
    gap: 8px;
    padding: 10px;
    overflow: hidden;
    border: 1px solid var(--secondary);
    font-family: var(--font-mono);
    font-size: 0.7rem;
  }

  .capture-panel__guide header,
  .capture-panel__guide-point,
  .capture-panel__guide-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__guide header strong,
  .capture-panel__guide-point strong { color: var(--secondary); }
  .capture-panel__guide header span,
  .capture-panel__guide-point code { color: var(--text-dim); }

  .capture-panel__guide-role-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    overflow: hidden;
  }

  .capture-panel__guide-role-grid button,
  .capture-panel__guide-actions button,
  .capture-panel__guide-undo {
    min-height: 30px;
    padding: 4px 6px;
    overflow: hidden;
    color: var(--text-dim);
    font-size: 0.64rem;
    font-weight: 700;
  }

  .capture-panel__guide-role-grid button.active {
    border-color: var(--primary);
    color: var(--primary);
  }

  .capture-panel__guide-disclosure {
    min-width: 0;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    background: color-mix(in srgb, var(--bg-100) 74%, transparent);
  }

  .capture-panel__guide-disclosure > summary {
    min-height: 28px;
    padding: 7px 8px;
    overflow: hidden;
    color: var(--secondary);
    cursor: pointer;
    font-weight: 700;
    letter-spacing: 0.06em;
    list-style: none;
  }

  .capture-panel__guide-disclosure > summary::-webkit-details-marker { display: none; }
  .capture-panel__guide-disclosure > summary::before { content: '+ '; color: var(--primary); }
  .capture-panel__guide-disclosure[open] > summary {
    border-bottom: 1px solid var(--bg-300);
  }
  .capture-panel__guide-disclosure[open] > summary::before { content: '− '; }
  .capture-panel__guide-disclosure > :not(summary) { margin: 7px; }
  .capture-panel__guide-disclosure > fieldset { width: calc(100% - 14px); }
  .capture-panel__guide-disclosure > fieldset + fieldset { margin-top: 0; }

  .capture-panel__guide-points {
    display: grid;
    gap: 3px;
    max-height: 116px;
    overflow: auto;
  }

  .capture-panel__guide-point {
    display: grid;
    grid-template-columns: 28px minmax(80px, 1fr) minmax(110px, 1fr) auto 28px;
    padding: 4px 6px;
    border: 1px solid var(--bg-300);
  }

  .capture-panel__guide-point.selected {
    border-color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, var(--bg-100));
  }

  .capture-panel__guide-point input,
  .capture-panel__guide-point select,
  .capture-panel__guide-point button,
  .capture-panel__guide-editor input,
  .capture-panel__guide-editor select,
  .capture-panel__guide-editor button {
    min-width: 0;
    height: 26px;
    padding: 2px 4px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
    border-radius: 0;
    background: var(--bg-100);
    color: var(--text);
    font: inherit;
  }

  .capture-panel__guide-point-focus strong { color: var(--secondary); }

  .capture-panel__guide-editor {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 6px;
    min-width: 0;
    margin: 0;
    padding: 8px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
  }

  .capture-panel__guide-editor legend {
    padding: 0 4px;
    color: var(--secondary);
  }

  .capture-panel__guide-editor label {
    display: grid;
    gap: 3px;
    min-width: 0;
    overflow: hidden;
    color: var(--text-dim);
  }

  .capture-panel__profile-order {
    grid-column: 1 / -1;
    display: grid;
    gap: 3px;
    max-height: 112px;
    overflow: auto;
  }

  .capture-panel__profile-order > div {
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) 28px 28px;
    gap: 4px;
    align-items: center;
    overflow: hidden;
  }

  .capture-panel__guide-field {
    display: grid;
    gap: 4px;
    overflow: hidden;
    color: var(--text-dim);
  }

  .capture-panel__guide-derived {
    display: grid;
    gap: 3px;
    margin: 0;
    padding: 7px;
    overflow: hidden;
    border: 1px solid var(--bg-300);
  }

  .capture-panel__deterministic-plan {
    display: grid;
    gap: 6px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--primary) 45%, transparent);
    padding: 7px;
  }

  .capture-panel__deterministic-plan > header,
  .capture-panel__stage,
  .capture-panel__plan-choice,
  .capture-panel__rejected-hypothesis {
    display: grid;
    gap: 3px;
    overflow: hidden;
  }

  .capture-panel__deterministic-plan > header,
  .capture-panel__plan-choice {
    grid-template-columns: auto 1fr;
    align-items: center;
  }

  .capture-panel__deterministic-plan > header { justify-content: space-between; }
  .capture-panel__stage span,
  .capture-panel__rejected-hypothesis span,
  .capture-panel__plan-choice small { color: var(--text-dim); }
  .capture-panel__stage code,
  .capture-panel__rejected-hypothesis code { overflow: hidden; text-overflow: ellipsis; }

  .capture-panel__guide-derived > div {
    display: grid;
    grid-template-columns: minmax(64px, 0.35fr) minmax(0, 1fr);
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__guide-derived dt { color: var(--text-dim); }
  .capture-panel__guide-derived dd {
    min-width: 0;
    margin: 0;
    overflow-wrap: anywhere;
    color: var(--secondary);
  }

  .capture-panel__comparison {
    display: grid;
    gap: 7px;
    padding: 8px;
    overflow: hidden;
    border: 1px solid var(--primary);
  }

  .capture-panel__comparison header {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__comparison header code {
    max-width: 44%;
    overflow: hidden;
    color: var(--text-dim);
    text-overflow: ellipsis;
  }

  .capture-panel__comparison-controls {
    display: grid;
    grid-template-columns: minmax(120px, 0.7fr) minmax(80px, 1fr);
    gap: 6px;
    align-items: center;
    overflow: hidden;
  }

  .capture-panel__comparison-controls label {
    display: flex;
    gap: 5px;
    align-items: center;
    overflow: hidden;
    color: var(--text-dim);
  }

  .capture-panel__comparison-controls input[type="range"] { min-width: 0; }

  .capture-panel__comparison-metrics {
    display: grid;
    gap: 3px;
    margin: 0;
    overflow: hidden;
  }

  .capture-panel__comparison-metrics > div {
    display: grid;
    grid-template-columns: minmax(70px, 0.4fr) minmax(0, 1fr);
    gap: 6px;
    overflow: hidden;
  }

  .capture-panel__comparison-metrics dt { color: var(--text-dim); }
  .capture-panel__comparison-metrics dd { margin: 0; color: var(--primary); }

  .capture-panel__comparison-inferred {
    display: grid;
    gap: 3px;
    overflow: hidden;
    color: var(--secondary);
  }

  .capture-panel__guide-field input,
  .capture-panel__guide-field textarea {
    min-width: 0;
    padding: 6px;
    resize: vertical;
  }

  .capture-panel__guide-readiness > div,
  .capture-panel__guide-error {
    display: grid;
    gap: 2px;
    overflow: hidden;
    color: var(--text-dim);
  }

  .capture-panel__guide-error { color: var(--danger); }
  .capture-panel__guide-actions button { flex: 1; color: var(--primary); }
  .capture-panel__guide button:disabled { opacity: 0.4; }

  .capture-panel__primary,
  .capture-panel__secondary {
    flex: 1 1 110px;
    min-width: 0;
    min-height: 36px;
    padding: 6px 10px;
    border: 1px solid var(--bg-300);
    border-radius: 0;
    font-family: var(--font-mono);
    font-size: 0.76rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    overflow: hidden;
  }

  .capture-panel__primary:disabled { opacity: 0.4; }

  .capture-panel__primary {
    background: color-mix(in srgb, var(--primary) 18%, var(--bg-100));
    color: var(--primary);
  }

  .capture-panel__secondary {
    background: color-mix(in srgb, var(--bg-100) 92%, transparent);
    color: var(--text);
  }

  @container (max-width: 720px) {
    .capture-panel__body { grid-template-columns: minmax(0, 1fr); overflow-y: auto; display: flex; flex-direction: column; }
    .capture-panel__viewport { flex: 1 0 280px; overflow: visible; }
    .capture-panel__side { flex: none; border-left: 0; border-top: 1px solid var(--bg-300); overflow: visible; }
    .scan-setup .capture-panel__viewport { flex-basis: auto; }
    .capture-panel__preparation { grid-template-columns: 1fr; gap: 16px; }
  }
  @container (max-width: 440px) {
    .capture-panel__connection { flex-direction: column; align-items: flex-start; }
    .capture-panel__navigation { padding: 8px; }
    .capture-panel__workflow button { padding: 5px 8px; }
  }
</style>
