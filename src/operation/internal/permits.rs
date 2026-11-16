use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Atomic bounded counter whose permit returns only when its owner is dropped.
pub(crate) struct PermitPool {
    used: Arc<AtomicUsize>,
    limit: usize,
}

impl PermitPool {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            used: Arc::new(AtomicUsize::new(0)),
            limit,
        }
    }
    pub(crate) fn try_acquire(&self, amount: usize) -> Option<Permit> {
        if amount > self.limit {
            return None;
        }
        let mut current = self.used.load(Ordering::Acquire);
        loop {
            let next = current.checked_add(amount)?;
            if next > self.limit {
                return None;
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(Permit {
                        used: Arc::clone(&self.used),
                        amount,
                    });
                }
                Err(actual) => current = actual,
            }
        }
    }
}

pub(crate) struct Permit {
    used: Arc<AtomicUsize>,
    amount: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.used.fetch_sub(self.amount, Ordering::AcqRel);
    }
}
