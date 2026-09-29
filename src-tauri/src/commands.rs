//! What the webview can ask the backend to do, one file per part of the app.
//! Every command is a thin wrapper that turns an error into the sentence the
//! window shows.

pub mod computer;
pub mod containers;
pub mod copy;
pub mod guide;
pub mod host;
pub mod machines;
pub mod settings;
pub mod stacks;
pub mod updates;

/// What a command answers: the value, or the sentence the window shows.
pub(crate) type CmdResult<T> = Result<T, String>;

/// An error with its whole chain of causes, as one line for the window.
pub(crate) fn fail(err: anyhow::Error) -> String {
    format!("{err:#}")
}
