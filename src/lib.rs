#![doc = include_str!("../README.md")]

pub mod _release_history {
    #![doc = include_str!("../NEWS.md")]
}

mod service_run;
pub use service_run::*;

#[cfg(feature = "service-gen")]
mod service_gen;

#[cfg(feature = "service-gen")]
pub use service_gen::TwirpServiceGenerator;
