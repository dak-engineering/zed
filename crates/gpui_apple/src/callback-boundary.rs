use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
};

enum Failure<E> {
    Error(E),
    Panic(Box<dyn Any + Send>),
}

pub(super) struct CallbackBoundary<T, E> {
    value: T,
    failure: Option<Failure<E>>,
}

impl<T, E> CallbackBoundary<T, E> {
    pub(super) fn new(value: T) -> Self {
        Self {
            value,
            failure: None,
        }
    }

    pub(super) fn accept(&mut self, action: impl FnOnce(&mut T) -> Result<(), E>) -> bool {
        if self.failure.is_some() {
            return false;
        }
        match catch_unwind(AssertUnwindSafe(|| action(&mut self.value))) {
            Ok(Ok(())) => true,
            Ok(Err(error)) => {
                self.failure = Some(Failure::Error(error));
                false
            }
            Err(panic) => {
                self.failure = Some(Failure::Panic(panic));
                false
            }
        }
    }

    pub(super) fn finish(self) -> Result<T, E> {
        match self.failure {
            None => Ok(self.value),
            Some(Failure::Error(error)) => Err(error),
            Some(Failure::Panic(panic)) => resume_unwind(panic),
        }
    }
}
