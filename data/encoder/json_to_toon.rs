use serde_json::Value;
use std::fs;
use std::path::Path;
use toon_format::encode_default;

const TARGET_DIRS: [&str; 2] = ["data", "data/pass"];

fn convert_json_in_dirs(directories: &[&str]) {
    for dir in directories {
        let dir_path = Path::new(dir);

        if !dir_path.exists() {
            eprintln!("Directory not found: {dir}");
            continue;
        }

        let entries = match fs::read_dir(dir_path) {
            Ok(entries) => entries,
            Err(err) => {
                eprintln!("Failed to read directory {dir}: {err}");
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    eprintln!("Failed to read entry in {dir}: {err}");
                    continue;
                }
            };

            let path = entry.path();

            let is_json_file =
                path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json");

            if !is_json_file {
                continue;
            }

            let toon_path = path.with_extension("toon");

            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let raw_data = fs::read_to_string(&path)?;
                let json_value: Value = serde_json::from_str(&raw_data)?;
                let toon = encode_default(&json_value)?;

                fs::write(&toon_path, toon)?;

                Ok(())
            })();

            match result {
                Ok(()) => println!("Converted: {} -> {}", path.display(), toon_path.display()),
                Err(err) => eprintln!("Failed to process {}: {}", path.display(), err),
            }
        }
    }
}

fn main() {
    convert_json_in_dirs(&TARGET_DIRS);
}
