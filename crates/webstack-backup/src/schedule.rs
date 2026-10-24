use std::{sync::Arc, time::Duration};

use chrono::Utc;
use croner::Cron;
use tokio::{task::JoinHandle, time::sleep};

use crate::{BackupError, Backups};

const MAX_SLEEP: Duration = Duration::from_hours(1);

/// Starts the background task that runs backups on a UTC cron schedule.
///
/// # Errors
///
/// Returns [`BackupError::Schedule`] when the expression cannot be parsed.
pub fn spawn_schedule(backups: Arc<Backups>, cron: &str) -> Result<JoinHandle<()>, BackupError> {
    let schedule = cron
        .parse::<Cron>()
        .map_err(|error| BackupError::Schedule(error.to_string()))?;
    Ok(tokio::spawn(run_schedule(backups, schedule)))
}

/// Waits for each scheduled time and runs one backup.
async fn run_schedule(backups: Arc<Backups>, schedule: Cron) {
    loop {
        let next = match schedule.find_next_occurrence(&Utc::now(), false) {
            Ok(next) => next,
            Err(error) => {
                tracing::error!(%error, "backup schedule has no next occurrence; scheduled backups stopped");
                return;
            }
        };
        tracing::info!(next = %next.to_rfc3339(), "next database backup scheduled");
        while let Ok(remaining) = (next - Utc::now()).to_std() {
            if remaining.is_zero() {
                break;
            }
            sleep(remaining.min(MAX_SLEEP)).await;
        }
        if let Err(error) = backups.run().await {
            tracing::error!(error = %error_chain(&error), "scheduled database backup failed");
        }
    }
}

/// Formats an error with its source chain for one log field.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
