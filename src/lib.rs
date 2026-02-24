#![deny(clippy::all)]

use napi_derive::napi;

#[cfg(any(
  all(target_os = "windows", target_arch = "x86_64"),
  all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
  ),
  all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
  )
))]
mod full_impl;

#[cfg(any(
  all(target_os = "windows", target_arch = "x86_64"),
  all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
  ),
  all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
  )
))]
pub use full_impl::*;

// Keep a minimal API on all targets so CI/test matrices can compile and run.
#[napi]
pub fn plus_100(input: u32) -> u32 {
  input + 100
}
