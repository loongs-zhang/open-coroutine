use crate::common::Blocker;
use crate::constants::PoolState;
use crate::pool::task::Task;
use crate::pool::{CoroutinePool, CoroutinePoolImpl, TaskPool};
use std::time::Duration;

#[allow(missing_docs, clippy::missing_errors_doc)]
pub trait HasCoroutinePool<'p> {
    fn pool(&self) -> &CoroutinePoolImpl<'p>;

    fn get_state(&self) -> PoolState {
        self.pool().get_state()
    }

    fn change_state(&self, state: PoolState) -> PoolState {
        self.pool().change_state(state)
    }

    fn set_min_size(&self, min_size: usize) {
        self.pool().set_min_size(min_size);
    }

    fn get_min_size(&self) -> usize {
        self.pool().get_min_size()
    }

    fn get_running_size(&self) -> usize {
        self.pool().get_running_size()
    }

    fn set_max_size(&self, max_size: usize) {
        self.pool().set_max_size(max_size);
    }

    fn get_max_size(&self) -> usize {
        self.pool().get_max_size()
    }

    fn set_keep_alive_time(&self, keep_alive_time: u64) {
        self.pool().set_keep_alive_time(keep_alive_time);
    }

    fn get_keep_alive_time(&self) -> u64 {
        self.pool().get_keep_alive_time()
    }

    fn has_task(&self) -> bool {
        self.pool().has_task()
    }

    fn count(&self) -> usize {
        self.pool().count()
    }

    fn pop(&self) -> Option<Task<'p>> {
        self.pool().pop()
    }

    #[allow(box_pointers)]
    fn change_blocker(&self, blocker: impl Blocker + 'p) -> Box<dyn Blocker>
    where
        'p: 'static,
    {
        self.pool().change_blocker(blocker)
    }

    fn try_run(&self) -> Option<()> {
        self.pool().try_run()
    }

    fn try_schedule_task(&self) -> std::io::Result<()> {
        self.pool().try_schedule_task()
    }

    fn try_timed_schedule_task(&self, dur: Duration) -> std::io::Result<u64> {
        self.pool().try_timed_schedule_task(dur)
    }

    fn try_timeout_schedule_task(&self, timeout_time: u64) -> std::io::Result<u64> {
        self.pool().try_timeout_schedule_task(timeout_time)
    }

    fn try_get_task_result(
        &'p self,
        task_name: &str,
    ) -> Option<(String, Result<Option<usize>, &str>)> {
        self.pool().try_get_task_result(task_name)
    }
}
