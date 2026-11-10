mod stencils {
    include!(concat!(env!("OUT_DIR"), "/stencils_generated.rs"));
}

#[cfg(test)]
mod tests {
    use super::stencils::*;

    #[test]
    fn generated_stencils_are_embedded() {
        assert_eq!(STENCILS.len(), 3);

        for name in ["_local_get2", "_i32_add", "_return_value"] {
            assert!(STENCILS.iter().any(|stencil| stencil.name == name));
        }
    }
}
