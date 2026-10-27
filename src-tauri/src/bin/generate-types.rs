use std::{env, error::Error, fs, path::PathBuf, process::ExitCode};

const GENERATED_PATH: &str = "../src/lib/generated/ui-contract.ts";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let check = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [argument] if argument == "--check" => true,
        _ => return Err("usage: generate-types [--check]".into()),
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GENERATED_PATH);
    let generated = picker_app_lib::contract::generated_typescript();

    if check {
        let committed = fs::read_to_string(&path).map_err(|error| {
            format!(
                "generated UI contract is missing at {}: {error}; run the generation command",
                path.display()
            )
        })?;
        if committed != generated {
            return Err(format!(
                "generated UI contract is stale at {}; run `cargo run -p picker-app --bin generate-types`",
                path.display()
            )
            .into());
        }
        println!("generated UI contract is current: {}", path.display());
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, generated)?;
    println!("generated UI contract: {}", path.display());
    Ok(())
}
