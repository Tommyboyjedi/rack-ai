pub mod speech;
pub mod speech_backend;
pub mod speech_gateway;
pub mod admission;
pub mod api;
pub mod backend;
pub mod config;
pub mod container;
pub mod control;
pub mod dispatch;
pub mod hosting;
pub mod idle;
pub mod inference;
pub mod media;
mod media_idle;
pub mod planner;
pub mod preflight;
pub mod process;
pub mod protocol;
pub mod retirement;
pub mod scoped_gateway;
pub mod service;
pub mod supervisor;
pub mod transition;
pub mod types;
pub mod validation;

pub mod media_limits;

pub mod capacity;
pub mod workers;

pub mod workspace_scope;
pub mod workspace_scope_api;

mod teardown;

mod network;

#[cfg(test)]
mod idle_tests;

mod container_socket;

mod gpu_cleanup;

mod reference_audio;
mod voice_registry;
mod voice_registration;
