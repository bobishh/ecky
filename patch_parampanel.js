const fs = require('fs');
const file = '/Users/bogdan/Workspace/personal/alcoholics_audacious/drydemacher/src/lib/ParamPanel.svelte';
let code = fs.readFileSync(file, 'utf8');

code = code.replace(
  /const match = macroCode\.match\(\/params\\s\*=\\s\*\(\\\{\[\\s\\S\]\*\?\\\}\|dict\\\(\[\\s\\S\]\*\?\\\)\)\/\);/,
  `// Find the LAST dictionary assigned to 'params' to avoid matching commented out old ones
        const matches = [...macroCode.matchAll(/^[^#\\n]*params\\s*=\\s*(\\{[\\s\\S]*?\\}|dict\\([\\s\\S]*?\\))/gm)];
        const match = matches.length > 0 ? matches[matches.length - 1] : null;`
);

fs.writeFileSync(file, code);
