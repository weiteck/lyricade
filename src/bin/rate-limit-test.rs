//! Basic testing of request rate limiting and concurrency values

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use rand::RngExt;
use tokio::sync::Semaphore;
use tokio::time::Duration;
use tokio::time::sleep;
use tracing::{Level, debug, info, trace};
use tracing_subscriber::FmtSubscriber;

const CONCURRENCY: usize = 20;

#[tokio::main]
async fn main() {
  let subscriber = FmtSubscriber::builder()
    .with_max_level(Level::INFO)
    .finish();
  tracing::subscriber::set_global_default(subscriber)
    .expect("setting default tracing subscriber failed");

  let limiter = Arc::new(
    leaky_bucket::RateLimiter::builder()
      .initial(4)
      .max(16)
      .refill(2)
      .interval(Duration::from_millis(150))
      .build(),
  );

  let semaphore = Arc::new(Semaphore::new(CONCURRENCY));

  let request_counter = Arc::new(AtomicUsize::new(0));

  // Spawn worker to log HTTP request rate
  let counter = Arc::clone(&request_counter);
  tokio::spawn(async move {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut last_count = 0;

    loop {
      interval.tick().await;
      trace!("Tick: Request rate logger");

      let count = counter.load(std::sync::atomic::Ordering::Relaxed);
      let delta = count.saturating_sub(last_count);
      last_count = count;

      if delta > 0 {
        info!(
          "Current completed request rate: {} req/sec ({} total requests)",
          delta, count
        );
      }
    }
  });

  info!("Limiter:\n{:#?}", limiter);

  let mut rng = rand::rng();

  let mut task: usize = 0;
  loop {
    task = task.wrapping_add(1);

    limiter.acquire_one().await;
    let permit = semaphore
      .clone()
      .acquire_owned()
      .await
      .expect("failed to acquire permit from semaphore");

    debug!(
      "Task {task} acquired bucket token; {} tokens remaining",
      limiter.balance()
    );
    debug!(
      "Task {task} acquired semaphore permit; {} permits remaining",
      semaphore.available_permits()
    );

    let duration = Duration::from_millis(rng.random_range(100..2000));
    let counter = Arc::clone(&request_counter);
    tokio::spawn(async move {
      let _permit = permit;
      sleep(duration).await;
      counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
      debug!("Task {task} completed after {} ms", duration.as_millis());
    });

    info!(
      "Current tasks: {}",
      CONCURRENCY - semaphore.available_permits()
    );
  }
}
