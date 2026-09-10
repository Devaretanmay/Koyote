pub mod sandbox;

pub mod engines;
pub mod runtime;

#[cfg(feature = "pyo3-binding")]
mod py_bindings;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn compress(content: &str) -> String {
    engines::compression::route_and_compress(content)
}
