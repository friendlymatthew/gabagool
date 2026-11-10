fn main() {
    #[cfg(feature = "core-tests")]
    core_tests::generate();

    #[cfg(not(feature = "core-tests"))]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        std::fs::write(
            std::path::Path::new(&out_dir).join("core_tests_generated.rs"),
            "",
        )
        .unwrap();
    }

    #[cfg(feature = "component-tests")]
    component_tests::generate();

    #[cfg(not(feature = "component-tests"))]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        std::fs::write(
            std::path::Path::new(&out_dir).join("component_tests_generated.rs"),
            "",
        )
        .unwrap();
    }

    #[cfg(feature = "jit")]
    stencil::generate();
}

#[cfg(feature = "component-tests")]
#[path = "build/component_tests.rs"]
mod component_tests;

#[cfg(feature = "core-tests")]
#[path = "build/core_tests.rs"]
mod core_tests;

#[cfg(feature = "jit")]
#[path = "build/stencil/mod.rs"]
mod stencil;
