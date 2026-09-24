use chrono::Utc;
use cron::Schedule;
use futures::Future;

pub fn spawn_job<F, Fut>(
    schedule: Schedule,
    job_name: &'static str,
    mut run: F,
) -> tokio::task::JoinHandle<()>
where
    F: (FnMut() -> Fut) + Send + 'static,
    Fut: Future + Send + 'static,
    <Fut as futures::Future>::Output: Send,
{
    tokio::spawn(async move {
        loop {
            let sleep_duration = time_till_next_call(&schedule);
            tracing::debug!(
                "scheduled next run of '{}' in {:?}",
                job_name,
                sleep_duration
            );
            tokio::time::sleep(sleep_duration).await;
            run().await;
        }
    })
}

fn time_till_next_call(schedule: &Schedule) -> std::time::Duration {
    let default = std::time::Duration::from_millis(500);
    let now = Utc::now();

    schedule
        .upcoming(Utc)
        .next()
        .map_or(default, |t| (t - now).to_std().unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn next_call() {
        assert!(
            // every second
            time_till_next_call(&Schedule::from_str("* * * * * * *").unwrap())
                <= std::time::Duration::from_secs(1)
        );

        assert!(
            // every 15 seconds
            time_till_next_call(&Schedule::from_str("0/15 * * * * * *").unwrap())
                <= std::time::Duration::from_secs(15)
        );

        assert!(
            // every hour
            time_till_next_call(&Schedule::from_str("0 0 * * * * *").unwrap())
                <= std::time::Duration::from_secs(60 * 60)
        );
    }
}
