use crate::common::Pool;
use crate::coroutine::stack::pooled::PooledStack;
use corosensei::stack::Stack;
use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub mod unpooled;

pub mod pooled;

#[derive(Debug)]
pub struct Mempool {
    //最小内存量
    min_size: AtomicUsize,
    //最大内存量
    max_size: AtomicUsize,
    //非核心内存的最大存活时间，单位ns
    keep_alive_time: AtomicU64,
    // stackSize -> memory
    hold: DashMap<usize, VecDeque<PooledStack>>,
}

impl Default for Mempool {
    fn default() -> Self {
        Mempool {
            min_size: AtomicUsize::new(4 * 1024 * 1024),
            max_size: AtomicUsize::new(usize::MAX),
            keep_alive_time: AtomicU64::new(30_000_000_000),
            hold: DashMap::default(),
        }
    }
}

impl Mempool {
    #[allow(unsafe_code, trivial_casts, box_pointers)]
    pub fn get_instance<'s>() -> &'s Mempool {
        static INSTANCE: AtomicUsize = AtomicUsize::new(0);
        let mut ret = INSTANCE.load(Ordering::Relaxed);
        if ret == 0 {
            let ptr: &'s mut Mempool = Box::leak(Box::default());
            ret = ptr as *mut Mempool as usize;
            INSTANCE.store(ret, Ordering::Relaxed);
        }
        unsafe { &*(ret as *mut Mempool) }
    }

    pub fn malloc(&self, size: usize) -> std::io::Result<PooledStack> {
        if let Some(mut queue) = self.hold.get_mut(&size) {
            if let Some(stack) = queue.pop_front() {
                return Ok(stack);
            }
            //队列为空
        }
        //队列不存在
        PooledStack::new(
            size,
            open_coroutine_timer::now().saturating_add(self.get_keep_alive_time()),
        )
    }

    pub fn revert(&self, stack: PooledStack) {
        let running = self.get_running_size();
        if running > self.get_min_size()
            && open_coroutine_timer::now().saturating_sub(stack.expire())
                >= self.get_keep_alive_time()
            || running >= self.get_max_size()
        {
            //直接把内存还给OS
            return;
        }
        let size = stack.limit().get();
        if let Some(mut queue) = self.hold.get_mut(&size) {
            queue.push_back(stack);
        } else if let Some(previous) = self.hold.insert(size, VecDeque::from(vec![stack])) {
            for s in previous {
                self.revert(s);
            }
        }
    }
}

impl Pool for Mempool {
    fn set_min_size(&self, min_size: usize) {
        self.min_size.store(min_size, Ordering::Release);
    }

    fn get_min_size(&self) -> usize {
        self.min_size.load(Ordering::Acquire)
    }

    fn get_running_size(&self) -> usize {
        let mut total = 0;
        for queue in &self.hold {
            if let Some(stack) = queue.front() {
                total += stack.limit().get() * queue.len();
            }
        }
        total
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
}
