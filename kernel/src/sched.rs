//! Threads and the scheduler.
//!
//! A thread is a stack plus the registers it needs to carry on: while it
//! does not run, `Context` holds its callee-saved registers, return address
//! and stack pointer (everything else was saved by whoever called the
//! switch). Switching threads is saving one context and loading another.
//!
//! Every core has its own run queue, and an idle thread that runs when the
//! queue is empty: it steals a waiting thread from a busy core, or sleeps
//! in `wfi`. Which thread runs next is the business of a `Policy` behind a
//! trait (pluggable, like Linux's sched_ext); the first one is scheduling
//! contexts plus EEVDF (sched/policy.rs).
//!
//! Preemption is lazy, Linux's preferred mode on arm64 since 7.0: a thread
//! runs until its time slice or budget is used up (the timer says so), and
//! only a thread of a higher priority waking up preempts it at once. The
//! timer is set only for the next such event: a thread alone on its core
//! with no budget limit runs without any timer interrupts.
//!
//! A switch happens with this core's run-queue lock held, and the thread
//! switched to releases it. Threads may move between cores, so a thread
//! switched out on one core stays `on_cpu` until the next thread on that
//! core has finished the switch, and nobody else may run it before.

mod policy;

pub use policy::Params;
use policy::{Entity, McsEevdf};

use crate::stacks::MAX_CORES;
use crate::sync::{IrqsOff, TicketLock, comes_before, irqs, order};
use crate::{gic, irq, smp, stacks, timer, vm};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt::Write;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, AtomicUsize, Ordering};

// A run queue may grow (allocate) while it is locked.
const _: () = comes_before::<order::RunQueue, order::Heap>();

/// EEVDF's time slice: 3 ms, Linux's default for 8 or more cores.
const SLICE_US: u64 = 3000;
/// The SGI a core sends to make another one look at its run queue.
const RESCHEDULE: u32 = 2;

/// What a thread keeps across a switch: x19..x30 and its stack pointer.
#[repr(C)]
struct Context {
    regs: [u64; 12],
    sp: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum State {
    Runnable = 0,
    Running = 1,
    Sleeping = 2,
    Dead = 3,
}

pub struct Thread {
    context: UnsafeCell<Context>,
    pub id: usize,
    pub name: &'static str,
    entry: fn(usize),
    arg: usize,
    /// Its registers are live on some core (or still being saved).
    on_cpu: AtomicBool,
    state: AtomicU8,
    /// CPU time used, in counter ticks.
    runtime: AtomicU64,
    /// The cores it has run on, one bit each.
    cores: AtomicU64,
    entity: UnsafeCell<Entity>,
}

// SAFETY: `context` is only touched by the switch, which owns the thread at
// that moment (`on_cpu`); `entity` only under its run queue's lock.
unsafe impl Sync for Thread {}

pub type ThreadRef = &'static Thread;

impl Thread {
    fn entity(&self) -> *mut Entity {
        self.entity.get()
    }
    fn on_cpu(&self) -> bool {
        self.on_cpu.load(Ordering::Acquire)
    }
    fn state(&self) -> State {
        match self.state.load(Ordering::Acquire) {
            0 => State::Runnable,
            1 => State::Running,
            2 => State::Sleeping,
            _ => State::Dead,
        }
    }
    fn set_state(&self, state: State) {
        self.state.store(state as u8, Ordering::Release);
    }
    /// CPU time used so far, in counter ticks.
    pub fn runtime(&self) -> u64 {
        self.runtime.load(Ordering::Relaxed)
    }
    pub fn finished(&self) -> bool {
        self.state() == State::Dead
    }
}

/// How a thread joins a run queue.
pub enum Enqueue {
    New,
    Wakeup,
    Preempted,
    Migrated,
}

/// A scheduling policy for one core's run queue.
pub trait Policy: Send {
    /// `t` may run on this core from now on.
    fn enqueue(&mut self, t: ThreadRef, now: u64, how: Enqueue);
    /// Take the thread that should run next, if one may run now.
    fn pick(&mut self, now: u64) -> Option<ThreadRef>;
    /// The running `t` used `ran` ticks; true if it should give way now.
    fn charge(&mut self, t: ThreadRef, ran: u64, now: u64) -> bool;
    /// Ticks the running `t` may still use before `charge` says so.
    fn time_left(&self, t: ThreadRef) -> u64;
    /// `t` has a budget, which must be cut off on time even if it is alone.
    fn limited(&self, t: ThreadRef) -> bool;
    /// Refill the budgets that are due; true if a refilled thread should
    /// preempt the running `current` at once.
    fn refill_preempts(&mut self, now: u64, current: ThreadRef) -> bool;
    /// When a queued thread next becomes runnable by itself (a budget refill).
    fn next_event(&self) -> Option<u64>;
    /// Some queued thread may run now.
    fn ready(&self, now: u64) -> bool;
    /// Give a thread that may run now to another core.
    fn steal(&mut self, now: u64) -> Option<ThreadRef>;
    /// Waking `t` should preempt the running `current` at once.
    fn preempts(&self, t: ThreadRef, current: ThreadRef) -> bool;
    fn len(&self) -> usize;
}

struct Queue {
    lock: TicketLock<IrqsOff, order::RunQueue>,
    inner: UnsafeCell<Inner>,
}

struct Inner {
    policy: Option<Box<dyn Policy>>,
    /// Sleeping threads and when they wake.
    sleepers: Vec<(u64, ThreadRef)>,
    /// When the running thread was last charged.
    charged: u64,
}

// SAFETY: `inner` is only reached with `lock` held.
unsafe impl Sync for Queue {}

impl Queue {
    const fn new() -> Self {
        Queue { lock: TicketLock::new(), inner: UnsafeCell::new(Inner { policy: None, sleepers: Vec::new(), charged: 0 }) }
    }

    /// Lock it; the guard unlocks when dropped.
    fn lock(&self) -> Locked<'_> {
        self.lock.raw_lock();
        Locked(self)
    }
}

struct Locked<'a>(&'a Queue);

impl core::ops::Deref for Locked<'_> {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        // SAFETY: the lock is held.
        unsafe { &*self.0.inner.get() }
    }
}
impl core::ops::DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut Inner {
        // SAFETY: the lock is held.
        unsafe { &mut *self.0.inner.get() }
    }
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        // SAFETY: taken in `Queue::lock` on this core.
        unsafe { self.0.lock.raw_unlock() };
    }
}

static QUEUES: [Queue; MAX_CORES] = [const { Queue::new() }; MAX_CORES];
/// Per core: the running thread, its idle thread, and the thread it is
/// switching away from (until the switch is finished).
static CURRENT: [AtomicPtr<Thread>; MAX_CORES] = [const { AtomicPtr::new(null_mut()) }; MAX_CORES];
static IDLE: [AtomicPtr<Thread>; MAX_CORES] = [const { AtomicPtr::new(null_mut()) }; MAX_CORES];
static PREV: [AtomicPtr<Thread>; MAX_CORES] = [const { AtomicPtr::new(null_mut()) }; MAX_CORES];
/// Per core: should it switch threads? 0 no, LAZY at the next chance
/// (its slice's end), NOW at the next interrupt return.
static RESCHED: [AtomicU8; MAX_CORES] = [const { AtomicU8::new(0) }; MAX_CORES];
const LAZY: u8 = 1;
const NOW: u8 = 2;
/// Cores whose scheduler runs, one bit each.
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
static SLICE: AtomicU64 = AtomicU64::new(0);

fn current(core: usize) -> ThreadRef {
    // SAFETY: set before the core's scheduler is active, never freed.
    unsafe { &*CURRENT[core].load(Ordering::Acquire) }
}
fn idle(core: usize) -> ThreadRef {
    // SAFETY: as above.
    unsafe { &*IDLE[core].load(Ordering::Acquire) }
}
fn active(core: usize) -> bool {
    ACTIVE.load(Ordering::Acquire) & 1 << core != 0
}

/// The thread running on this core.
pub fn current_thread() -> Option<ThreadRef> {
    let core = smp::this_core();
    active(core).then(|| current(core))
}

/// Mask IRQs on this core, returning the old DAIF to give to `restore`.
fn save_and_mask() -> u64 {
    let daif: u64;
    // SAFETY: masks IRQs; the old state is returned.
    unsafe { core::arch::asm!("mrs {}, DAIF", "msr DAIFSet, #2", out(reg) daif, options(nostack)) };
    daif
}
fn restore(daif: u64) {
    // SAFETY: puts back what `save_and_mask` returned.
    unsafe { core::arch::asm!("msr DAIF, {}", "isb", in(reg) daif, options(nostack)) };
}

// saiph_switch(from: *mut Context, to: *const Context): save this thread's
// registers and stack pointer, load the other's, and return into it.
// saiph_thread_start: where a new thread's first switch returns to (its
// context holds x19 = the thread).
core::arch::global_asm!(
    ".section .text.saiph_switch, \"ax\"",
    ".global saiph_switch",
    "saiph_switch:",
    "bti c",
    "stp x19, x20, [x0, #0]",
    "stp x21, x22, [x0, #16]",
    "stp x23, x24, [x0, #32]",
    "stp x25, x26, [x0, #48]",
    "stp x27, x28, [x0, #64]",
    "stp x29, x30, [x0, #80]",
    "mov x9, sp",
    "str x9, [x0, #96]",
    "ldp x19, x20, [x1, #0]",
    "ldp x21, x22, [x1, #16]",
    "ldp x23, x24, [x1, #32]",
    "ldp x25, x26, [x1, #48]",
    "ldp x27, x28, [x1, #64]",
    "ldp x29, x30, [x1, #80]",
    "ldr x9, [x1, #96]",
    "mov sp, x9",
    "ret",
    "",
    ".global saiph_thread_start",
    "saiph_thread_start:",
    "bti c",
    "mov x0, x19",
    "bl saiph_thread_main",
    "1: wfe",
    "b 1b",
);

unsafe extern "C" {
    fn saiph_switch(from: *mut Context, to: *const Context);
    fn saiph_thread_start();
}

/// A new thread's first code, on its own stack.
#[unsafe(no_mangle)]
extern "C" fn saiph_thread_main(thread: &'static Thread) -> ! {
    finish_switch();
    // SAFETY: threads run with interrupts on.
    unsafe { core::arch::asm!("msr DAIFClr, #2", options(nostack)) };
    (thread.entry)(thread.arg);
    schedule(Reason::Exit);
    unreachable!("a finished thread was switched back to");
}

/// Release this core's run-queue lock, held across the switch, and let the
/// thread switched away from run elsewhere.
fn finish_switch() {
    let core = smp::this_core();
    let prev = PREV[core].swap(null_mut(), Ordering::AcqRel);
    if !prev.is_null() {
        // SAFETY: threads are never freed.
        unsafe { (*prev).on_cpu.store(false, Ordering::Release) };
    }
    // SAFETY: the thread that switched to us took this lock on this core.
    unsafe { QUEUES[core].lock.raw_unlock() };
}

enum Reason {
    Yield,
    Preempt,
    Sleep(u64),
    Exit,
}

/// Set this core's timer for the next scheduling event: the running
/// thread's slice or budget end (only if another thread waits, or it has a
/// budget; not if a switch is due anyway, which sets the timer for the next
/// thread), a sleeper's wake-up, a budget refill.
fn program_timer(core: usize, inner: &Inner, running: ThreadRef, now: u64) {
    let policy = inner.policy.as_ref().unwrap();
    let mut deadline = u64::MAX;
    let switching = RESCHED[core].load(Ordering::Relaxed) == NOW;
    if !switching && !core::ptr::eq(running, idle(core)) && (policy.len() > 0 || policy.limited(running)) {
        deadline = now + policy.time_left(running).max(1);
    }
    for &(wake, _) in &inner.sleepers {
        deadline = deadline.min(wake);
    }
    if let Some(event) = policy.next_event() {
        deadline = deadline.min(event);
    }
    if deadline == u64::MAX { timer::disarm() } else { timer::arm(deadline) }
}

/// Give up this core: the running thread goes back in the queue, sleeps or
/// ends, and the policy's pick runs.
fn schedule(reason: Reason) {
    let core = smp::this_core();
    debug_assert!(irqs::depth() == 0, "schedule() with an IrqsOff lock held");
    let daif = save_and_mask();
    let mut q = QUEUES[core].lock();
    let now = timer::now();
    let running = current(core);
    let is_idle = core::ptr::eq(running, idle(core));
    let ran = now - q.charged;
    q.charged = now;
    RESCHED[core].store(0, Ordering::Relaxed);
    let inner = &mut *q;
    let policy = inner.policy.as_mut().unwrap();
    if !is_idle {
        running.runtime.fetch_add(ran, Ordering::Relaxed);
        policy.charge(running, ran, now);
        match reason {
            Reason::Yield | Reason::Preempt => {
                running.set_state(State::Runnable);
                policy.enqueue(running, now, Enqueue::Preempted);
            }
            Reason::Sleep(wake) => {
                running.set_state(State::Sleeping);
                inner.sleepers.push((wake, running));
            }
            Reason::Exit => running.set_state(State::Dead),
        }
    }
    let next = policy.pick(now).unwrap_or_else(|| idle(core));
    if core::ptr::eq(next, running) {
        running.set_state(State::Running);
        program_timer(core, inner, running, now);
        drop(q);
        restore(daif);
        return;
    }
    // A thread switched out on another core a moment ago may still be
    // saving its registers.
    while next.on_cpu() {
        core::hint::spin_loop();
    }
    next.on_cpu.store(true, Ordering::Relaxed);
    next.set_state(State::Running);
    next.cores.fetch_or(1 << core, Ordering::Relaxed);
    CURRENT[core].store(next as *const Thread as *mut Thread, Ordering::Release);
    PREV[core].store(running as *const Thread as *mut Thread, Ordering::Release);
    program_timer(core, inner, next, now);
    core::mem::forget(q); // the next thread releases the lock (finish_switch)
    // SAFETY: `running` is this thread and is on_cpu, so nobody else touches
    // its context; `next` is not on_cpu any more and now belongs to us.
    unsafe { saiph_switch(running.context.get(), next.context.get()) };
    // Back again, maybe on another core.
    finish_switch();
    restore(daif);
}

/// Let other threads of this core's queue run.
pub fn yield_now() {
    schedule(Reason::Yield);
}

/// Sleep until the counter reaches `deadline`.
pub fn sleep_until(deadline: u64) {
    match current_thread() {
        Some(t) if !core::ptr::eq(t, idle(smp::this_core())) => schedule(Reason::Sleep(deadline)),
        _ => timer::sleep_until(deadline),
    }
}

/// At the end of an interrupt: switch threads if this core should now.
pub fn preempt_if_needed() {
    let core = smp::this_core();
    if !active(core) || RESCHED[core].load(Ordering::Relaxed) != NOW {
        return;
    }
    // The interrupted code holds a lock: it goes on, and is preempted at
    // the next chance (preempting it could leave others spinning on it).
    if crate::sync::holding_locks() || irqs::depth() != 0 {
        return;
    }
    schedule(Reason::Preempt);
}

/// The timer interrupt (from timer.rs): charge the running thread, wake
/// sleepers, decide whether to switch, set the next deadline.
pub fn timer_fired() {
    let core = smp::this_core();
    if !active(core) {
        return;
    }
    let mut q = QUEUES[core].lock();
    let now = timer::now();
    let running = current(core);
    let is_idle = core::ptr::eq(running, idle(core));
    let inner = &mut *q;
    let policy = inner.policy.as_mut().unwrap();
    if !is_idle {
        let ran = now - inner.charged;
        running.runtime.fetch_add(ran, Ordering::Relaxed);
        if policy.charge(running, ran, now) {
            RESCHED[core].store(NOW, Ordering::Relaxed);
        }
    }
    inner.charged = now;
    let mut n = 0;
    while n < inner.sleepers.len() {
        if inner.sleepers[n].0 <= now {
            let (_, t) = inner.sleepers.swap_remove(n);
            t.set_state(State::Runnable);
            if is_idle || policy.preempts(t, running) {
                RESCHED[core].store(NOW, Ordering::Relaxed);
            }
            policy.enqueue(t, now, Enqueue::Wakeup);
        } else {
            n += 1;
        }
    }
    // Refill the budgets that are due here, so the timer is never set for
    // a refill time that has passed.
    let refilled_preempts = policy.refill_preempts(now, running);
    if (!is_idle && refilled_preempts) || (is_idle && policy.ready(now)) {
        RESCHED[core].store(NOW, Ordering::Relaxed);
    }
    program_timer(core, inner, running, now);
}

/// SGI RESCHEDULE: another core changed this core's queue.
fn reschedule_requested() {
    let core = smp::this_core();
    if active(core) {
        let q = QUEUES[core].lock();
        program_timer(core, &q, current(core), timer::now());
    }
}

/// Put `t` on `core`'s queue and tell that core.
fn enqueue_on(core: usize, t: ThreadRef, how: Enqueue) {
    let mut q = QUEUES[core].lock();
    let now = timer::now();
    let running = current(core);
    let inner = &mut *q;
    let policy = inner.policy.as_mut().unwrap();
    let urgent = core::ptr::eq(running, idle(core)) || policy.preempts(t, running);
    policy.enqueue(t, now, how);
    let flag = if urgent { NOW } else { LAZY };
    RESCHED[core].fetch_max(flag, Ordering::Relaxed);
    let busy = !urgent;
    if core == smp::this_core() {
        program_timer(core, inner, running, now);
        drop(q);
        if urgent {
            preempt_if_needed();
        }
    } else {
        drop(q);
        gic::send_sgi(smp::mpidr(core), RESCHEDULE);
    }
    // The core has work already: wake an idle core to come and steal.
    if busy {
        kick_idle_core(core);
    }
}

/// Send one idle core (not `except`) a reschedule interrupt; its idle loop
/// then looks for work to steal.
fn kick_idle_core(except: usize) {
    let active = ACTIVE.load(Ordering::Acquire);
    let me = smp::this_core();
    if let Some(c) = (0..MAX_CORES).find(|&c| c != except && c != me && active & 1 << c != 0 && core::ptr::eq(current(c), idle(c))) {
        gic::send_sgi(smp::mpidr(c), RESCHEDULE);
    }
}

fn new_thread(name: &'static str, entry: fn(usize), arg: usize, params: Params) -> &'static mut Thread {
    Box::leak(Box::new(Thread {
        context: UnsafeCell::new(Context { regs: [0; 12], sp: 0 }),
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        name,
        entry,
        arg,
        on_cpu: AtomicBool::new(false),
        state: AtomicU8::new(State::Runnable as u8),
        runtime: AtomicU64::new(0),
        cores: AtomicU64::new(0),
        entity: UnsafeCell::new(Entity::new(params)),
    }))
}

/// Give `t` a stack whose first switch "returns" into saiph_thread_start
/// with x19 = `t`.
fn give_stack(t: &mut Thread) -> Result<(), vm::Error> {
    let top = stacks::allocate_thread(t.id)?;
    let this = t as *const Thread as u64;
    let context = t.context.get_mut();
    context.sp = top;
    context.regs[0] = this;
    context.regs[11] = saiph_thread_start as *const () as u64;
    Ok(())
}

/// A thread for the code this core is running right now (no new stack).
fn adopt(name: &'static str, core: usize) -> ThreadRef {
    let t = new_thread(name, |_| {}, 0, Params::normal());
    t.on_cpu.store(true, Ordering::Relaxed);
    t.set_state(State::Running);
    t.cores.store(1 << core, Ordering::Relaxed);
    t
}

/// Start a thread running `entry(arg)` on `core`, or on the least busy core.
pub fn spawn(name: &'static str, entry: fn(usize), arg: usize, params: Params, core: Option<usize>) -> Result<ThreadRef, vm::Error> {
    let t = new_thread(name, entry, arg, params);
    give_stack(t)?;
    let t: ThreadRef = t;
    let core = core.unwrap_or_else(least_busy);
    enqueue_on(core, t, Enqueue::New);
    Ok(t)
}

fn least_busy() -> usize {
    let active = ACTIVE.load(Ordering::Acquire);
    (0..MAX_CORES)
        .filter(|&c| active & 1 << c != 0)
        .min_by_key(|&c| {
            let q = QUEUES[c].lock();
            q.policy.as_ref().map_or(0, |p| p.len()) + !core::ptr::eq(current(c), idle(c)) as usize
        })
        .unwrap_or(0)
}

/// What every core shares; before any core's scheduler starts.
pub fn init() {
    SLICE.store(timer::frequency() * SLICE_US / 1_000_000, Ordering::Relaxed);
    irq::set_handler(RESCHEDULE, reschedule_requested);
}

fn start(core: usize, current: ThreadRef, idle: ThreadRef) {
    {
        let mut q = QUEUES[core].lock();
        q.policy = Some(Box::new(McsEevdf::new(SLICE.load(Ordering::Relaxed))));
        q.charged = timer::now();
    }
    CURRENT[core].store(current as *const Thread as *mut Thread, Ordering::Release);
    IDLE[core].store(idle as *const Thread as *mut Thread, Ordering::Release);
    gic::enable(RESCHEDULE);
    ACTIVE.fetch_or(1 << core, Ordering::AcqRel);
}

/// The boot core: the boot code becomes a thread, with an idle thread
/// beside it for when it sleeps.
pub fn start_boot_core() -> Result<(), vm::Error> {
    let core = smp::this_core();
    let boot = adopt("boot", core);
    let idle = new_thread("idle", |_| idle_loop(), 0, Params::normal());
    give_stack(idle)?;
    start(core, boot, idle);
    Ok(())
}

/// Another core: the code it runs now becomes its idle thread. Never returns.
pub fn start_other_core() -> ! {
    let core = smp::this_core();
    let idle = adopt("idle", core);
    start(core, idle, idle);
    idle_loop()
}

/// Run what this core's queue has, steal from busy cores, else sleep.
fn idle_loop() -> ! {
    let core = smp::this_core();
    loop {
        let ready = QUEUES[core].lock().policy.as_ref().is_some_and(|p| p.ready(timer::now()));
        if ready || steal(core) {
            yield_now();
            continue;
        }
        // Masked around the check, so a wake-up cannot slip in before wfi.
        let daif = save_and_mask();
        if !QUEUES[core].lock().policy.as_ref().is_some_and(|p| p.ready(timer::now())) {
            // SAFETY: waits for an interrupt; changes nothing.
            unsafe { core::arch::asm!("wfi", options(nostack)) };
        }
        restore(daif);
    }
}

/// Take a waiting thread from a core that is busy running another one.
fn steal(core: usize) -> bool {
    let active = ACTIVE.load(Ordering::Acquire);
    let now = timer::now();
    for n in 1..MAX_CORES {
        let victim = (core + n) % MAX_CORES;
        if active & 1 << victim == 0 || core::ptr::eq(current(victim), idle(victim)) {
            continue;
        }
        let stolen = QUEUES[victim].lock().policy.as_mut().and_then(|p| p.steal(now));
        if let Some(t) = stolen {
            let mut q = QUEUES[core].lock();
            q.policy.as_mut().unwrap().enqueue(t, now, Enqueue::Migrated);
            return true;
        }
    }
    false
}

// The self-test.

/// Spin until the counter reaches `arg`.
fn busy_until(end: usize) {
    while timer::now() < end as u64 {
        core::hint::spin_loop();
    }
}

/// Wait (sleeping) until every thread in `threads` has finished, at most
/// `limit` ticks. Prints the ones that did not.
fn wait_for(out: &mut impl Write, threads: &[ThreadRef], limit: u64) -> bool {
    let f = timer::frequency();
    let give_up = timer::now() + limit;
    while !threads.iter().all(|t| t.finished()) && timer::now() < give_up {
        sleep_until(timer::now() + f / 100);
    }
    for t in threads.iter().filter(|t| !t.finished()) {
        let _ = writeln!(out, "    thread {} ({}) did not finish: state {}, ran {} us, on cores {:#x}, on_cpu {}",
            t.id, t.name, t.state.load(Ordering::Relaxed), t.runtime() * 1_000_000 / f, t.cores.load(Ordering::Relaxed), t.on_cpu());
    }
    threads.iter().all(|t| t.finished())
}

/// Run threads and check the policy and the stealing. First, at once: on
/// every core but the last, a thread of weight 2048 against one of 1024
/// (EEVDF should give them 2:1); on the last core, a high-priority thread
/// with a 2 ms per 10 ms budget against a normal one (it should get about
/// 20%, not everything). Then threads all queued on this core, which the
/// idle cores should steal.
pub fn self_test(out: &mut impl Write) -> bool {
    let f = timer::frequency();
    let cores: Vec<usize> = (0..MAX_CORES).filter(|&c| active(c)).collect();
    let pinned = Params { pinned: true, ..Params::normal() };
    let end = (timer::now() + f * 3 / 10) as usize; // 300 ms
    let (budget_core, pair_cores) = cores.split_last().unwrap();
    let mut pairs = Vec::new();
    for &c in pair_cores {
        if let (Ok(h), Ok(l)) = (spawn("heavy", busy_until, end, Params { weight: 2048, ..pinned }, Some(c)), spawn("light", busy_until, end, pinned, Some(c))) {
            pairs.push((h, l));
        }
    }
    let limited = spawn("limited", busy_until, end, Params { priority: 1, budget: f / 500, period: f / 100, ..pinned }, Some(*budget_core));
    let rival = spawn("rival", busy_until, end, pinned, Some(*budget_core));
    let (Ok(limited), Ok(rival)) = (limited, rival) else {
        let _ = writeln!(out, "  scheduler self-test: could not start its threads; FAILED");
        return false;
    };
    let mut first: Vec<ThreadRef> = pairs.iter().flat_map(|&(h, l)| [h, l]).collect();
    first.extend([limited, rival]);
    if !wait_for(out, &first, 3 * f) {
        let _ = writeln!(out, "  scheduler self-test: FAILED");
        return false;
    }

    // Stealing: the other cores are idle now.
    let steal_end = (timer::now() + f / 20) as usize; // 50 ms
    let stealable: Vec<ThreadRef> = (0..3 * cores.len())
        .filter_map(|_| spawn("stealable", busy_until, steal_end, Params::normal(), Some(smp::this_core())).ok())
        .collect();
    if !wait_for(out, &stealable, 3 * f) {
        let _ = writeln!(out, "  scheduler self-test: FAILED");
        return false;
    }

    let ratios: Vec<u64> = pairs.iter().map(|(h, l)| h.runtime() * 100 / l.runtime().max(1)).collect();
    let (min, max) = (*ratios.iter().min().unwrap_or(&200), *ratios.iter().max().unwrap_or(&200));
    let fair = min >= 160 && max <= 250;
    let share = limited.runtime() * 1000 / (limited.runtime() + rival.runtime()).max(1);
    // 20% on real cores (20.2% under KVM); QEMU's emulated timer fires a
    // little late, which lets the budget overrun by a few percent.
    let budget_ok = (150..=300).contains(&share);
    let spread = stealable.iter().fold(0u64, |c, t| c | t.cores.load(Ordering::Relaxed)).count_ones();
    let stolen = spread as usize >= cores.len().min(2);
    let _ = writeln!(out, "  scheduler: {} cores; weight 2048 vs 1024 got {}.{:02}x to {}.{:02}x the time; a 2 ms per 10 ms budget at high priority got {}.{}% of its core; {} threads queued on core {} ran on {} cores",
        cores.len(), min / 100, min % 100, max / 100, max % 100, share / 10, share % 10, stealable.len(), smp::this_core(), spread);
    let _ = writeln!(out, "  scheduler self-test: {}", if fair && budget_ok && stolen { "ok" } else { "FAILED" });
    fair && budget_ok && stolen
}
