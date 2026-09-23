mod activity_retention;
pub mod admission;
pub mod api;
pub mod backend;
pub mod config;
pub mod container;
mod contract;
pub mod control;
pub mod dispatch;
mod history_archive;
pub mod hosting;
pub mod idle;
pub mod inference;
pub mod media;
mod media_idle;
pub mod planner;
pub mod preflight;
pub mod process;
pub mod protocol;
mod residency;
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

mod native_description;
pub mod reservation;
mod reservation_admission;
mod reservation_refresh;
mod reservation_view;
mod work;
mod work_execution;
pub mod work_payload;
pub mod workspace_recovery;

mod gpu_cleanup;
mod reference_audio;
mod speech;
mod speech_backend;
mod speech_gateway;
mod voice_registration;
mod voice_registry;

mod media_evidence;
mod media_recovery;
mod owned_effect;
mod recovery;
mod recovery_media;
mod recovery_media_group;
