//! One thread for jobs that run after a delay, rather than a sleeping
//! thread per job (the idle check a minute after each Stop).

use crate::locks::LockExt;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Queue {
    /// Sorted by deadline; jobs with the same deadline keep their order.
    jobs: VecDeque<(Instant, Job)>,
}

#[derive(Default)]
pub struct Timer {
    inner: Arc<(Mutex<Queue>, Condvar)>,
    started: OnceLock<()>,
}

impl Timer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `job` once `delay` has passed, on the timer's thread. A job that
    /// panics is reported and the timer carries on.
    pub fn after(&self, delay: Duration, job: impl FnOnce() + Send + 'static) {
        self.started.get_or_init(|| {
            let inner = Arc::clone(&self.inner);
            std::thread::Builder::new()
                .name("timer".into())
                .spawn(move || run(&inner))
                .expect("cannot start the timer thread");
        });
        let at = Instant::now() + delay;
        let (lock, cv) = &*self.inner;
        let mut q = lock.lk();
        let i = q.jobs.partition_point(|(t, _)| *t <= at);
        q.jobs.insert(i, (at, Box::new(job)));
        cv.notify_one();
    }

    #[cfg(test)]
    fn pending(&self) -> usize {
        self.inner.0.lk().jobs.len()
    }
}

fn run(inner: &(Mutex<Queue>, Condvar)) {
    let (lock, cv) = inner;
    let mut q = lock.lk();
    loop {
        let now = Instant::now();
        match q.jobs.front() {
            None => q = cv.wait(q).unwrap_or_else(std::sync::PoisonError::into_inner),
            Some((at, _)) if *at > now => {
                let left = *at - now;
                q = cv
                    .wait_timeout(q, left)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            Some(_) => {
                let (_, job) = q.jobs.pop_front().expect("a job is there");
                drop(q);
                if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
                    let text = e
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| e.downcast_ref::<String>().cloned())
                        .unwrap_or_default();
                    eprintln!("imperatorium: a timed job panicked: {text}");
                }
                q = lock.lk();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn jobs_run_in_deadline_order_on_one_thread() {
        let t = Timer::new();
        let (tx, rx) = mpsc::channel();
        for (ms, n) in [(120u64, 3), (40, 1), (80, 2), (80, 22)] {
            let tx = tx.clone();
            t.after(Duration::from_millis(ms), move || {
                tx.send((n, std::thread::current().id())).unwrap();
            });
        }
        let got: Vec<_> = (0..4).map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap()).collect();
        assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![1, 2, 22, 3]);
        assert!(got.iter().all(|g| g.1 == got[0].1));
        assert_eq!(t.pending(), 0);
    }

    #[test]
    fn a_panicking_job_does_not_stop_the_timer() {
        let t = Timer::new();
        let (tx, rx) = mpsc::channel();
        t.after(Duration::from_millis(10), || panic!("boom"));
        t.after(Duration::from_millis(20), move || tx.send(()).unwrap());
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
