use crate::coroutine::stack::unpooled::UnpooledStack;
use corosensei::stack::{Stack, StackPointer};

#[derive(Debug)]
pub struct PooledStack {
    expire: u64,
    stack: UnpooledStack,
}

impl PooledStack {
    pub(crate) fn new(size: usize, expire: u64) -> std::io::Result<Self> {
        Ok(PooledStack {
            expire,
            stack: UnpooledStack::new(size)?,
        })
    }

    pub fn expire(&self) -> u64 {
        self.expire
    }
}

unsafe impl Stack for PooledStack {
    #[inline]
    fn base(&self) -> StackPointer {
        self.stack.base()
    }

    #[inline]
    fn limit(&self) -> StackPointer {
        self.stack.limit()
    }

    #[cfg(windows)]
    fn teb_fields(&self) -> corosensei::stack::StackTebFields {
        self.0.teb_fields()
    }

    #[cfg(windows)]
    fn update_teb_fields(&mut self, stack_limit: usize, guaranteed_stack_bytes: usize) {
        self.0
            .update_teb_fields(stack_limit, guaranteed_stack_bytes)
    }
}
