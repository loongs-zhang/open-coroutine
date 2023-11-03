use crate::common::{Current, Named};
use crate::constants::{CoroutineState, SyscallState};
use crate::coroutine::suspender::{Suspender, SuspenderImpl};
use crate::coroutine::{Coroutine, CoroutineImpl, SimpleCoroutine, StateCoroutine};
use crate::scheduler::listener::Listener;
use dashmap::DashMap;
use open_coroutine_queue::LocalQueue;
use open_coroutine_timer::TimerList;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fmt::Debug;
use std::io::{Error, ErrorKind};
use std::panic::UnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A type for Scheduler.
pub type SchedulableCoroutine<'s> = CoroutineImpl<'s, (), (), ()>;

/// A type for Scheduler.
pub type SchedulableSuspender<'s> = SuspenderImpl<'s, (), ()>;

/// Listener abstraction and impl.
pub mod listener;

#[cfg(test)]
mod tests;

/// A trait implemented for schedulers.
pub trait Scheduler<'s>: Debug + Default + Named + Current<'s> + Listener {
    /// Extension points within the open-coroutine framework.
    fn init(&mut self);

    /// Set the default stack stack size for the coroutines in this scheduler.
    /// If it has not been set, it will be `crate::constant::DEFAULT_STACK_SIZE`.
    fn set_stack_size(&self, stack_size: usize);

    /// Submit a closure to create new coroutine, then the coroutine will be push into ready queue.
    ///
    /// Allow multiple threads to concurrently submit coroutine to the scheduler,
    /// but only allow one thread to execute scheduling.
    ///
    /// # Errors
    /// if create coroutine fails.
    fn submit_coroutine(
        &self,
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) + UnwindSafe + 's,
        stack_size: Option<usize>,
    ) -> std::io::Result<()>;

    /// Resume a coroutine from the system call table to the ready queue,
    /// it's generally only required for framework level crates.
    ///
    /// If we can't find the coroutine, nothing happens.
    ///
    /// # Errors
    /// if change to ready fails.
    fn try_resume(&self, co_name: &'s str) -> std::io::Result<()>;

    /// Schedule the coroutines.
    ///
    /// Allow multiple threads to concurrently submit coroutine to the scheduler,
    /// but only allow one thread to execute scheduling.
    ///
    /// # Errors
    /// see `try_timeout_schedule`.
    fn try_schedule(&mut self) -> std::io::Result<()> {
        _ = self.try_timeout_schedule(std::time::Duration::MAX.as_secs())?;
        Ok(())
    }

    /// Try scheduling the coroutines for up to `dur`.
    ///
    /// Allow multiple threads to concurrently submit coroutine to the scheduler,
    /// but only allow one thread to execute scheduling.
    ///
    /// # Errors
    /// see `try_timeout_schedule`.
    fn try_timed_schedule(&mut self, dur: std::time::Duration) -> std::io::Result<u64> {
        self.try_timeout_schedule(open_coroutine_timer::get_timeout_time(dur))
    }

    /// Attempt to schedule the coroutines before the `timeout_time` timestamp.
    ///
    /// Allow multiple threads to concurrently submit coroutine to the scheduler,
    /// but only allow one thread to execute scheduling.
    ///
    /// Returns the left time in ns.
    ///
    /// # Errors
    /// if change to ready fails.
    fn try_timeout_schedule(&mut self, timeout_time: u64) -> std::io::Result<u64>;

    /// Returns `true` if the ready queue, suspend queue, and syscall queue are all empty.
    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Returns the number of coroutines owned by this scheduler.
    fn size(&self) -> usize;

    /// Add a listener to this scheduler.
    fn add_listener(&mut self, listener: impl Listener + 's);
}

#[allow(missing_docs, clippy::missing_errors_doc)]
pub trait HasScheduler<'s> {
    fn scheduler(&self) -> &SchedulerImpl<'s>;

    fn scheduler_mut(&mut self) -> &mut SchedulerImpl<'s>;

    fn set_stack_size(&self, stack_size: usize) {
        self.scheduler().set_stack_size(stack_size);
    }

    fn submit_coroutine(
        &self,
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) + UnwindSafe + 's,
        stack_size: Option<usize>,
    ) -> std::io::Result<()> {
        self.scheduler().submit_coroutine(f, stack_size)
    }

    fn try_resume(&self, co_name: &'s str) -> std::io::Result<()> {
        self.scheduler().try_resume(co_name)
    }

    fn try_schedule(&mut self) -> std::io::Result<()> {
        _ = self.try_timeout_schedule(std::time::Duration::MAX.as_secs())?;
        Ok(())
    }

    fn try_timed_schedule(&mut self, dur: std::time::Duration) -> std::io::Result<u64> {
        self.try_timeout_schedule(open_coroutine_timer::get_timeout_time(dur))
    }

    fn try_timeout_schedule(&mut self, timeout_time: u64) -> std::io::Result<u64> {
        self.scheduler_mut().try_timeout_schedule(timeout_time)
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    fn size(&self) -> usize {
        self.scheduler().size()
    }

    fn add_listener(&mut self, listener: impl Listener + 's) {
        self.scheduler_mut().add_listener(listener);
    }
}

#[allow(missing_docs, box_pointers)]
#[derive(Debug)]
pub struct SchedulerImpl<'s> {
    name: String,
    stack_size: AtomicUsize,
    ready: LocalQueue<'s, SchedulableCoroutine<'s>>,
    suspend: TimerList<SchedulableCoroutine<'s>>,
    syscall: DashMap<&'s str, SchedulableCoroutine<'s>>,
    syscall_suspend: TimerList<&'s str>,
    listeners: VecDeque<Box<dyn Listener + 's>>,
}

impl SchedulerImpl<'_> {
    #[allow(missing_docs, box_pointers)]
    #[must_use]
    pub fn new(name: String, stack_size: usize) -> Self {
        let mut scheduler = SchedulerImpl {
            name,
            stack_size: AtomicUsize::new(stack_size),
            ready: LocalQueue::default(),
            suspend: TimerList::default(),
            syscall: DashMap::default(),
            syscall_suspend: TimerList::default(),
            listeners: VecDeque::default(),
        };
        scheduler.init();
        scheduler
    }

    fn check_ready(&mut self) -> std::io::Result<()> {
        // Check if the elements in the suspend queue are ready
        for _ in 0..self.suspend.entry_len() {
            if let Some((exec_time, _)) = self.suspend.front() {
                if open_coroutine_timer::now() < *exec_time {
                    break;
                }
                if let Some((_, mut entry)) = self.suspend.pop_front() {
                    while !entry.is_empty() {
                        if let Some(coroutine) = entry.pop_front() {
                            if let Err(e) = coroutine.ready() {
                                Self::clean_current();
                                return Err(e);
                            }
                            self.ready.push_back(coroutine);
                        }
                    }
                }
            }
        }
        // Check if the elements in the syscall suspend queue are ready
        for _ in 0..self.syscall_suspend.entry_len() {
            if let Some((exec_time, _)) = self.syscall_suspend.front() {
                if open_coroutine_timer::now() < *exec_time {
                    break;
                }
                if let Some((_, mut entry)) = self.syscall_suspend.pop_front() {
                    while !entry.is_empty() {
                        if let Some(co_name) = entry.pop_front() {
                            if let Some(r) = self.syscall.remove(&co_name) {
                                let coroutine = r.1;
                                match coroutine.state() {
                                    CoroutineState::SystemCall(val, syscall, state) => {
                                        if let SyscallState::Suspend(_) = state {
                                            if let Err(e) = coroutine.syscall(
                                                val,
                                                syscall,
                                                SyscallState::Timeout,
                                            ) {
                                                Self::clean_current();
                                                return Err(e);
                                            }
                                        }
                                        self.ready.push_back(coroutine);
                                    }
                                    _ => unreachable!("check_ready should never execute to here"),
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl Default for SchedulerImpl<'_> {
    fn default() -> Self {
        Self::new(
            format!("open-coroutine-scheduler-{}", uuid::Uuid::new_v4()),
            crate::constants::DEFAULT_STACK_SIZE,
        )
    }
}

impl Drop for SchedulerImpl<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                self.ready.is_empty(),
                "There are still coroutines to be carried out in the ready queue:{:#?} !",
                self.ready
            );
            assert!(
                self.suspend.is_empty(),
                "There are still coroutines to be carried out in the suspend queue:{:#?} !",
                self.suspend
            );
            assert!(
                self.syscall.is_empty(),
                "There are still coroutines to be carried out in the syscall queue:{:#?} !",
                self.syscall
            );
        }
    }
}

impl Eq for SchedulerImpl<'_> {}

impl PartialEq for SchedulerImpl<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl Named for SchedulerImpl<'_> {
    fn get_name(&self) -> &str {
        &self.name
    }
}

thread_local! {
    static SCHEDULER: std::cell::RefCell<VecDeque<*const c_void>> =
        std::cell::RefCell::new(VecDeque::new());
}

impl<'s> Current<'s> for SchedulerImpl<'s> {
    #[allow(clippy::ptr_as_ptr)]
    fn init_current(current: &Self)
    where
        Self: Sized,
    {
        SCHEDULER.with(|s| {
            s.borrow_mut()
                .push_front(current as *const _ as *const c_void);
        });
    }

    fn current() -> Option<&'s Self>
    where
        Self: Sized,
    {
        SCHEDULER.with(|s| {
            s.borrow()
                .front()
                .map(|ptr| unsafe { &*(*ptr).cast::<SchedulerImpl<'s>>() })
        })
    }

    fn clean_current()
    where
        Self: Sized,
    {
        SCHEDULER.with(|s| _ = s.borrow_mut().pop_front());
    }
}

impl<'s> Scheduler<'s> for SchedulerImpl<'s> {
    fn init(&mut self) {
        #[cfg(all(unix, feature = "preemptive-schedule"))]
        self.add_listener(crate::monitor::creator::MonitorTaskCreator::default());
    }

    fn set_stack_size(&self, stack_size: usize) {
        self.stack_size.store(stack_size, Ordering::Release);
    }

    fn submit_coroutine(
        &self,
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) + UnwindSafe + 's,
        stack_size: Option<usize>,
    ) -> std::io::Result<()> {
        let coroutine = SchedulableCoroutine::new(
            format!("{}|{}", self.name, uuid::Uuid::new_v4()),
            f,
            stack_size.unwrap_or(self.stack_size.load(Ordering::Acquire)),
        )?;
        coroutine.ready()?;
        self.on_create(&coroutine);
        self.ready.push_back(coroutine);
        Ok(())
    }

    fn try_resume(&self, co_name: &'s str) -> std::io::Result<()> {
        if let Some(r) = self.syscall.remove(&co_name) {
            let coroutine = r.1;
            match coroutine.state() {
                CoroutineState::SystemCall(val, syscall, _) => {
                    coroutine.syscall(val, syscall, SyscallState::Executing)?;
                }
                _ => unreachable!("try_resume should never execute to here"),
            }
            self.ready.push_back(coroutine);
        }
        Ok(())
    }

    fn try_timeout_schedule(&mut self, timeout_time: u64) -> std::io::Result<u64> {
        Self::init_current(self);
        loop {
            let left_time = timeout_time.saturating_sub(open_coroutine_timer::now());
            if left_time == 0 {
                Self::clean_current();
                return Ok(0);
            }
            self.check_ready()?;
            // schedule coroutines
            match self.ready.pop_front() {
                None => {
                    Self::clean_current();
                    return Ok(left_time);
                }
                Some(mut coroutine) => {
                    self.on_resume(timeout_time, &coroutine);
                    match coroutine.resume() {
                        Ok(state) => {
                            match state {
                                CoroutineState::Suspend((), timestamp) => {
                                    self.on_suspend(timeout_time, &coroutine);
                                    if timestamp <= open_coroutine_timer::now() {
                                        self.ready.push_back(coroutine);
                                    } else {
                                        self.suspend.insert(timestamp, coroutine);
                                    }
                                }
                                CoroutineState::SystemCall((), syscall, state) => {
                                    self.on_syscall(timeout_time, &coroutine, syscall, state);
                                    #[allow(box_pointers)]
                                    let co_name = Box::leak(Box::from(coroutine.get_name()));
                                    if let SyscallState::Suspend(timestamp) = state {
                                        self.syscall_suspend.insert(timestamp, co_name);
                                    }
                                    _ = self.syscall.insert(co_name, coroutine);
                                }
                                CoroutineState::Complete(()) => {
                                    self.on_complete(timeout_time, &coroutine);
                                }
                                CoroutineState::Error(message) => {
                                    self.on_error(timeout_time, &coroutine, message);
                                }
                                _ => {
                                    Self::clean_current();
                                    return Err(Error::new(
                                        ErrorKind::Other,
                                        "try_timeout_schedule should never execute to here",
                                    ));
                                }
                            };
                        }
                        Err(e) => {
                            Self::clean_current();
                            return Err(e);
                        }
                    };
                }
            }
        }
    }

    fn size(&self) -> usize {
        self.ready.len() + self.suspend.len() + self.syscall.len()
    }

    #[allow(box_pointers)]
    fn add_listener(&mut self, listener: impl Listener + 's) {
        self.listeners.push_back(Box::new(listener));
    }
}
