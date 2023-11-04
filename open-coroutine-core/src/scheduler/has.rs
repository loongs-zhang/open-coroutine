use crate::common::Named;
use crate::coroutine::suspender::Suspender;
use crate::scheduler::join::JoinHandleImpl;
use crate::scheduler::listener::Listener;
use crate::scheduler::{Scheduler, SchedulerImpl};
use std::panic::UnwindSafe;

#[allow(missing_docs, clippy::missing_errors_doc)]
pub trait HasScheduler<'s> {
    fn scheduler(&self) -> &SchedulerImpl<'s>;

    fn scheduler_mut(&mut self) -> &mut SchedulerImpl<'s>;

    fn set_stack_size(&self, stack_size: usize) {
        self.scheduler().set_stack_size(stack_size);
    }

    fn submit_co(
        &self,
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) -> Option<usize> + UnwindSafe + 's,
        stack_size: Option<usize>,
    ) -> std::io::Result<JoinHandleImpl<'s>> {
        self.scheduler().submit_co(f, stack_size)
    }

    fn try_resume(&self, co_name: &'s str) -> std::io::Result<()> {
        self.scheduler().try_resume(co_name)
    }

    fn try_schedule(&self) -> std::io::Result<()> {
        _ = self.try_timeout_schedule(std::time::Duration::MAX.as_secs())?;
        Ok(())
    }

    fn try_timed_schedule(&self, dur: std::time::Duration) -> std::io::Result<u64> {
        self.try_timeout_schedule(open_coroutine_timer::get_timeout_time(dur))
    }

    fn try_timeout_schedule(&self, timeout_time: u64) -> std::io::Result<u64> {
        self.scheduler().try_timeout_schedule(timeout_time)
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

impl<'s, HasSchedulerImpl: HasScheduler<'s>> Named for HasSchedulerImpl {
    fn get_name(&self) -> &str {
        #[allow(box_pointers)]
        Box::leak(Box::from(self.scheduler().get_name()))
    }
}
