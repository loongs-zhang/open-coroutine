use crate::common::{Current, JoinHandle, Named};
use crate::constants::{CoroutineState, SyscallState};
use crate::coroutine::suspender::{Suspender, SuspenderImpl};
use crate::coroutine::{Coroutine, CoroutineImpl, SimpleCoroutine, StateCoroutine};
use crate::scheduler::join::JoinHandleImpl;
use crate::scheduler::listener::Listener;
use dashmap::DashMap;
use open_coroutine_queue::LocalQueue;
use open_coroutine_timer::TimerList;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fmt::Debug;
use std::io::{Error, ErrorKind};
use std::panic::UnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A type for Scheduler.
pub type SchedulableCoroutine<'s> = CoroutineImpl<'s, (), (), Option<usize>>;

/// A type for Scheduler.
pub type SchedulableSuspender<'s> = SuspenderImpl<'s, (), ()>;

/// Listener abstraction and impl.
pub mod listener;

/// Join impl for scheduler.
pub mod join;

/// Has scheduler abstraction.
pub mod has;

#[cfg(test)]
mod tests;

/// A trait implemented for schedulers.
pub trait Scheduler<'s, Join: JoinHandle>:
    Debug + Default + Named + Current<'s> + Listener
{
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
    fn submit_co(
        &self,
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) -> Option<usize> + UnwindSafe + 's,
        stack_size: Option<usize>,
    ) -> std::io::Result<Join>;

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
    fn try_schedule(&self) -> std::io::Result<()> {
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
    fn try_timed_schedule(&self, dur: std::time::Duration) -> std::io::Result<u64> {
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
    fn try_timeout_schedule(&self, timeout_time: u64) -> std::io::Result<u64>;

    /// Attempt to obtain coroutine result with the given `co_name`.
    fn try_get_coroutine_result(&self, co_name: &str) -> Option<Result<Option<usize>, &str>>;

    /// Returns `true` if the ready queue, suspend queue, and syscall queue are all empty.
    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Returns the number of coroutines owned by this scheduler.
    fn size(&self) -> usize;

    /// Add a listener to this scheduler.
    fn add_listener(&mut self, listener: impl Listener + 's);
}

#[allow(missing_docs, box_pointers)]
#[derive(Debug)]
pub struct SchedulerImpl<'s> {
    name: String,
    scheduling: AtomicBool,
    stack_size: AtomicUsize,
    ready: LocalQueue<'s, SchedulableCoroutine<'s>>,
    suspend: RefCell<TimerList<SchedulableCoroutine<'s>>>,
    syscall: DashMap<&'s str, SchedulableCoroutine<'s>>,
    syscall_suspend: RefCell<TimerList<&'s str>>,
    results: DashMap<&'s str, Result<Option<usize>, &'static str>>,
    listeners: VecDeque<Box<dyn Listener + 's>>,
}

impl SchedulerImpl<'_> {
    #[allow(missing_docs, box_pointers)]
    #[must_use]
    pub fn new(name: String, stack_size: usize) -> Self {
        let mut scheduler = SchedulerImpl {
            name,
            scheduling: AtomicBool::new(false),
            stack_size: AtomicUsize::new(stack_size),
            ready: LocalQueue::default(),
            suspend: RefCell::default(),
            syscall: DashMap::default(),
            syscall_suspend: RefCell::default(),
            results: DashMap::default(),
            listeners: VecDeque::default(),
        };
        scheduler.init();
        scheduler
    }

    fn check_ready(&self) -> std::io::Result<()> {
        // Check if the elements in the suspend queue are ready
        loop {
            if let Ok(mut suspend) = self.suspend.try_borrow_mut() {
                for _ in 0..suspend.entry_len() {
                    if let Some((exec_time, _)) = suspend.front() {
                        if open_coroutine_timer::now() < *exec_time {
                            break;
                        }
                        if let Some((_, mut entry)) = suspend.pop_front() {
                            while let Some(coroutine) = entry.pop_front() {
                                coroutine.ready()?;
                                self.ready.push_back(coroutine);
                            }
                        }
                    }
                }
                break;
            }
        }
        // Check if the elements in the syscall suspend queue are ready
        loop {
            if let Ok(mut syscall_suspend) = self.syscall_suspend.try_borrow_mut() {
                for _ in 0..syscall_suspend.entry_len() {
                    if let Some((exec_time, _)) = syscall_suspend.front() {
                        if open_coroutine_timer::now() < *exec_time {
                            break;
                        }
                        if let Some((_, mut entry)) = syscall_suspend.pop_front() {
                            while let Some(co_name) = entry.pop_front() {
                                if let Some((_, coroutine)) = self.syscall.remove(&co_name) {
                                    match coroutine.state() {
                                        CoroutineState::SystemCall(val, syscall, state) => {
                                            if let SyscallState::Suspend(_) = state {
                                                coroutine.syscall(
                                                    val,
                                                    syscall,
                                                    SyscallState::Timeout,
                                                )?;
                                            }
                                            self.ready.push_back(coroutine);
                                        }
                                        _ => {
                                            unreachable!("check_ready should never execute to here")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                break;
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
                self.suspend.get_mut().is_empty(),
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
    static SCHEDULER: RefCell<VecDeque<*const c_void>> = RefCell::new(VecDeque::new());
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

impl<'s> Scheduler<'s, JoinHandleImpl<'s>> for SchedulerImpl<'s> {
    fn init(&mut self) {
        #[cfg(all(unix, feature = "preemptive-schedule"))]
        self.add_listener(crate::monitor::creator::MonitorTaskCreator::default());
    }

    fn set_stack_size(&self, stack_size: usize) {
        self.stack_size.store(stack_size, Ordering::Release);
    }

    fn submit_co(
        &self,
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) -> Option<usize> + UnwindSafe + 's,
        stack_size: Option<usize>,
    ) -> std::io::Result<JoinHandleImpl<'s>> {
        let co_name = format!("{}|{}", self.name, uuid::Uuid::new_v4());
        let coroutine = SchedulableCoroutine::new(
            co_name.clone(),
            f,
            stack_size.unwrap_or(self.stack_size.load(Ordering::Acquire)),
        )?;
        coroutine.ready()?;
        self.on_create(&coroutine);
        self.ready.push_back(coroutine);
        Ok(JoinHandleImpl::new(self, &co_name))
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

    fn try_timeout_schedule(&self, timeout_time: u64) -> std::io::Result<u64> {
        if self
            .scheduling
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(timeout_time.saturating_sub(open_coroutine_timer::now()));
        }
        Self::init_current(self);
        loop {
            let left_time = timeout_time.saturating_sub(open_coroutine_timer::now());
            if left_time == 0 {
                Self::clean_current();
                self.scheduling.store(false, Ordering::Release);
                return Ok(0);
            }
            if let Err(e) = self.check_ready() {
                Self::clean_current();
                self.scheduling.store(false, Ordering::Release);
                return Err(e);
            }
            // schedule coroutines
            match self.ready.pop_front() {
                None => {
                    Self::clean_current();
                    self.scheduling.store(false, Ordering::Release);
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
                                        loop {
                                            if let Ok(mut suspend) = self.suspend.try_borrow_mut() {
                                                suspend.insert(timestamp, coroutine);
                                                break;
                                            }
                                        }
                                    }
                                }
                                CoroutineState::SystemCall((), syscall, state) => {
                                    self.on_syscall(timeout_time, &coroutine, syscall, state);
                                    #[allow(box_pointers)]
                                    let co_name = Box::leak(Box::from(coroutine.get_name()));
                                    if let SyscallState::Suspend(timestamp) = state {
                                        loop {
                                            if let Ok(mut syscall_suspend) =
                                                self.syscall_suspend.try_borrow_mut()
                                            {
                                                syscall_suspend.insert(timestamp, co_name);
                                                break;
                                            }
                                        }
                                    }
                                    _ = self.syscall.insert(co_name, coroutine);
                                }
                                CoroutineState::Complete(result) => {
                                    self.on_complete(timeout_time, &coroutine, result);
                                    #[allow(box_pointers)]
                                    let co_name = Box::leak(Box::from(coroutine.get_name()));
                                    assert!(
                                        self.results.insert(co_name, Ok(result)).is_none(),
                                        "not consume result"
                                    );
                                }
                                CoroutineState::Error(message) => {
                                    self.on_error(timeout_time, &coroutine, message);
                                    #[allow(box_pointers)]
                                    let co_name = Box::leak(Box::from(coroutine.get_name()));
                                    assert!(
                                        self.results.insert(co_name, Err(message)).is_none(),
                                        "not consume result"
                                    );
                                }
                                _ => {
                                    Self::clean_current();
                                    self.scheduling.store(false, Ordering::Release);
                                    return Err(Error::new(
                                        ErrorKind::Other,
                                        "try_timeout_schedule should never execute to here",
                                    ));
                                }
                            };
                        }
                        Err(e) => {
                            Self::clean_current();
                            self.scheduling.store(false, Ordering::Release);
                            return Err(e);
                        }
                    };
                }
            }
        }
    }

    fn try_get_coroutine_result(&self, co_name: &str) -> Option<Result<Option<usize>, &str>> {
        self.results.remove(co_name).map(|r| r.1)
    }

    fn size(&self) -> usize {
        loop {
            if let Ok(suspend) = self.suspend.try_borrow() {
                return self.ready.len() + suspend.len() + self.syscall.len();
            }
        }
    }

    #[allow(box_pointers)]
    fn add_listener(&mut self, listener: impl Listener + 's) {
        self.listeners.push_back(Box::new(listener));
    }
}
