//! What a notebook (and, later, a script's `# %%` cells, the R console and the data viewer) needs from a running
//! Jupyter kernel, kept apart from how the kernel is reached, so the notebook can be built and tested with a
//! kernel of its own making.
//!
//! Outputs travel as nbformat 4 output dictionaries (`{"output_type": "stream", "name": "stdout", "text": "…"}`,
//! `display_data`, `execute_result`, `error`), exactly as they're stored in an `.ipynb` file, so a notebook keeps
//! what it's given without translating it.

use std::path::PathBuf;

/// A kernel Jupyter knows of, read from a `kernel.json` in one of Jupyter's data folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelSpec {
    /// The folder's name, which notebooks name in `metadata.kernelspec.name` ("python3", "ir").
    pub name: String,
    /// What the person sees ("Python 3 (ipykernel)", "R").
    pub display_name: String,
    /// The language, lower case ("python", "R" is given as "R" by IRkernel: compare without case).
    pub language: String,
    /// The command that starts it, with `{connection_file}` still in it.
    pub argv: Vec<String>,
    /// Extra environment variables for it.
    pub env: Vec<(String, String)>,
    /// The folder `kernel.json` is in.
    pub dir: PathBuf,
}

/// Whether the kernel is working, as its `status` messages say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelStatus {
    Starting,
    Idle,
    Busy,
    /// Stopped, or never answered; `KernelEvent::Died` says why.
    Dead,
}

/// Something that happened in the kernel, collected by `KernelLink::poll`. `request` is the number `execute`
/// gave back, so each output goes to the cell that asked.
#[derive(Clone, Debug, PartialEq)]
pub enum KernelEvent {
    Status(KernelStatus),
    /// One nbformat output for a request. A `stream` output may come in many pieces: join consecutive ones of the
    /// same name, as Jupyter does.
    Output { request: u64, output: serde_json::Value },
    /// `clear_output`: with `wait`, clear when the next output comes rather than now.
    ClearOutput { request: u64, wait: bool },
    /// An earlier `display_data` with this `display_id` is to be replaced by `output` (`update_display_data`).
    UpdateDisplay { display_id: String, output: serde_json::Value },
    /// The request has finished. `ok` is false after an error or an interrupt.
    Done { request: u64, execution_count: Option<u32>, ok: bool },
    /// The code asked for typed input (Python's `input()`); answer with `send_input`.
    InputRequest { request: u64, prompt: String, password: bool },
    /// The kernel stopped or couldn't be reached, in words for the person.
    Died(String),
}

/// A running kernel, as the notebook uses it. Every call returns at once; what the kernel does comes back
/// through `poll`, which the window calls each frame (the link asks egui to repaint when something arrives).
pub trait KernelLink: Send {
    /// Runs code, returning the number its events will carry.
    fn execute(&mut self, code: &str) -> u64;
    /// Runs code for the window's own use, such as listing the variables: it isn't kept in the kernel's history,
    /// doesn't count as an execution, and doesn't stop the cells queued after it if it fails. Its events carry
    /// the number returned, as `execute`'s do.
    fn execute_quietly(&mut self, code: &str) -> u64 {
        self.execute(code)
    }
    /// Everything that has happened since the last call, oldest first.
    fn poll(&mut self) -> Vec<KernelEvent>;
    fn status(&self) -> KernelStatus;
    /// Stops what's running (an interrupt request, or SIGINT where the kernel asks for a signal).
    fn interrupt(&mut self);
    /// Starts the kernel afresh, losing its variables.
    fn restart(&mut self) -> Result<(), String>;
    /// The answer to an `InputRequest`.
    fn send_input(&mut self, text: &str);
    /// Stops the kernel for good (also done on drop).
    fn shutdown(&mut self);
    /// The kernel's language, from its `kernel_info` reply once known ("python", "R").
    fn language(&self) -> String;
}
