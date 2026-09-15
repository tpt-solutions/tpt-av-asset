//! Generate proxies for a folder of media.
//!
//! ```text
//! cargo run -p tpt-av-asset-examples --bin proxy_generator [-- path/to/folder]
//! ```
//!
//! Walks the folder for `.wav` / `.tkv` files and renders each through the
//! proxy engine (720p medium preset) into `<folder>/proxies/`.

use std::path::{Path, PathBuf};

use tpt_av_asset_proxy::{ProxyGenerator, ProxyProfile};
use tpt_av_asset_utils::{AssetError, ProgressReporter};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), AssetError> {
    let folder: PathBuf = match std::env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        None => {
            // Demo mode: synthesize a couple of sources to convert.
            let demo = PathBuf::from("demo").join("batch");
            tpt_av_asset_examples::ensure_demo_media(&demo.join("intro.wav"))?;
            tpt_av_asset_examples::ensure_demo_media(&demo.join("b_roll.tkv"))?;
            demo
        }
    };

    let folder = if folder.is_dir() {
        folder
    } else {
        folder
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let sources = collect_media(&folder)?;
    if sources.is_empty() {
        println!("no .wav / .tkv files found under {}", folder.display());
        return Ok(());
    }

    let out_dir = folder.join("proxies");
    std::fs::create_dir_all(&out_dir)?;
    let generator = ProxyGenerator::new(ProxyProfile::proxy_720p_medium());
    let audio_generator = ProxyGenerator::new(ProxyProfile::audio_proxy_flac());

    println!(
        "generating proxies for {} file(s) under {} (720p Medium / FLAC)\n",
        sources.len(),
        folder.display()
    );

    let mut failed = 0usize;
    for source in &sources {
        let is_audio = source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "media".to_string());
        let output = out_dir.join(if is_audio {
            format!("{stem}_proxy.flac")
        } else {
            format!("{stem}_proxy.mp4")
        });

        print!("  {} → {} ", source.display(), output.display());
        let reporter = ProgressReporter::with_callback(|event| {
            tpt_av_asset_examples::print_progress(event.fraction, "render");
        });
        let result = if is_audio {
            audio_generator.generate_audio_proxy(source, &output, &reporter)
        } else {
            generator.generate_video_proxy(source, &output, &reporter)
        };
        match result {
            Ok(()) => println!(
                "  done ({} B)",
                std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0)
            ),
            Err(e) => {
                failed += 1;
                println!("  FAILED: {e}");
            }
        }
    }

    if failed > 0 {
        return Err(AssetError::validation(format!(
            "{failed} proxy render(s) failed"
        )));
    }
    println!("\nall proxies written to {}", out_dir.display());
    Ok(())
}

fn collect_media(folder: &Path) -> Result<Vec<PathBuf>, AssetError> {
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_media = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            let e = e.to_ascii_lowercase();
            e == "wav" || e == "tkv"
        });
        if is_media {
            sources.push(path);
        }
    }
    sources.sort();
    Ok(sources)
}
