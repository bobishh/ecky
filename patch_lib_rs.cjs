const fs = require('fs');
const file = '/Users/bogdan/Workspace/personal/alcoholics_audacious/drydemacher/src-tauri/src/lib.rs';
let code = fs.readFileSync(file, 'utf8');

const target = "- NO BRACES: NEVER use `{var}` style interpolation inside the macro_code string.";
const replacement = target + "\n- CLEANUP: You MUST remove any parameters from \"ui_spec\" and \"initial_params\" that are no longer used in the current \"macro_code\". Do not accumulate parameters from previous designs.";

code = code.replace(target, replacement);

fs.writeFileSync(file, code);
