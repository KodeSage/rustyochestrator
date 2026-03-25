use serde::Serialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let sec = secs % 60;
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let mut days = (secs / 86400) as u32; // days since 1970-01-01

    // Determine year, accounting for leap years
    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Determine month and day within the year
    let month_lengths = [
        31u32,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &len in &month_lengths {
        if days < len {
            break;
        }
        days -= len;
        month += 1;
    }
    let day = days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, min, sec
    )
}

#[inline]
fn is_leap(year: u32) -> bool {
    year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100))
}

/// Arguments for [`Event::pipeline_completed`].
pub struct PipelineCompletedArgs<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub success: bool,
    pub total_tasks: usize,
    pub cached_tasks: usize,
    pub failed_tasks: usize,
    pub duration_ms: u64,
    pub user_login: &'a str,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    PipelineStarted {
        pipeline_id: String,
        pipeline_name: String,
        total_tasks: usize,
        started_at: String,
        user_login: String,
    },
    TaskCompleted {
        pipeline_id: String,
        task_id: String,
        cache_hit: bool,
        duration_ms: u64,
        success: bool,
    },
    PipelineCompleted {
        pipeline_id: String,
        pipeline_name: String,
        status: String,
        total_tasks: usize,
        cached_tasks: usize,
        failed_tasks: usize,
        duration_ms: u64,
        finished_at: String,
        user_login: String,
    },
}

impl Event {
    pub fn pipeline_started(id: &str, name: &str, total_tasks: usize, user_login: &str) -> Self {
        Self::PipelineStarted {
            pipeline_id: id.to_string(),
            pipeline_name: name.to_string(),
            total_tasks,
            started_at: now_iso(),
            user_login: user_login.to_string(),
        }
    }

    pub fn task_completed(
        pipeline_id: &str,
        task_id: &str,
        cache_hit: bool,
        duration_ms: u64,
        success: bool,
    ) -> Self {
        Self::TaskCompleted {
            pipeline_id: pipeline_id.to_string(),
            task_id: task_id.to_string(),
            cache_hit,
            duration_ms,
            success,
        }
    }

    pub fn pipeline_completed(args: PipelineCompletedArgs<'_>) -> Self {
        Self::PipelineCompleted {
            pipeline_id: args.id.to_string(),
            pipeline_name: args.name.to_string(),
            status: if args.success { "success" } else { "failed" }.to_string(),
            total_tasks: args.total_tasks,
            cached_tasks: args.cached_tasks,
            failed_tasks: args.failed_tasks,
            duration_ms: args.duration_ms,
            finished_at: now_iso(),
            user_login: args.user_login.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Reporter {
    client: Arc<reqwest::Client>,
    url: String,
    token: String,
}

impl Reporter {
    pub fn new(url: String, token: String) -> Self {
        Self {
            client: Arc::new(reqwest::Client::new()),
            url,
            token,
        }
    }

    /// Fire and forget — spawns a background task so the pipeline is not slowed down.
    pub fn send(&self, event: Event) {
        let client = self.client.clone();
        let url = format!("{}/api/events", self.url.trim_end_matches('/'));
        let token = self.token.clone();
        tokio::spawn(async move {
            if let Err(e) = client
                .post(&url)
                .bearer_auth(&token)
                .json(&event)
                .send()
                .await
            {
                tracing::debug!("reporter: {}", e);
            }
        });
    }
}
