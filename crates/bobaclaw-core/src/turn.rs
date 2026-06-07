use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("turn interrupted")]
pub struct TurnInterrupted;
