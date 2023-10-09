use crate::coroutine::Named;
use std::cell::Cell;
use std::fmt::{Debug, Formatter};
use std::panic::UnwindSafe;

/// A trait implemented for describing task.
/// Note: the param and the result is raw pointer.
pub trait Task<'t>: Named + UnwindSafe {
    /// Create a new `Task` instance.
    fn new(
        name: String,
        func: impl FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 't,
        param: Option<usize>,
    ) -> Self;

    /// Set a param for this task.
    fn set_param(&self, param: usize) -> Option<usize>;

    /// Get param from this task.
    fn get_param(&self) -> Option<usize>;

    /// exec the task
    ///
    /// # Errors
    /// if an exception occurred while executing this task.
    fn run<'e>(self) -> (String, Result<Option<usize>, &'e str>);
}

#[repr(C)]
#[allow(clippy::type_complexity, box_pointers, missing_docs)]
pub struct TaskImpl<'t> {
    name: String,
    func: Box<dyn FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 't>,
    param: Cell<Option<usize>>,
}

impl UnwindSafe for TaskImpl<'_> {}

impl Debug for TaskImpl<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("name", &self.name)
            .field("param", &self.param)
            .finish_non_exhaustive()
    }
}

impl Named for TaskImpl<'_> {
    fn get_name(&self) -> &str {
        &self.name
    }
}

impl<'t> Task<'t> for TaskImpl<'t> {
    #[allow(box_pointers)]
    fn new(
        name: String,
        func: impl FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 't,
        param: Option<usize>,
    ) -> Self {
        TaskImpl {
            name,
            func: Box::new(func),
            param: Cell::new(param),
        }
    }

    fn set_param(&self, param: usize) -> Option<usize> {
        self.param.replace(Some(param))
    }

    fn get_param(&self) -> Option<usize> {
        self.param.get()
    }

    #[allow(box_pointers)]
    fn run<'e>(self) -> (String, Result<Option<usize>, &'e str>) {
        let paran = self.get_param();
        (
            self.name.clone(),
            std::panic::catch_unwind(|| (self.func)(paran)).map_err(|e| {
                let message = *e
                    .downcast_ref::<&'static str>()
                    .unwrap_or(&"task failed without message");
                crate::error!("task:{} finish with error:{}", self.name, message);
                message
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let task = TaskImpl::new(
            String::from("test"),
            |p| {
                println!("hello");
                p
            },
            None,
        );
        assert_eq!((String::from("test"), Ok(None)), task.run());
    }

    #[test]
    fn test_panic() {
        let task = TaskImpl::new(
            String::from("test"),
            |_| {
                panic!("test panic, just ignore it");
            },
            None,
        );
        assert_eq!(
            (String::from("test"), Err("test panic, just ignore it")),
            task.run()
        );
    }
}
