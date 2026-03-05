import fs from 'fs';

const path = 'src/lib/stores/sessionFlow.ts';
let content = fs.readFileSync(path, 'utf8');

// Replacements in runRequestPipeline

content = content.replace(/async function runRequestPipeline\(requestId: string\) \{\n  const q = get\(requestQueue\);\n  const req = q\.byId\[requestId\];\n  if \(\!req\) return;\n\n  const \{\n    isQuestionIntent,\n    viewerComponent\n  \} = appState;\n\n  try \{/, 
`async function runRequestPipeline(requestId: string) {
  const q = get(requestQueue);
  const req = q.byId[requestId];
  if (!req) return;

  const {
    isQuestionIntent,
    viewerComponent
  } = appState;

  // STEP 1 FIX: Capture stable snapshot for this request pipeline
  const snapshotThreadId = req.threadId;
  const snapshotParentMacroCode = get(workingCopy).macroCode || null;
  const snapshotWorkingDesign = buildWorkingDesignSnapshot();

  try {`);

content = content.replace(/threadId: get\(activeThreadId\),\n        context: buildLightReasoningContext\(\)/, 
`threadId: snapshotThreadId,
        context: buildLightReasoningContext()`); 

content = content.replace(/threadId: get\(activeThreadId\),\n        titleHint: get\(activeThreadId\) \? undefined : 'Question Session',/,
`threadId: snapshotThreadId,
        titleHint: snapshotThreadId ? undefined : 'Question Session',`);

content = content.replace(/threadId: get\(activeThreadId\),\n          parentMacroCode: get\(workingCopy\)\.macroCode \|\| null,\n          workingDesign: buildWorkingDesignSnapshot\(\),/,
`threadId: snapshotThreadId,
          parentMacroCode: snapshotParentMacroCode,
          workingDesign: snapshotWorkingDesign,`);


content = content.replace(/activeThreadId\.set\(result\.thread_id\);\n      requestQueue\.patch\(requestId, \{ threadId: result\.thread_id \}\);\n      await refreshHistory\(\);/,
`// Only update active thread if the user hasn't navigated away
      if (get(activeThreadId) === snapshotThreadId) {
        activeThreadId.set(result.thread_id);
      }
      requestQueue.patch(requestId, { threadId: result.thread_id });
      await refreshHistory();`);


content = content.replace(/activeThreadId\.set\(qResult\.thread_id\);\n          await refreshHistory\(\);/,
`if (get(activeThreadId) === snapshotThreadId) {
            activeThreadId.set(qResult.thread_id);
          }
          await refreshHistory();`);


content = content.replace(/activeThreadId\.set\(result\.threadId\);\n            activeVersionId\.set\(result\.messageId\);\n            requestQueue\.patch\(requestId, \{ threadId: result\.threadId \}\);\n\n            const currentQ = get\(requestQueue\);\n            if \(currentQ\.activeId === requestId\) \{\n              workingCopy\.loadVersion\(data, result\.messageId\);\n              session\.setStlUrl\(stlUrlValue\);\n            \}/,
`if (get(activeThreadId) === snapshotThreadId) {
              activeThreadId.set(result.threadId);
              activeVersionId.set(result.messageId);
              const currentQ = get(requestQueue);
              if (currentQ.activeId === requestId) {
                workingCopy.loadVersion(data, result.messageId);
                session.setStlUrl(stlUrlValue);
              }
            }
            requestQueue.patch(requestId, { threadId: result.threadId });`);


fs.writeFileSync(path, content, 'utf8');
console.log('Patched runRequestPipeline successfully.');
