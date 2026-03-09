const fs = require('fs');
const file = '/Users/bogdan/Workspace/personal/alcoholics_audacious/drydemacher/src/lib/ParamPanel.svelte';
let code = fs.readFileSync(file, 'utf8');

// The file is currently mangled around line 107. Let's fix it by regex.
const mangledRegex = /const match = macroCode\.match\(\/params\\s\*=\\s\*\(\\\{\[\\s\\S\]\*\?\\\}\|[\s\S]*?const match = matches\.length > 0 \? matches\[matches\.length - 1\] : null;/;

code = code.replace(mangledRegex, `const matches = [...macroCode.matchAll(/^[^#\\n]*params\\s*=\\s*(\\{[\\s\\S]*?\\}|dict\\([\\s\\S]*?\\))/gm)];
        const match = matches.length > 0 ? matches[matches.length - 1] : null;`);

fs.writeFileSync(file, code);
