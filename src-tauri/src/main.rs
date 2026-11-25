// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if ecky_cad_lib::services::agy_voice::maybe_print_terminal_title() {
        return;
    }
    if let Some(exit_code) =
        ecky_cad_lib::services::fem_solver_worker::maybe_run_from_process_args()
    {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) =
        ecky_cad_lib::services::codex_pre_tool_hook::maybe_run_from_process_args()
    {
        std::process::exit(exit_code);
    }
    ecky_cad_lib::run();
}
