import fs from 'fs';

const path = 'src-tauri/src/commands/generation.rs';
let content = fs.readFileSync(path, 'utf8');

// We need to replace the block starting with `let assistant_msg_id = Uuid::new_v4().to_string();`
// all the way down to before `if let Some(out) = output {`
const searchStart = 'let assistant_msg_id = Uuid::new_v4().to_string();\\n    let thread_id_actual = ctx.thread_id.clone();\\n\\n    \\{\\n        let db = state.db.lock().unwrap();';
const searchRegex = new RegExp(searchStart + '[\\\\s\\\\S]*?let _ = persist_thread_summary\\(&db, &thread_id_actual, &thread_title\\);\\n    \\}');

if (searchRegex.test(content)) {
    content = content.replace(searchRegex, `let assistant_msg_id = Uuid::new_v4().to_string();
    let thread_id_actual = ctx.thread_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());`);
    fs.writeFileSync(path, content, 'utf8');
    console.log('Patched generation.rs db writes out successfully.');
} else {
    // maybe thread_id_actual is handled differently, let's just do a manual replace
    console.log('Could not find exact block, let me use a simpler regex.');
}
