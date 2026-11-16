use std::collections::VecDeque;
use std::sync::Condvar;
use std::sync::Mutex;

use super::permits::Permit;

pub(crate) struct QueueState {
    ordinary: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
    cleanup: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
    stopping: bool,
    last_was_cleanup: bool,
}

pub(crate) struct WorkerQueue {
    state: Mutex<QueueState>,
    wake: Condvar,
}

impl Default for WorkerQueue {
    fn default() -> Self {
        Self {
            state: Mutex::new(QueueState {
                ordinary: VecDeque::new(),
                cleanup: VecDeque::new(),
                stopping: false,
                last_was_cleanup: false,
            }),
            wake: Condvar::new(),
        }
    }
}

impl WorkerQueue {
    pub(crate) fn push(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        cleanup: bool,
        limit: usize,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping {
            return Err(job);
        }
        let queue = if cleanup {
            &mut state.cleanup
        } else {
            &mut state.ordinary
        };
        if queue.len() >= limit {
            return Err(job);
        }
        queue.push_back(job);
        self.wake.notify_one();
        Ok(())
    }

    pub(crate) fn push_cleanup(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
        permit: Permit,
        limit: usize,
    ) -> Result<(), (Box<dyn FnOnce() + Send + 'static>, Permit)> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping || state.cleanup.len() >= limit {
            return Err((job, permit));
        }
        let wrapped = Box::new(move || {
            let _permit = permit;
            job();
        });
        state.cleanup.push_back(wrapped);
        self.wake.notify_one();
        Ok(())
    }

    pub(crate) fn run(&self) {
        loop {
            let job = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                while state.ordinary.is_empty() && state.cleanup.is_empty() && !state.stopping {
                    state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                if state.stopping && state.ordinary.is_empty() && state.cleanup.is_empty() {
                    return;
                }
                let choose_cleanup = if state.cleanup.is_empty() {
                    false
                } else if state.ordinary.is_empty() {
                    true
                } else {
                    !state.last_was_cleanup
                };
                state.last_was_cleanup = choose_cleanup;
                if choose_cleanup {
                    state.cleanup.pop_front()
                } else {
                    state.ordinary.pop_front()
                }
            };
            if let Some(job) = job {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            }
        }
    }

    pub(crate) fn stop(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopping = true;
        self.wake.notify_all();
    }
}
