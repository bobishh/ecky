import { test } from 'node:test';
import assert from 'node:assert/strict';
import { versionDiagnosticForDisplay } from './versionDiagnostic';

test('versionDiagnosticForDisplay keeps actionable errors while redacting credentials and payloads', () => {
  const result = versionDiagnosticForDisplay(
    'Render failed: invalid shape; api_key=secret-value Bearer bearer-value raw_body={"customer":"private"}',
  );

  assert.equal(
    result,
    'Render failed: invalid shape; api_key=[REDACTED] Bearer [REDACTED] raw_body=[REDACTED]',
  );
});

test('versionDiagnosticForDisplay redacts quoted secrets and multiline payloads', () => {
  const result = versionDiagnosticForDisplay(
    'Render failed; password="two words"\npayload={\n"customer":"private"\n}',
  );

  assert.equal(result, 'Render failed; password=[REDACTED]\npayload=[REDACTED]');
});

test('versionDiagnosticForDisplay redacts quoted JSON payload keys', () => {
  assert.equal(
    versionDiagnosticForDisplay('Render failed: {"payload":{"customer":"private-example"}}'),
    'Render failed: [REDACTED]',
  );
});

test('versionDiagnosticForDisplay bounds long diagnostics and handles empty values', () => {
  assert.equal(versionDiagnosticForDisplay('  '), null);
  assert.equal(versionDiagnosticForDisplay('x'.repeat(2500))?.length, 2000);
});
