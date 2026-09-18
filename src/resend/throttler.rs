use std::time::{Duration, Instant};

use futures::lock::Mutex;

/// Spaces requests evenly so that at most `per_second` are started each second.
pub struct Throttler {
    interval: Duration,
    next_slot: Mutex<Instant>,
}

impl Throttler {
    pub fn new(per_second: u32) -> Self {
        Throttler {
            interval: Duration::from_secs_f64(1.0 / per_second.max(1) as f64),
            next_slot: Mutex::new(Instant::now()),
        }
    }

    pub async fn wait(&self) {
        let slot = {
            let mut next_slot = self.next_slot.lock().await;
            let slot = (*next_slot).max(Instant::now());
            *next_slot = slot + self.interval;
            slot
        };
        actix_rt::time::sleep_until(slot.into()).await;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[actix_rt::test]
    async fn test_should_space_requests() {
        let throttler = Throttler::new(10);
        let start = Instant::now();
        throttler.wait().await;
        throttler.wait().await;
        throttler.wait().await;
        assert!(start.elapsed() >= Duration::from_millis(190));
    }
}
