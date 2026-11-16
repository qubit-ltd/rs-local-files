// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

struct PermitState {
    used: AtomicUsize,
    limit: usize,
}

/// Atomic bounded counter whose permits return exactly their reserved amount.
pub(crate) struct PermitPool {
    state: Arc<PermitState>,
}

impl PermitPool {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            state: Arc::new(PermitState {
                used: AtomicUsize::new(0),
                limit,
            }),
        }
    }

    pub(crate) fn try_acquire(&self, amount: usize) -> Option<Permit> {
        let mut permit = Permit {
            state: Arc::clone(&self.state),
            amount: 0,
        };
        permit.grow(amount).then_some(permit)
    }

    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.state.used.load(Ordering::Acquire)
    }
}

pub(crate) struct Permit {
    state: Arc<PermitState>,
    amount: usize,
}

impl Permit {
    pub(crate) fn amount(&self) -> usize {
        self.amount
    }

    pub(crate) fn grow(&mut self, amount: usize) -> bool {
        if amount == 0 {
            return true;
        }
        let Some(next_amount) = self.amount.checked_add(amount) else {
            return false;
        };
        let mut current = self.state.used.load(Ordering::Acquire);
        loop {
            let Some(next_used) = current.checked_add(amount) else {
                return false;
            };
            if next_used > self.state.limit {
                return false;
            }
            match self.state.used.compare_exchange_weak(
                current,
                next_used,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.amount = next_amount;
                    return true;
                }
                Err(actual) => current = actual,
            }
        }
    }

    pub(crate) fn shrink(&mut self, amount: usize) {
        let released = amount.min(self.amount);
        self.amount -= released;
        self.state.used.fetch_sub(released, Ordering::AcqRel);
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.state.used.fetch_sub(self.amount, Ordering::AcqRel);
    }
}

#[allow(dead_code)]
struct DiskPermitState {
    used: AtomicU64,
    limit: u64,
}

#[allow(dead_code)]
pub(crate) struct DiskPermitPool {
    state: Arc<DiskPermitState>,
}

#[allow(dead_code)]
impl DiskPermitPool {
    pub(crate) fn new(limit: u64) -> Self {
        Self {
            state: Arc::new(DiskPermitState {
                used: AtomicU64::new(0),
                limit,
            }),
        }
    }

    pub(crate) fn try_acquire(&self, amount: u64) -> Option<DiskPermit> {
        let mut permit = DiskPermit {
            state: Arc::clone(&self.state),
            amount: 0,
        };
        permit.grow(amount).then_some(permit)
    }

    pub(crate) fn used(&self) -> u64 {
        self.state.used.load(Ordering::Acquire)
    }
}

#[allow(dead_code)]
pub(crate) struct DiskPermit {
    state: Arc<DiskPermitState>,
    amount: u64,
}

#[allow(dead_code)]
impl DiskPermit {
    pub(crate) fn amount(&self) -> u64 {
        self.amount
    }

    pub(crate) fn grow(&mut self, amount: u64) -> bool {
        let Some(next_amount) = self.amount.checked_add(amount) else {
            return false;
        };
        let mut current = self.state.used.load(Ordering::Acquire);
        loop {
            let Some(next_used) = current.checked_add(amount) else {
                return false;
            };
            if next_used > self.state.limit {
                return false;
            }
            match self.state.used.compare_exchange_weak(
                current,
                next_used,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.amount = next_amount;
                    return true;
                }
                Err(actual) => current = actual,
            }
        }
    }
}

#[allow(dead_code)]
impl Drop for DiskPermit {
    fn drop(&mut self) {
        self.state.used.fetch_sub(self.amount, Ordering::AcqRel);
    }
}
