import { readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

execFileSync('npx', ['tsx', 'scripts/worm_farm_sources.ts'], { stdio: 'inherit' });

const sourceRoot = 'docs/books/ecky-ir/examples/worm-farm';
for (const file of readdirSync(sourceRoot).filter(file => file.endsWith('.ecky')).sort()) {
  execFileSync(process.execPath, [
    'scripts/render_ecky_preview_png.mjs', '--ecky', `${sourceRoot}/${file}`,
    '--png', `docs/books/ecky-ir/assets/worm-farm/${file.replace(/\.ecky$/, '.png')}`,
    '--size', '960',
  ], { stdio: 'inherit' });
}
