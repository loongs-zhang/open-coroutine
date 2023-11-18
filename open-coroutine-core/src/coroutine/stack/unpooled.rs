use corosensei::stack::{DefaultStack, Stack, StackPointer};
use std::fmt::{Debug, Formatter};

/// Default stack implementation which uses `mmap`.
pub struct UnpooledStack(DefaultStack);

impl Debug for UnpooledStack {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnpooledStack")
            .field("base", &self.base())
            .field("limit", &self.limit())
            .finish()
    }
}

impl UnpooledStack {
    /// Creates a new stack which has at least the given capacity.
    pub fn new(size: usize) -> std::io::Result<Self> {
        Ok(UnpooledStack(DefaultStack::new(size)?))
    }
}

impl Default for UnpooledStack {
    fn default() -> Self {
        Self::new(crate::constants::DEFAULT_STACK_SIZE).expect("failed to allocate stack")
    }
}

unsafe impl Stack for UnpooledStack {
    #[inline]
    fn base(&self) -> StackPointer {
        self.0.base()
    }

    #[inline]
    fn limit(&self) -> StackPointer {
        self.0.limit()
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
