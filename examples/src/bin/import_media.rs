//! Import a media file and generate all caches through the background
//! pipeline.
//!
//! ```text
//! cargo run -p tpt-av-asset-examples --bin import_media [-- path/to/media.wav]
//! ```
//!
//! Without an argument a synthetic WAV is generated under `demo/`. The
//! engine home is `./.tpt-av-asset` (or `TPT_AV_ASSET_HOME`).

use std::time::Duration;

use tpt_av_asset_db::JobState;
use tpt_av_asset_pipeline::{AssetImporter, ProcessingPipeline};
use tpt_av_asset_utils::AssetError;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), AssetError> {
    let home = tpt_av_asset_examples::engine_home();
    let (db, storage) = tpt_av_asset_examples::open_engine(&home)?;
    println!("engine home: {}", home.display());

    let media = match std::env::args().nth(1) {
        Some(path) => tpt_av_asset_examples::ensure_demo_media(std::path::Path::new(&path))?,
        None => tpt_av_asset_examples::ensure_demo_media(
            &std::path::PathBuf::from("demo").join("demo_song.wav"),
        )?,
    };

    let mut pipeline = ProcessingPipeline::with_db(2, db.clone())?;
    pipeline.start()?;
    let importer = AssetImporter::new(pipeline.clone(), db.clone(), storage.clone());

    let started = std::time::Instant::now();
    let (asset, job_ids) = importer.import_with_jobs(&media)?;
    println!(
        "imported {} → asset {:016x}, {} background job(s) scheduled",
        media.display(),
        asset.hash(),
        job_ids.len()
    );

    // Watch the jobs run to completion.
    let deadline = Duration::from_secs(120);
    loop {
        let mut terminal = 0;
        let mut fraction_sum = 0.0;
        for job in &job_ids {
            if let Ok(snapshot) = pipeline.wait_for_timeout(*job, Duration::from_millis(25)) {
                fraction_sum += snapshot.fraction;
                if snapshot.state != JobState::Pending && snapshot.state != JobState::Running {
                    terminal += 1;
                } else {
                    tpt_av_asset_examples::print_progress(
                        snapshot.fraction,
                        &format!("job {}", job.0),
                    );
                }
            }
        }
        if terminal == job_ids.len() {
            break;
        }
        let _ = fraction_sum;
        if started.elapsed() > deadline {
            return Err(AssetError::validation("jobs did not finish in time"));
        }
    }

    println!("\nall jobs finished in {:.1?}:", started.elapsed());
    for (cache_type, path) in db.list_cache_entries(asset)? {
        println!("  {cache_type}: {}", path.display());
    }

    pipeline.stop()?;
    Ok(())
}
