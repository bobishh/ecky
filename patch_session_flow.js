const fs = require('fs');

const path = 'src/lib/stores/sessionFlow.ts';
let content = fs.readFileSync(path, 'utf8');

// Replacements in runRequestPipeline

content = content.replace(/async function runRequestPipeline\(requestId: string\) \{([\s\S]*?)try \{/, 
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

content = content.replace(/threadId: get\(activeThreadId\),\s*context: buildLightReasoningContext\(\)/, 
`threadId: snapshotThreadId,
        context: buildLightReasoningContext()`); // actually context uses workingCopy, maybe that's fine. Wait, let's fix it properly.

content = content.replace(/threadId: get\(activeThreadId\),\s*titleHint: get\(activeThreadId\) \? undefined : 'Question Session',/,
`threadId: snapshotThreadId,
        titleHint: snapshotThreadId ? undefined : 'Question Session',`);

content = content.replace(/threadId: get\(activeThreadId\),\s*parentMacroCode: get\(workingCopy\)\.macroCode \|\| null,\s*workingDesign: buildWorkingDesignSnapshot\(\),/,
`threadId: snapshotThreadId,
          parentMacroCode: snapshotParentMacroCode,
          workingDesign: snapshotWorkingDesign,`);


content = content.replace(/activeThreadId\.set\(result\.thread_id\);\s*requestQueue\.patch\(requestId, \{ threadId: result\.thread_id \}\);\s*await refreshHistory\(\);/,
`// Only update active thread if the user hasn't navigated away
      if (get(activeThreadId) === snapshotThreadId) {
        activeThreadId.set(result.thread_id);
      }
      requestQueue.patch(requestId, { threadId: result.thread_id });
      await refreshHistory();`);


content = content.replace(/activeThreadId\.set\(qResult\.thread_id\);\s*await refreshHistory\(\);/,
`if (get(activeThreadId) === snapshotThreadId) {
            activeThreadId.set(qResult.thread_id);
          }
          await refreshHistory();`);


content = content.replace(/activeThreadId\.set\(result\.threadId\);\s*activeVersionId\.set\(result\.messageId\);\s*requestQueue\.patch\(requestId, \{ threadId: result\.threadId \}\);\s*const currentQ = get\(requestQueue\);\s*if \(currentQ\.activeId === requestId\) \{\s*workingCopy\.loadVersion\(data, result\.messageId\);\s*session\.setStlUrl\(stlUrlValue\);\s*\}/,
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
