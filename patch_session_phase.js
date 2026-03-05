import fs from 'fs';

const path = 'src/lib/stores/sessionFlow.ts';
let content = fs.readFileSync(path, 'utf8');

content = content.replace(/const hasActiveLLM = phases.some\(p => \['classifying', 'generating', 'rendering', 'queued_for_render', 'committing'\]\.includes\(p\)\);/,
`const hasActiveLLM = phases.some(p => ['classifying', 'answering', 'generating', 'repairing', 'rendering', 'queued_for_render', 'committing'].includes(p));`);

content = content.replace(/if \(phases\.some\(p => p === 'rendering' \|\| p === 'queued_for_render' \|\| p === 'committing'\)\) \{\n    newPhase = 'rendering';\n  \} else if \(phases\.some\(p => p === 'generating'\)\) \{\n    newPhase = 'generating';/,
`if (phases.some(p => p === 'rendering' || p === 'queued_for_render' || p === 'committing')) {
    newPhase = 'rendering';
  } else if (phases.some(p => p === 'repairing')) {
    newPhase = 'repairing';
  } else if (phases.some(p => p === 'generating')) {
    newPhase = 'generating';
  } else if (phases.some(p => p === 'answering')) {
    newPhase = 'answering';`);

content = content.replace(/if \(isQuestion\) \{\n      session\.setStatus\('Answering question\.\.\.'\);/,
`if (isQuestion) {
      session.setStatus('Answering question...');
      requestQueue.patch(requestId, { phase: 'answering' });
      syncSessionPhaseFromQueue();`);

content = content.replace(/requestQueue\.patch\(requestId, \{ phase: 'generating', attempt \}\);/,
`requestQueue.patch(requestId, { phase: attempt > 1 ? 'repairing' : 'generating', attempt });`);

fs.writeFileSync(path, content, 'utf8');
console.log('Patched phases successfully.');
