//! The scheduling policy: seL4-style scheduling contexts, then EEVDF.
//!
//! Every thread has a scheduling context (`Params`): a priority, and a CPU
//! time budget per period (or no budget limit). Higher priorities run
//! first. A thread that has used its budget is throttled until its period
//! ends and the budget is refilled, whatever its priority, so a thread can
//! be urgent without being able to take a whole core. (seL4 refills budgets
//! with sporadic servers; this is the simpler periodic refill. Making these
//! contexts capabilities that can be lent along with a message, like
//! Linux's proxy execution, comes with the capability work.)
//!
//! Among threads of the same priority, EEVDF (Linux's fair scheduler since
//! 6.6, still in 7.3) picks. Every thread has a virtual runtime that grows
//! with the time it runs, more slowly the higher its weight; the queue's
//! average virtual runtime says what everyone is owed. A thread is
//! eligible when it has not run ahead of that average, and of the eligible
//! threads the one whose virtual deadline (virtual runtime plus one time
//! slice, scaled by weight) comes first runs.

use super::{Enqueue, Policy, ThreadRef};
use alloc::vec::Vec;

/// The weight of a normal thread (Linux's nice 0).
pub const NORMAL_WEIGHT: u64 = 1024;

/// A thread's scheduling context.
#[derive(Clone, Copy)]
pub struct Params {
    /// Share of CPU time among its priority, relative to NORMAL_WEIGHT.
    pub weight: u64,
    /// Higher runs first.
    pub priority: u8,
    /// CPU time (counter ticks) it may use per period; 0 = no limit.
    pub budget: u64,
    pub period: u64,
    /// Stays on the core it was put on (never stolen).
    pub pinned: bool,
}

impl Params {
    pub const fn normal() -> Self {
        Params { weight: NORMAL_WEIGHT, priority: 0, budget: 0, period: 0, pinned: false }
    }
}

/// Policy state of one thread.
pub struct Entity {
    pub params: Params,
    vruntime: u64,
    deadline: u64,
    slice_left: u64,
    /// Budget left in this period, and when the period ends.
    remaining: u64,
    period_end: u64,
    throttled: bool,
    /// How far behind the queue's average it was when it left a queue.
    lag: i64,
}

impl Entity {
    pub const fn new(params: Params) -> Self {
        Entity { params, vruntime: 0, deadline: 0, slice_left: 0, remaining: 0, period_end: 0, throttled: false, lag: 0 }
    }
    fn limited(&self) -> bool {
        self.params.budget != 0 && self.params.budget < self.params.period
    }
}

fn entity(t: ThreadRef) -> &'static mut Entity {
    // SAFETY: a thread's entity is only touched by the policy of the queue
    // that holds it (or of the core it runs on), under that queue's lock.
    unsafe { &mut *t.entity() }
}

pub struct McsEevdf {
    queue: Vec<ThreadRef>,
    /// EEVDF's time slice, in counter ticks.
    slice: u64,
    /// The average virtual runtime when the queue was last not empty.
    clock: u64,
}

impl McsEevdf {
    pub fn new(slice: u64) -> Self {
        McsEevdf { queue: Vec::new(), slice, clock: 0 }
    }

    /// Weighted average virtual runtime of the threads of `priority` that
    /// may run (all of the queue's, if `priority` is None).
    fn average(&self, priority: Option<u8>) -> u64 {
        let (mut sum, mut weights) = (0u128, 0u128);
        for &t in &self.queue {
            let e = entity(t);
            if priority.is_none_or(|p| !e.throttled && e.params.priority == p) {
                sum += e.vruntime as u128 * e.params.weight as u128;
                weights += e.params.weight as u128;
            }
        }
        sum.checked_div(weights).map_or(self.clock, |average| average as u64)
    }

    /// Virtual time for `ticks` of real time at weight `weight`.
    fn scaled(ticks: u64, weight: u64) -> u64 {
        (ticks as u128 * NORMAL_WEIGHT as u128 / weight.max(1) as u128) as u64
    }

    fn new_slice(&self, e: &mut Entity) {
        e.slice_left = self.slice;
        e.deadline = e.vruntime + Self::scaled(self.slice, e.params.weight);
    }

    fn refill(e: &mut Entity, now: u64) {
        if e.limited() && now >= e.period_end {
            e.remaining = e.params.budget;
            e.period_end = now + e.params.period;
            e.throttled = false;
        }
    }
}

impl Policy for McsEevdf {
    fn enqueue(&mut self, t: ThreadRef, now: u64, how: Enqueue) {
        let average = self.average(None);
        let e = entity(t);
        match how {
            Enqueue::New => {
                e.vruntime = average;
                e.remaining = e.params.budget;
                e.period_end = now + e.params.period;
                self.new_slice(e);
            }
            // Keep what it was owed, but no more than one slice.
            Enqueue::Wakeup => e.vruntime = e.vruntime.max(average.saturating_sub(Self::scaled(self.slice, e.params.weight))),
            Enqueue::Preempted => {}
            Enqueue::Migrated => {
                e.vruntime = (average as i64 - e.lag).max(0) as u64;
                e.deadline = e.vruntime + Self::scaled(e.slice_left, e.params.weight);
            }
        }
        self.queue.push(t);
        self.clock = self.clock.max(self.average(None));
    }

    fn pick(&mut self, now: u64) -> Option<ThreadRef> {
        for &t in &self.queue {
            Self::refill(entity(t), now);
        }
        let top = self.queue.iter().map(|&t| entity(t)).filter(|e| !e.throttled).map(|e| e.params.priority).max()?;
        let average = self.average(Some(top));
        let candidates = || self.queue.iter().enumerate().filter(|&(_, &t)| {
            let e = entity(t);
            !e.throttled && e.params.priority == top
        });
        // The earliest deadline among the eligible; if rounding left none
        // eligible, the earliest deadline at all.
        let earliest = |eligible_only: bool| {
            candidates()
                .filter(|&(_, &t)| !eligible_only || entity(t).vruntime <= average)
                .min_by_key(|&(_, &t)| entity(t).deadline)
                .map(|(i, _)| i)
        };
        let index = earliest(true).or_else(|| earliest(false))?;
        self.clock = self.clock.max(average);
        Some(self.queue.swap_remove(index))
    }

    fn charge(&mut self, t: ThreadRef, ran: u64, now: u64) -> bool {
        let slice = self.slice;
        let e = entity(t);
        e.vruntime += Self::scaled(ran, e.params.weight);
        e.slice_left = e.slice_left.saturating_sub(ran);
        let mut give_up = false;
        if e.limited() {
            Self::refill(e, now);
            e.remaining = e.remaining.saturating_sub(ran);
            if e.remaining == 0 {
                e.throttled = true;
                give_up = true;
            }
        }
        if e.slice_left == 0 {
            e.slice_left = slice;
            e.deadline = e.vruntime + Self::scaled(slice, e.params.weight);
            give_up = true;
        }
        give_up
    }

    fn time_left(&self, t: ThreadRef) -> u64 {
        let e = entity(t);
        if e.limited() { e.slice_left.min(e.remaining) } else { e.slice_left }
    }

    fn limited(&self, t: ThreadRef) -> bool {
        entity(t).limited()
    }

    fn refill_preempts(&mut self, now: u64, current: ThreadRef) -> bool {
        let mut preempts = false;
        for &t in &self.queue {
            let e = entity(t);
            if e.throttled {
                Self::refill(e, now);
                preempts |= !e.throttled && e.params.priority > entity(current).params.priority;
            }
        }
        preempts
    }

    fn next_event(&self) -> Option<u64> {
        self.queue.iter().map(|&t| entity(t)).filter(|e| e.throttled).map(|e| e.period_end).min()
    }

    fn ready(&self, now: u64) -> bool {
        self.queue.iter().any(|&t| {
            let e = entity(t);
            !e.throttled || now >= e.period_end
        })
    }

    fn steal(&mut self, now: u64) -> Option<ThreadRef> {
        // The least urgent thread that may run now and may move.
        let average = self.average(None);
        let index = self
            .queue
            .iter()
            .enumerate()
            .filter(|&(_, &t)| {
                let e = entity(t);
                !e.params.pinned && !t.on_cpu() && (!e.throttled || now >= e.period_end)
            })
            .max_by_key(|&(_, &t)| entity(t).deadline)
            .map(|(i, _)| i)?;
        let t = self.queue.swap_remove(index);
        let e = entity(t);
        e.lag = average as i64 - e.vruntime as i64;
        Some(t)
    }

    fn preempts(&self, t: ThreadRef, current: ThreadRef) -> bool {
        entity(t).params.priority > entity(current).params.priority
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
}
