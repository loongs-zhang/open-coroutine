use crate::common::{Current, Pool};
use crate::constants::CoroutineState;
use crate::coroutine::listener::Listener;
use crate::pool::{CoroutinePool, CoroutinePoolImpl};
use crate::scheduler::{SchedulableCoroutine, SchedulableCoroutineState, SchedulableListener};
use std::sync::atomic::Ordering;

#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct CoroutineCreator {}

impl Listener<(), (), Option<usize>> for CoroutineCreator {
    fn on_state_changed(
        &self,
        _: &SchedulableCoroutine,
        _: SchedulableCoroutineState,
        new_state: SchedulableCoroutineState,
    ) {
        match new_state {
            CoroutineState::Suspend((), _) | CoroutineState::SystemCall((), _, _) => {
                if let Some(pool) = CoroutinePoolImpl::current() {
                    _ = pool.try_grow();
                }
            }
            CoroutineState::Complete(_) => {
                if let Some(pool) = CoroutinePoolImpl::current() {
                    //worker协程正常退出
                    pool.running
                        .store(pool.get_running_size().saturating_sub(1), Ordering::Release);
                }
            }
            CoroutineState::Error(_) => {
                if let Some(pool) = CoroutinePoolImpl::current() {
                    //worker协程异常退出，需要先回收再创建
                    pool.running
                        .store(pool.get_running_size().saturating_sub(1), Ordering::Release);
                    _ = pool.try_grow();
                }
            }
            _ => {}
        }
    }
}

impl SchedulableListener for CoroutineCreator {}
