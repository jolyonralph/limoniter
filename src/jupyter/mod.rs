//! Jupyter kernels: finding them, starting them and talking to them.
//!
//! `kernel_specs` lists the kernels on this computer (with `specs::labels` for a list of them, and
//! `specs::install_command` for when there are none), and `start` starts one, giving the notebook a
//! `KernelLink`. `kernel::start_kernel` gives the `Kernel` itself, which can also complete and inspect.

pub mod kernel;
pub mod link;
pub mod process;
pub mod specs;
pub mod wire;
pub mod zmtp;

#[cfg(test)]
mod kernel_tests;

use link::{KernelLink, KernelSpec};
use std::path::Path;

pub use kernel::set_repaint;
pub use specs::kernel_specs;

/// Starts a kernel in this folder, as `notebook::StartKernel` wants. It returns once the program has started;
/// the kernel says it's ready with `Status(Idle)`, and code given before then waits for it.
pub fn start(spec: &KernelSpec, cwd: &Path) -> Result<Box<dyn KernelLink>, String> {
    // A Python kernel runs in the Python environment chosen for the files being worked on, if it has ipykernel.
    let spec = crate::py_env::kernel_for_chosen(spec);
    Ok(Box::new(kernel::start_kernel(&spec, cwd)?))
}
