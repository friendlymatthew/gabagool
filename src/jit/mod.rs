core::cfg_select! {
    all(target_os = "macos", target_arch = "aarch64") => {
        include!(concat!(env!("OUT_DIR"), "/stencils_generated.rs"));

        mod arm64;
        use arm64::resolve_data_slot_addresses;

        mod codebuffer;
        mod code_layout;
        mod compiler;
        pub(crate) use compiler::{compile, supports, JitCompiledFunction};
    }
    _ => {
        compile_error!("the jit feature currently supports only arm64 macos sorry bruh");
    }
}
