pub mod browser;
pub mod download;
pub mod fetch;
pub mod intercept;
pub mod tap;
pub mod transform;

pub use browser::register_browser_steps;
pub use download::register_download_steps;
pub use fetch::register_fetch_steps;
pub use intercept::register_intercept_steps;
pub use tap::register_tap_steps;
pub use transform::register_transform_steps;

use crate::step_registry::StepRegistry;

pub fn register_all_steps(registry: &mut StepRegistry) {
    register_transform_steps(registry);
    register_fetch_steps(registry);
    register_browser_steps(registry);
    register_intercept_steps(registry);
    register_tap_steps(registry);
    register_download_steps(registry);
}
