use std::{
    fs::{
        self,
        metadata,
        File,
    },
    path::Path,
};

use anyhow::Context;
use chrono::{
    DateTime,
    Local,
};
use simplelog::{
    ColorChoice,
    CombinedLogger,
    ConfigBuilder,
    LevelFilter,
    TermLogger,
    TerminalMode,
    WriteLogger,
};

pub const LATEST_LOG_FILENAME: &str = "./latest.log";

pub fn push_back_latest_log() -> anyhow::Result<()> {
    let metadata = metadata(LATEST_LOG_FILENAME)
        .with_context(|| format!("Grabbing metadata for {}", LATEST_LOG_FILENAME))?;
    let time = metadata
        .modified()
        .or_else(|e| {
            metadata
                .created()
                .or_else(|_| metadata.accessed().or(Err(e)))
        })
        .with_context(|| format!("Checking metadata timestamps for {}", LATEST_LOG_FILENAME))?;
    let datetime = DateTime::<Local>::from(time);
    let date_fmt = datetime.format("%Y%m%d_%H%M");
    let new_filename = format!("./logs/{}.log", date_fmt);
    if !Path::new("logs").is_dir() {
        fs::create_dir("logs").with_context(|| "Creating logs directory")?;
    }
    fs::copy(LATEST_LOG_FILENAME, &new_filename)
        .with_context(|| format!("Copying {} to {}", LATEST_LOG_FILENAME, &new_filename))?;
    fs::remove_file(LATEST_LOG_FILENAME)
        .with_context(|| format!("Removing {}", LATEST_LOG_FILENAME))?;
    Ok(())
}

pub fn init() -> anyhow::Result<()> {
    if Path::new(LATEST_LOG_FILENAME).is_file() {
        push_back_latest_log()
            .with_context(|| format!("Trying to move {} into logs folder", LATEST_LOG_FILENAME))?;
    }
    let config = ConfigBuilder::default().set_time_to_local(true).build();
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Debug,
            config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Debug, /* setting to trace will result in a lot from the async libraries used in this project */
            config,
            File::create(LATEST_LOG_FILENAME).unwrap(),
        ),
    ])?;
    Ok(())
}
