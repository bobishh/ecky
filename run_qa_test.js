import { execSync } from 'child_process';
try {
  execSync('npx playwright test e2e/qa.spec.js --project=chromium -g "requesting a design should trigger rendering and model update" --debug', { stdio: 'inherit' });
} catch (e) {
  console.log("Test failed");
}
