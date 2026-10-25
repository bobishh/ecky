use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match ecky_cad_lib::cairn::run(&arguments).await {
        Ok(result) => {
            ecky_cad_lib::cairn::print_result(
                &result,
                arguments.iter().any(|argument| argument == "--json"),
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cairn: {error}");
            ExitCode::FAILURE
        }
    }
}
