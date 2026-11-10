use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget, SymbolKind};

mod mach_o {
    pub const CODE_SECTION: &str = "__text";
}

pub(super) struct Stencil<'a> {
    pub(super) function_name: &'a str,
    pub(super) code: &'a [u8],
    pub(super) patches: Vec<Patch<'a>>,
}

pub(super) struct Patch<'a> {
    pub(super) offset: u64,
    pub(super) symbol: &'a str,
    pub(super) relocation: object::Relocation,
}

pub(super) fn into_stencils(object_bytes: &[u8]) -> Vec<Stencil<'_>> {
    let object = object::File::parse(object_bytes).unwrap();

    let text_section = object
        .section_by_name(mach_o::CODE_SECTION)
        .expect("machine code does not have code section");

    // collect all symbols in text section
    let mut functions = Vec::new();
    for sym in object.symbols() {
        if sym.section_index() != Some(text_section.index())
            || sym.kind() != SymbolKind::Text
            || !sym.is_global()
        {
            continue;
        }

        let name = sym.name().expect("invalid function name");
        let relative_start_offset = sym.address() - text_section.address();

        functions.push((name, relative_start_offset))
    }

    functions.sort_by_key(|&(_, o)| o);

    // split code up by functions
    let code = text_section.data().unwrap();

    let mut out = Vec::with_capacity(functions.len());

    for (i, &(function_name, relative_start_offset)) in functions.iter().enumerate() {
        let relative_end_offset = functions
            .get(i + 1)
            .map(|&(_, o)| o)
            .unwrap_or_else(|| u64::try_from(code.len()).unwrap());

        let mut patches = Vec::new();
        {
            let relocations_within_function_range =
                text_section.relocations().filter(|&(offset, _)| {
                    relative_start_offset <= offset && offset < relative_end_offset
                });

            for (offset, relocation) in relocations_within_function_range {
                let sym_index = match relocation.target() {
                    RelocationTarget::Symbol(i) => i,
                    _ => panic!("unsupported relocation target"),
                };

                let sym = object.symbol_by_index(sym_index).unwrap();

                let name = sym.name().expect("invalid symbol name");
                let local_offset = offset - relative_start_offset;

                println!(
                    "cargo::warning={function_name}: patch at {local_offset}, \
                        target {name}, encoding {:?}",
                    relocation.flags(),
                );

                patches.push(Patch {
                    offset: local_offset,
                    symbol: name,
                    relocation,
                });
            }
        }

        let bytes = &code[relative_start_offset as usize..relative_end_offset as usize];

        out.push(Stencil {
            function_name,
            code: bytes,
            patches,
        });

        println!(
            "cargo::warning=stencil {function_name}: {} bytes",
            bytes.len()
        );
    }

    out
}
