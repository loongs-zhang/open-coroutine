use crate::blocker::Blocker;
use crate::coroutine::suspender::SimpleSuspender;
use crate::coroutine::{Current, Named};
use crate::pool::constants::PoolState;
use crate::pool::creator::CoroutineCreator;
use crate::pool::join::{JoinHandle, JoinHandleImpl};
use crate::pool::task::{Task, TaskImpl};
use crate::scheduler::{SchedulableCoroutine, Scheduler, SchedulerImpl};
use crossbeam_deque::{Injector, Steal};
use dashmap::DashMap;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fmt::Debug;
use std::io::{Error, ErrorKind};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Constants.
pub mod constants;

/// Task abstraction and impl.
pub mod task;

/// Task join abstraction and impl.
pub mod join;

mod creator;

/// The `Pool` abstraction.
pub trait Pool<'p, Join: JoinHandle>: Debug + Default + RefUnwindSafe + Named {
    /// Get the state of this pool.
    fn get_state(&self) -> PoolState;

    /// Change the state of this pool.
    fn change_state(&self, state: PoolState) -> PoolState;

    /// Set the minimum number of coroutines to run in this pool.
    fn set_min_size(&self, min_size: usize);

    /// Get the minimum number of coroutines to run in this pool.
    fn get_min_size(&self) -> usize;

    /// Gets the number of coroutines currently running in this pool.
    fn get_running_size(&self) -> usize;

    /// Set the maximum number of coroutines to run in this pool.
    fn set_max_size(&self, max_size: usize);

    /// Get the maximum number of coroutines to run in this pool.
    fn get_max_size(&self) -> usize;

    /// Set the maximum idle time for coroutines running in this pool.
    /// `keep_alive_time` has `ns` units.
    fn set_keep_alive_time(&self, keep_alive_time: u64);

    /// Get the maximum idle time for coroutines running in this pool.
    /// Returns in `ns` units.
    fn get_keep_alive_time(&self) -> u64;

    /// Returns `true` if the task queue is empty.
    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Returns the number of tasks owned by this pool.
    fn size(&self) -> usize;

    /// Submit a new task to this pool and wait for the task to complete.
    ///
    /// # Errors
    /// see `wait_result`
    #[allow(clippy::type_complexity)]
    fn submit_and_wait(
        &self,
        name: Option<String>,
        func: impl FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 'p,
        param: Option<usize>,
        wait_time: Duration,
    ) -> std::io::Result<Option<(String, Result<Option<usize>, &str>)>> {
        let join = self.submit(name, func, param);
        self.wait_result(join.get_name()?, wait_time)
    }

    /// Use the given `task_name` to obtain task results, and if no results are found,
    /// block the current thread for `wait_time`.
    ///
    /// # Errors
    /// if timeout
    #[allow(clippy::type_complexity)]
    fn wait_result(
        &self,
        task_name: &str,
        wait_time: Duration,
    ) -> std::io::Result<Option<(String, Result<Option<usize>, &str>)>>;

    /// Submit a new task to this pool.
    ///
    /// Allow multiple threads to concurrently submit task to the pool,
    /// but only allow one thread to execute scheduling.
    fn submit(
        &self,
        name: Option<String>,
        func: impl FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 'p,
        param: Option<usize>,
    ) -> Join {
        self.submit_raw(TaskImpl::new(
            name.unwrap_or(format!("{}|{}", self.get_name(), uuid::Uuid::new_v4())),
            func,
            param,
        ))
    }

    /// Submit new task to this pool.
    ///
    /// Allow multiple threads to concurrently submit task to the pool,
    /// but only allow one thread to execute scheduling.
    fn submit_raw(&self, task: TaskImpl<'p>) -> Join;

    /// pop a task
    fn pop(&self) -> Option<TaskImpl<'p>>;

    /// Change the blocker in this pool.
    fn change_blocker(&self, blocker: impl Blocker + 'p) -> Box<dyn Blocker>
    where
        'p: 'static;

    /// Start an additional thread to consume tasks.
    ///
    /// # Errors
    /// if create the additional thread failed.
    fn start(self) -> std::io::Result<Arc<Self>>
    where
        'p: 'static;

    /// Stop this pool.
    ///
    /// # Errors
    /// if timeout.
    fn stop(&self, wait_time: Duration) -> std::io::Result<()>;
}

/// The `CoroutinePool` abstraction.
pub trait CoroutinePool<'p>: Current<'p> + Pool<'p, JoinHandleImpl<'p>> {
    /// Create a new `CoroutinePool` instance.
    fn new(
        name: String,
        cpu: usize,
        stack_size: usize,
        min_size: usize,
        max_size: usize,
        keep_alive_time: u64,
        blocker: impl Blocker + 'p,
    ) -> Self
    where
        Self: Sized;

    /// Extension points within the open-coroutine framework.
    fn init(&mut self);

    /// Set the default stack stack size for the coroutines in this pool.
    /// If it has not been set, it will be `crate::coroutine::DEFAULT_STACK_SIZE`.
    fn set_stack_size(&self, stack_size: usize);

    /// Resume a coroutine from the system call table to the ready queue,
    /// it's generally only required for framework level crates.
    ///
    /// If we can't find the coroutine, nothing happens.
    ///
    /// # Errors
    /// if change to ready fails.
    fn try_resume(&self, co_name: &'p str) -> std::io::Result<()>;

    /// Attempt to run a task in current coroutine or thread.
    fn try_run(&self) -> Option<()>;

    /// Create a coroutine in this pool.
    ///
    /// # Errors
    /// if create failed.
    fn grow(&self, should_grow: bool) -> std::io::Result<()>;

    /// Schedule the tasks.
    ///
    /// Allow multiple threads to concurrently submit task to the pool,
    /// but only allow one thread to execute scheduling.
    ///
    /// # Errors
    /// see `try_timeout_schedule`.
    fn try_schedule(&self) -> std::io::Result<()> {
        _ = self.try_timeout_schedule(Duration::MAX.as_secs())?;
        Ok(())
    }

    /// Try scheduling the tasks for up to `dur`.
    ///
    /// Allow multiple threads to concurrently submit task to the scheduler,
    /// but only allow one thread to execute scheduling.
    ///
    /// # Errors
    /// see `try_timeout_schedule`.
    fn try_timed_schedule(&self, dur: Duration) -> std::io::Result<u64> {
        self.try_timeout_schedule(open_coroutine_timer::get_timeout_time(dur))
    }

    /// Attempt to schedule the tasks before the `timeout_time` timestamp.
    ///
    /// Allow multiple threads to concurrently submit task to the scheduler,
    /// but only allow one thread to execute scheduling.
    ///
    /// Returns the left time in ns.
    ///
    /// # Errors
    /// if change to ready fails.
    fn try_timeout_schedule(&self, timeout_time: u64) -> std::io::Result<u64>;

    /// Attempt to obtain task results with the given `task_name`.
    fn try_get_result(&self, task_name: &str) -> Option<(String, Result<Option<usize>, &str>)>;
}

#[allow(missing_docs, box_pointers, dead_code)]
#[derive(Debug)]
pub struct CoroutinePoolImpl<'p> {
    //绑定到哪个CPU核心
    cpu: usize,
    //协程池状态
    state: Cell<PoolState>,
    //任务队列
    task_queue: Injector<TaskImpl<'p>>,
    //工作协程组
    workers: UnsafeCell<SchedulerImpl<'p>>,
    //是否正在调度，不允许多线程并行调度
    scheduling: AtomicBool,
    //当前协程数
    running: AtomicUsize,
    //尝试取出任务失败的次数
    pop_fail_times: AtomicUsize,
    //最小协程数，即核心协程数
    min_size: AtomicUsize,
    //最大协程数
    max_size: AtomicUsize,
    //非核心协程的最大存活时间，单位ns
    keep_alive_time: AtomicU64,
    //阻滞器
    blocker: RefCell<Box<dyn Blocker + 'p>>,
    //任务执行结果
    results: DashMap<String, Result<Option<usize>, &'p str>>,
    //正在等待结果的
    waits: DashMap<&'p str, Arc<(Mutex<bool>, Condvar)>>,
    //用于停止额外线程
    stop: Arc<(Mutex<bool>, Condvar)>,
}

impl Drop for CoroutinePoolImpl<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(
                0,
                self.get_running_size(),
                "There are still tasks in progress !"
            );
            assert_eq!(0, self.size(), "There are still tasks to be carried out !");
        }
    }
}

unsafe impl Send for CoroutinePoolImpl<'_> {}

unsafe impl Sync for CoroutinePoolImpl<'_> {}

impl RefUnwindSafe for CoroutinePoolImpl<'_> {}

impl Named for CoroutinePoolImpl<'_> {
    fn get_name(&self) -> &str {
        unsafe { (*self.workers.get()).get_name() }
    }
}

impl Default for CoroutinePoolImpl<'_> {
    fn default() -> Self {
        let blocker = crate::blocker::SleepBlocker::default();
        Self::new(
            format!("open-coroutine-pool-{}", uuid::Uuid::new_v4()),
            1,
            crate::coroutine::DEFAULT_STACK_SIZE,
            0,
            65536,
            0,
            blocker,
        )
    }
}

impl Eq for CoroutinePoolImpl<'_> {}

impl PartialEq for CoroutinePoolImpl<'_> {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            (*self.workers.get())
                .get_name()
                .eq((*other.workers.get()).get_name())
        }
    }
}

thread_local! {
    static COROUTINE_POOL: RefCell<VecDeque<*const c_void>> = RefCell::new(VecDeque::new());
}

impl<'p> Current<'p> for CoroutinePoolImpl<'p> {
    #[allow(clippy::ptr_as_ptr)]
    fn init_current(current: &Self)
    where
        Self: Sized,
    {
        COROUTINE_POOL.with(|s| {
            s.borrow_mut()
                .push_front(current as *const _ as *const c_void);
        });
    }

    fn current() -> Option<&'p Self>
    where
        Self: Sized,
    {
        COROUTINE_POOL.with(|s| {
            s.borrow()
                .front()
                .map(|ptr| unsafe { &*(*ptr).cast::<CoroutinePoolImpl<'p>>() })
        })
    }

    fn clean_current()
    where
        Self: Sized,
    {
        COROUTINE_POOL.with(|s| _ = s.borrow_mut().pop_front());
    }
}

impl<'p> Pool<'p, JoinHandleImpl<'p>> for CoroutinePoolImpl<'p> {
    fn get_state(&self) -> PoolState {
        self.state.get()
    }

    fn change_state(&self, state: PoolState) -> PoolState {
        self.state.replace(state)
    }

    fn set_min_size(&self, min_size: usize) {
        self.min_size.store(min_size, Ordering::Release);
    }

    fn get_min_size(&self) -> usize {
        self.min_size.load(Ordering::Acquire)
    }

    fn get_running_size(&self) -> usize {
        self.running.load(Ordering::Acquire)
    }

    fn set_max_size(&self, max_size: usize) {
        self.max_size.store(max_size, Ordering::Release);
    }

    fn get_max_size(&self) -> usize {
        self.max_size.load(Ordering::Acquire)
    }

    fn set_keep_alive_time(&self, keep_alive_time: u64) {
        self.keep_alive_time
            .store(keep_alive_time, Ordering::Release);
    }

    fn get_keep_alive_time(&self) -> u64 {
        self.keep_alive_time.load(Ordering::Acquire)
    }

    fn size(&self) -> usize {
        self.task_queue.len()
    }

    #[allow(box_pointers)]
    fn wait_result(
        &self,
        task_name: &str,
        wait_time: Duration,
    ) -> std::io::Result<Option<(String, Result<Option<usize>, &str>)>> {
        let key = Box::leak(Box::from(task_name));
        if let Some(r) = self.try_get_result(key) {
            _ = self.waits.remove(key);
            return Ok(Some(r));
        }
        if SchedulableCoroutine::current().is_some() {
            let timeout_time = open_coroutine_timer::get_timeout_time(wait_time);
            loop {
                _ = self.try_run();
                if let Some(r) = self.try_get_result(key) {
                    return Ok(Some(r));
                }
                if timeout_time.saturating_sub(open_coroutine_timer::now()) == 0 {
                    return Err(Error::new(ErrorKind::TimedOut, "wait timeout"));
                }
            }
        }
        let arc = if let Some(arc) = self.waits.get(key) {
            arc.clone()
        } else {
            let arc = Arc::new((Mutex::new(true), Condvar::new()));
            assert!(self.waits.insert(key, arc.clone()).is_none());
            arc
        };
        let (lock, cvar) = &*arc;
        _ = cvar
            .wait_timeout_while(lock.lock().unwrap(), wait_time, |&mut pending| pending)
            .unwrap();
        if let Some(r) = self.try_get_result(key) {
            assert!(self.waits.remove(key).is_some());
            return Ok(Some(r));
        }
        Err(Error::new(ErrorKind::TimedOut, "wait timeout"))
    }

    #[allow(box_pointers)]
    fn submit_raw(&self, task: TaskImpl<'p>) -> JoinHandleImpl<'p> {
        let task_name = Box::leak(Box::from(task.get_name()));
        self.task_queue.push(task);
        JoinHandleImpl::new(self, task_name)
    }

    fn pop(&self) -> Option<TaskImpl<'p>> {
        // Fast path, if len == 0, then there are no values
        if self.is_empty() {
            return None;
        }
        loop {
            match self.task_queue.steal() {
                Steal::Success(item) => return Some(item),
                Steal::Retry => continue,
                Steal::Empty => return None,
            }
        }
    }

    #[allow(box_pointers)]
    fn change_blocker(&self, blocker: impl Blocker + 'p) -> Box<dyn Blocker>
    where
        'p: 'static,
    {
        self.blocker.replace(Box::new(blocker))
    }

    fn start(self) -> std::io::Result<Arc<Self>>
    where
        'p: 'static,
    {
        loop {
            #[allow(box_pointers)]
            if let Ok(blocker) = self.blocker.try_borrow() {
                if crate::blocker::SLEEP_BLOCKER_NAME == blocker.get_name() {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "You need change to another blocker !",
                    ));
                }
                break;
            }
        }
        assert_eq!(PoolState::Created, self.change_state(PoolState::Running));
        let arc = Arc::new(self);
        let consumer = arc.clone();
        let join_handle = std::thread::Builder::new()
            .name(format!("open-coroutine-pool-{}", arc.get_name()))
            .spawn(move || {
                // thread per core
                _ = core_affinity::set_for_current(core_affinity::CoreId { id: consumer.cpu });
                loop {
                    match consumer.get_state() {
                        PoolState::Running => {
                            _ = consumer.try_timed_schedule(Duration::from_millis(10));
                        }
                        PoolState::Stopping(true) => {
                            while !consumer.is_empty() || consumer.get_running_size() > 0 {
                                _ = consumer.try_timed_schedule(Duration::from_millis(10));
                            }
                            let (lock, cvar) = &*consumer.stop.clone();
                            let mut pending = lock.lock().unwrap();
                            *pending = false;
                            // Notify the condvar that the value has changed.
                            cvar.notify_one();
                            return;
                        }
                        _ => unreachable!("never happens"),
                    }
                }
            })
            .map_err(|e| Error::new(ErrorKind::Other, format!("{e:?}")))?;
        std::mem::forget(join_handle);
        Ok(arc)
    }

    fn stop(&self, wait_time: Duration) -> std::io::Result<()> {
        let state = self.get_state();
        if PoolState::Stopped == state {
            return Ok(());
        }
        _ = self.try_timed_schedule(Duration::ZERO)?;
        if PoolState::Running == state {
            assert_eq!(
                PoolState::Running,
                self.change_state(PoolState::Stopping(true))
            );
            //开启了单独的线程
            let (lock, cvar) = &*self.stop;
            let result = cvar
                .wait_timeout_while(lock.lock().unwrap(), wait_time, |&mut pending| pending)
                .unwrap();
            if result.1.timed_out() {
                return Err(Error::new(ErrorKind::TimedOut, "stop timeout !"));
            }
            assert_eq!(
                PoolState::Stopping(true),
                self.change_state(PoolState::Stopped)
            );
            return Ok(());
        }
        assert_eq!(
            PoolState::Created,
            self.change_state(PoolState::Stopping(false))
        );
        let mut left = wait_time;
        loop {
            let left_time = self.try_timed_schedule(left)?;
            if self.is_empty() && self.get_running_size() == 0 {
                assert_eq!(
                    PoolState::Stopping(false),
                    self.change_state(PoolState::Stopped)
                );
                return Ok(());
            }
            if left_time == 0 {
                return Err(Error::new(ErrorKind::TimedOut, "stop timeout !"));
            }
            left = Duration::from_nanos(left_time);
        }
    }
}

impl<'p> CoroutinePool<'p> for CoroutinePoolImpl<'p> {
    #[allow(box_pointers)]
    fn new(
        name: String,
        cpu: usize,
        stack_size: usize,
        min_size: usize,
        max_size: usize,
        keep_alive_time: u64,
        blocker: impl Blocker + 'p,
    ) -> Self
    where
        Self: Sized,
    {
        let mut pool = CoroutinePoolImpl {
            cpu,
            state: Cell::new(PoolState::Created),
            workers: UnsafeCell::new(SchedulerImpl::new(name, stack_size)),
            scheduling: AtomicBool::new(false),
            running: AtomicUsize::new(0),
            pop_fail_times: AtomicUsize::new(0),
            min_size: AtomicUsize::new(min_size),
            max_size: AtomicUsize::new(max_size),
            task_queue: Injector::default(),
            keep_alive_time: AtomicU64::new(keep_alive_time),
            blocker: RefCell::new(Box::new(blocker)),
            results: DashMap::new(),
            waits: DashMap::new(),
            stop: Arc::new((Mutex::new(true), Condvar::new())),
        };
        pool.init();
        pool
    }

    #[allow(box_pointers)]
    fn init(&mut self) {
        unsafe { (*self.workers.get()).add_listener(CoroutineCreator::default()) };
    }

    fn set_stack_size(&self, stack_size: usize) {
        unsafe { (*self.workers.get()).set_stack_size(stack_size) };
    }

    fn try_resume(&self, co_name: &'p str) -> std::io::Result<()> {
        unsafe { (*self.workers.get()).try_resume(co_name) }
    }

    fn try_run(&self) -> Option<()> {
        #[allow(box_pointers)]
        self.pop().map(|task| {
            let (task_name, result) = task.run();
            assert!(
                self.results.insert(task_name.clone(), result).is_none(),
                "The previous result was not retrieved in a timely manner"
            );
            if let Some(arc) = self.waits.get(&*task_name) {
                let (lock, cvar) = &**arc;
                let mut pending = lock.lock().unwrap();
                *pending = false;
                // Notify the condvar that the value has changed.
                cvar.notify_one();
            }
        })
    }

    fn grow(&self, should_grow: bool) -> std::io::Result<()> {
        if !should_grow || self.is_empty() || self.get_running_size() >= self.get_max_size() {
            return Ok(());
        }
        let create_time = open_coroutine_timer::now();
        unsafe {
            (*self.workers.get()).submit(
                move |suspender, ()| {
                    loop {
                        let pool = Self::current().expect("current pool not found");
                        if pool.try_run().is_some() {
                            pool.pop_fail_times.store(0, Ordering::Release);
                            continue;
                        }
                        let recycle = match pool.get_state() {
                            PoolState::Created | PoolState::Running => false,
                            PoolState::Stopping(_) | PoolState::Stopped => true,
                        };
                        let running = pool.get_running_size();
                        if open_coroutine_timer::now().saturating_sub(create_time)
                            >= pool.get_keep_alive_time()
                            && running > pool.get_min_size()
                            || recycle
                        {
                            //回收worker协程
                            pool.running
                                .store(running.saturating_sub(1), Ordering::Release);
                            return;
                        }
                        _ = pool.pop_fail_times.fetch_add(1, Ordering::Release);
                        match pool.pop_fail_times.load(Ordering::Acquire).cmp(&running) {
                            //让出CPU给下一个协程
                            std::cmp::Ordering::Less => suspender.suspend(),
                            //减少CPU在N个无任务的协程中空轮询
                            std::cmp::Ordering::Equal | std::cmp::Ordering::Greater => {
                                loop {
                                    #[allow(box_pointers)]
                                    if let Ok(blocker) = pool.blocker.try_borrow() {
                                        blocker.block(Duration::from_millis(1));
                                        break;
                                    }
                                }
                                pool.pop_fail_times.store(0, Ordering::Release);
                            }
                        }
                    }
                },
                None,
            )?;
        }
        _ = self.running.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn try_timeout_schedule(&self, timeout_time: u64) -> std::io::Result<u64> {
        if self
            .scheduling
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Self::init_current(self);
            let should_grow = match self.get_state() {
                PoolState::Created | PoolState::Running => true,
                PoolState::Stopping(_) | PoolState::Stopped => false,
            };
            self.grow(should_grow).map_err(|e| {
                self.scheduling.store(false, Ordering::Release);
                e
            })?;
            let result = unsafe { (*self.workers.get()).try_timeout_schedule(timeout_time) };
            Self::clean_current();
            self.scheduling.store(false, Ordering::Release);
            return result;
        }
        Ok(timeout_time.saturating_sub(open_coroutine_timer::now()))
    }

    fn try_get_result(&self, task_name: &str) -> Option<(String, Result<Option<usize>, &str>)> {
        self.results.remove(task_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coroutine::suspender::SimpleDelaySuspender;
    use crate::scheduler::SchedulableSuspender;

    #[test]
    fn test_simple() {
        let task_name = "test_simple";
        let pool = CoroutinePoolImpl::default();
        pool.set_max_size(1);
        assert!(pool.is_empty());
        _ = pool.submit(
            Some(String::from("test_panic")),
            |_| panic!("test panic, just ignore it"),
            None,
        );
        assert!(!pool.is_empty());
        let name = pool.submit(
            Some(String::from(task_name)),
            |_| {
                println!("2");
                Some(2)
            },
            None,
        );
        assert_eq!(task_name, name.get_name().unwrap());
        _ = pool.try_schedule();
        assert_eq!(
            Some((
                String::from("test_panic"),
                Err("test panic, just ignore it")
            )),
            pool.try_get_result("test_panic")
        );
        assert_eq!(
            Some((String::from(task_name), Ok(Some(2)))),
            pool.try_get_result(task_name)
        );
    }

    #[test]
    fn test_current() -> std::io::Result<()> {
        let parent_name = "parent";
        let pool = CoroutinePoolImpl::new(
            String::from(parent_name),
            1,
            crate::coroutine::DEFAULT_STACK_SIZE,
            0,
            65536,
            0,
            crate::blocker::SleepBlocker::default(),
        );
        _ = pool.submit(
            None,
            |_| {
                assert!(SchedulableCoroutine::current().is_some());
                assert!(SchedulableSuspender::current().is_some());
                assert!(SchedulerImpl::current().is_some());
                assert_eq!(
                    parent_name,
                    CoroutinePoolImpl::current().unwrap().get_name()
                );
                assert_eq!(
                    parent_name,
                    CoroutinePoolImpl::current().unwrap().get_name()
                );

                let child_name = "child";
                let pool = CoroutinePoolImpl::new(
                    String::from(child_name),
                    1,
                    crate::coroutine::DEFAULT_STACK_SIZE,
                    0,
                    65536,
                    0,
                    crate::blocker::SleepBlocker::default(),
                );
                _ = pool.submit(
                    None,
                    |_| {
                        assert!(SchedulableCoroutine::current().is_some());
                        assert!(SchedulableSuspender::current().is_some());
                        assert!(SchedulerImpl::current().is_some());
                        assert_eq!(child_name, CoroutinePoolImpl::current().unwrap().get_name());
                        assert_eq!(child_name, CoroutinePoolImpl::current().unwrap().get_name());
                        None
                    },
                    None,
                );
                pool.try_schedule().unwrap();

                assert_eq!(
                    parent_name,
                    CoroutinePoolImpl::current().unwrap().get_name()
                );
                assert_eq!(
                    parent_name,
                    CoroutinePoolImpl::current().unwrap().get_name()
                );
                None
            },
            None,
        );
        pool.try_schedule()
    }

    #[test]
    fn test_suspend() -> std::io::Result<()> {
        let pool = CoroutinePoolImpl::default();
        pool.set_max_size(2);
        _ = pool.submit(
            None,
            |param| {
                println!("[coroutine] delay");
                if let Some(suspender) = SchedulableSuspender::current() {
                    suspender.delay(Duration::from_millis(100));
                }
                println!("[coroutine] back");
                param
            },
            None,
        );
        _ = pool.submit(
            None,
            |_| {
                println!("middle");
                Some(1)
            },
            None,
        );
        pool.try_schedule()?;
        std::thread::sleep(Duration::from_millis(200));
        pool.try_schedule()
    }

    #[test]
    fn test_wait() {
        let task_name = "test_wait";
        let pool = CoroutinePoolImpl::default();
        pool.set_max_size(1);
        assert!(pool.is_empty());
        let name = pool.submit(
            Some(String::from(task_name)),
            |_| {
                println!("2");
                Some(2)
            },
            None,
        );
        assert_eq!(task_name, name.get_name().unwrap());
        assert_eq!(None, pool.try_get_result(task_name));
        match pool.wait_result(task_name, Duration::from_millis(100)) {
            Ok(_) => panic!(),
            Err(_) => {}
        }
        assert_eq!(None, pool.try_get_result(task_name));
        _ = pool.try_schedule();
        match pool.wait_result(task_name, Duration::from_secs(100)) {
            Ok(v) => assert_eq!(Some((String::from(task_name), Ok(Some(2)))), v),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn test_co_simple() -> std::io::Result<()> {
        let mut scheduler = SchedulerImpl::default();
        scheduler.submit(
            |_, _| {
                let task_name = "test_co_simple";
                let pool = CoroutinePoolImpl::default();
                pool.set_max_size(1);
                let result = pool.submit_and_wait(
                    Some(String::from(task_name)),
                    |_| Some(1),
                    None,
                    Duration::from_secs(1),
                );
                assert_eq!(
                    Some((String::from(task_name), Ok(Some(1)))),
                    result.unwrap()
                );
            },
            None,
        )?;
        scheduler.try_schedule()
    }

    #[test]
    fn test_nest() {
        let pool = Arc::new(CoroutinePoolImpl::default());
        pool.set_max_size(1);
        let arc = pool.clone();
        _ = pool.submit_and_wait(
            None,
            move |_| {
                println!("start");
                _ = arc.submit_and_wait(
                    None,
                    |_| {
                        println!("middle");
                        None
                    },
                    None,
                    Duration::from_secs(1),
                );
                println!("end");
                None
            },
            None,
            Duration::from_secs(1),
        );
    }

    #[test]
    fn test_stop() -> std::io::Result<()> {
        let pool = CoroutinePoolImpl::default();
        pool.set_max_size(1);
        _ = pool.submit(None, |_| panic!("test panic, just ignore it"), None);
        _ = pool.submit(
            None,
            |_| {
                println!("2");
                Some(2)
            },
            None,
        );
        pool.stop(Duration::from_secs(1))
    }

    #[allow(box_pointers)]
    #[test]
    fn test_simple_auto() -> std::io::Result<()> {
        let pool = CoroutinePoolImpl::default();
        _ = pool.change_blocker(crate::blocker::DelayBlocker::default());
        let pool = pool.start()?;
        pool.set_max_size(1);
        _ = pool.submit(None, |_| panic!("test panic, just ignore it"), None);
        _ = pool.submit(
            None,
            |_| {
                println!("2");
                Some(2)
            },
            None,
        );
        pool.stop(Duration::from_secs(3))
    }

    #[allow(box_pointers)]
    #[test]
    fn test_wait_auto() -> std::io::Result<()> {
        let pool = CoroutinePoolImpl::default();
        _ = pool.change_blocker(crate::blocker::DelayBlocker::default());
        let pool = pool.start()?;
        pool.set_max_size(1);
        let task_name = uuid::Uuid::new_v4().to_string();
        _ = pool.submit(None, |_| panic!("test panic, just ignore it"), None);
        let result = pool.submit_and_wait(
            Some(task_name.clone()),
            |_| {
                println!("2");
                Some(2)
            },
            None,
            Duration::from_secs(3),
        );
        assert_eq!(Some((task_name, Ok(Some(2)))), result.unwrap());
        pool.stop(Duration::from_secs(3))
    }
}
